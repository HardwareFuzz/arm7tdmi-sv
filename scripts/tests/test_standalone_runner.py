"""Execute real RTL without a compiler, reference simulator, or golden trace."""
import json
import os
from pathlib import Path
import resource
import struct
import subprocess
import tempfile
import unittest


BINARY = Path(os.environ.get(
    "ARM7_STANDALONE_BINARY",
    str(Path(__file__).resolve().parents[1] / "obj_dir/standalone/Varm7tdmis_standalone_tb"),
)).resolve()


def branch(pc, target):
    return 0xEA000000 | (((target - pc - 8) >> 2) & 0xFFFFFF)


@unittest.skipUnless(BINARY.is_file(), "make -C scripts standalone-build first")
class StandaloneRunnerTests(unittest.TestCase):
    def execute(self, instructions, *, stop_pc=None, memory_base=0x200,
                memory_length=16, max_cycles=100000, thumb=None, extra=(),
                program_contents=None):
        # The payload and hand-written expectations are directed tests, not
        # independent differential validation. Vectors return past each fault.
        image = bytearray(0x100 + len(instructions) * 4)
        for vector, handler in ((0, 0x100), (4, 0x80), (8, 0x84),
                                (12, 0x8C), (16, 0x88)):
            struct.pack_into("<I", image, vector, branch(vector, handler))
        for address, word in ((0x80, 0xE1B0F00E), (0x84, 0xE1B0F00E),
                              (0x88, 0xE25EF004), (0x8C, 0xE1A00000)):
            struct.pack_into("<I", image, address, word)
        for index, word in enumerate(instructions):
            struct.pack_into("<I", image, 0x100 + index * 4, word)
        if thumb:
            image.extend(bytes(max(0, 0x200 + len(thumb) * 2 - len(image))))
            for index, halfword in enumerate(thumb):
                struct.pack_into("<H", image, 0x200 + index * 2, halfword)
        image.extend(bytes((-len(image)) % 4))
        if stop_pc is None:
            stop_pc = 0x100 + (len(instructions) - 1) * 4
        with tempfile.TemporaryDirectory(prefix="arm7-standalone-test-") as directory:
            root = Path(directory)
            program = root / "program.hex"
            program.write_text("".join(
                f"{word:08x}\n" for (word,) in struct.iter_unpack("<I", image)
            ), encoding="ascii")
            if program_contents is not None:
                program.write_text(program_contents, encoding="ascii")
            result = root / "result.jsonl"
            command = [str(BINARY), f"+PROGRAM_HEX={program}",
                       f"+RESULT_JSONL={result}", f"+STOP_PC={stop_pc:x}",
                       f"+MEMORY_BASE={memory_base:x}", f"+MEMORY_LENGTH={memory_length}",
                       f"+MAX_CYCLES={max_cycles}", *extra]
            # Expected timeout/invalid-input failures must not leave core dumps.
            def disable_core_dumps():
                resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
            process = subprocess.run(
                command, cwd=root, env={**os.environ, "PATH": str(root / "no-tools")},
                capture_output=True, text=True, timeout=15, preexec_fn=disable_core_dumps,
            )
            rows = [json.loads(line) for line in result.read_text().splitlines()] if result.exists() else []
        return process, rows

    def completed(self, instructions, **kwargs):
        process, rows = self.execute(instructions, **kwargs)
        self.assertEqual(process.returncode, 0, process.stdout + process.stderr)
        self.assertEqual(rows[0]["kind"], "header")
        self.assertEqual(rows[0]["execution_backend"], "arm7tdmi-sv")
        self.assertFalse(rows[0]["reference_only"])
        result = rows[-1]
        self.assertEqual(result["kind"], "result")
        self.assertTrue(result["complete"])
        self.assertEqual(result["status"], "completed")
        events = rows[1:-1]
        self.assertTrue(all(row["kind"] == "exception" for row in events))
        self.assertEqual(result["exception_count"], len(events))
        self.assertEqual(result["exception_log_count"], len(events))
        self.assertEqual(result["exception_dropped_count"], 0)
        self.assertEqual([row["sequence"] for row in events], list(range(len(events))))
        self.assertIsNone(result["registers"]["r15"])
        self.assertEqual(result["r15_availability"], "unavailable")
        self.assertEqual(len(bytes.fromhex(result["memory"]["bytes"])), result["memory"]["length"])
        return result, events

    def test_program_updates_real_registers_and_memory_without_qemu(self):
        result, events = self.completed([
            0xE3A00005,  # mov r0, #5
            0xE3A01007,  # mov r1, #7
            0xE0802001,  # add r2, r0, r1
            0xE3A03C02,  # mov r3, #0x200
            0xE5832000,  # str r2, [r3]
            0xE1A00000,  # completion nop
        ])
        self.assertEqual(events, [])
        self.assertEqual(int(result["registers"]["r2"], 16), 12)
        self.assertEqual(result["memory"]["bytes"], "0c000000" + "00" * 12)

    def test_every_synchronous_fault_is_recorded_and_execution_continues(self):
        result, events = self.completed([
            0xE3A00005,  # mov r0, #5
            0xEF000042,  # svc
            0xE7F000F0,  # undefined
            0xEE000010,  # absent coprocessor -> undefined, no injected fault list
            0xE3500000,  # cmp r0, #0; EQ false
            0x07F000F0,  # condition-failed undefined must not trap
            0xE2800001,  # add r0, r0, #1 after all exceptions
            0xE1A00000,
        ])
        self.assertEqual([event["pc"] for event in events],
                         ["0x00000104", "0x00000108", "0x0000010c"])
        self.assertEqual([event["raw_reason_code"] for event in events], [2, 1, 1])
        self.assertEqual([event["cause"] for event in events], ["svc", "undefined", "undefined"])
        self.assertEqual(events[1]["instruction_bytes"], "f000f0e7")
        self.assertEqual(int(result["registers"]["r0"], 16), 6)

    def test_thumb_fault_pcs_and_widths(self):
        result, events = self.completed(
            [0xE59F0000, 0xE12FFF10, 0x00000201],  # ldr r0, literal; bx r0
            thumb=[0x2009, 0xDF42, 0xDE00, 0x3001, 0x46C0],
            stop_pc=0x208, memory_base=0x300,
        )
        self.assertEqual([event["pc"] for event in events], ["0x00000202", "0x00000204"])
        self.assertEqual([event["instruction_width"] for event in events], [2, 2])
        self.assertEqual([event["instruction_bytes"] for event in events], ["42df", "00de"])
        self.assertTrue(all(event["thumb"] for event in events))
        self.assertEqual(int(result["registers"]["r0"], 16), 10)

    def test_unmapped_store_aborts_without_aliasing_ram(self):
        result, events = self.completed([
            0xE3A00055, 0xE3A03201,  # r0=0x55; r3=0x10000000
            0xE5830000, 0xE2800001, 0xE1A00000,
        ], memory_base=0, memory_length=4)
        self.assertEqual([(event["pc"], event["cause"]) for event in events],
                         [("0x00000108", "data_abort")])
        self.assertEqual(events[0]["raw_reason_code"], 4)
        self.assertEqual(result["memory"]["bytes"], struct.pack("<I", branch(0, 0x100)).hex())
        self.assertEqual(int(result["registers"]["r0"], 16), 0x56)

    def test_unmapped_fetch_has_pc_without_fabricated_opcode(self):
        _, events = self.completed(
            [0xE59FF000, 0xE1A00000, 0x00040000],  # ldr pc, literal -> unmapped
            stop_pc=0x8C,
        )
        self.assertEqual([(event["pc"], event["cause"]) for event in events],
                         [("0x00040000", "prefetch_abort")])
        self.assertIsNone(events[0]["instruction_bytes"])
        self.assertIsNone(events[0]["instruction_width"])

    def test_event_stream_has_no_2048_event_array_limit(self):
        _, events = self.completed([0xEF000042] * 3000 + [0xE1A00000])
        self.assertEqual(len(events), 3000)

    def test_timeout_preserves_observed_exceptions_but_is_not_complete(self):
        process, rows = self.execute(
            [0xEF000042, branch(0x104, 0x100), 0xE1A00000], max_cycles=150,
        )
        self.assertNotEqual(process.returncode, 0)
        self.assertFalse(rows[-1]["complete"])
        self.assertEqual(rows[-1]["status"], "timeout")
        self.assertGreater(rows[-1]["exception_count"], 0)
        self.assertEqual(rows[-1]["exception_count"], len(rows) - 2)

    def test_invalid_bounds_and_limits_fail_without_a_success_report(self):
        for options in ({"memory_base": 0x3FFFF, "memory_length": 2},
                        {"max_cycles": 0}, {"max_cycles": -1}, {"stop_pc": 0x101}):
            with self.subTest(options=options):
                process, rows = self.execute([0xE1A00000], **options)
                self.assertNotEqual(process.returncode, 0)
                self.assertFalse(any(row.get("complete") is True for row in rows))

    def test_empty_and_malformed_program_images_fail_without_success(self):
        for contents in ("", "not-hex\n", "@00010000\ne1a00000\n"):
            with self.subTest(contents=contents):
                process, rows = self.execute([0xE1A00000], program_contents=contents)
                self.assertNotEqual(process.returncode, 0)
                self.assertFalse(any(row.get("complete") is True for row in rows))


if __name__ == "__main__":
    unittest.main()
