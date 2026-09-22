# Standalone program execution

The HardwareFuzz fork provides a runner that executes a supplied program on
`arm7tdmis_top` and exports the RTL's observed exceptions and final state.
It requires no QEMU executable, reference trace, expected register values,
or expected exception list. The existing QEMU differential campaigns remain
separate verification targets.

The runner is a SystemVerilog testbench compiled into a native executable by
Verilator. Python is used only by its directed regression tests. It uses the
public [VER-009 retirement interface](VERIFICATION.md#architectural-retirement-contract)
for CPU observation and the testbench memory model for RAM contents; it does
not change the synthesizable CPU RTL or the project's release status.

## Build and run

Requirements: Verilator 5.x, GNU Make, and a C++ toolchain. Build from the
repository root:

```sh
make -C scripts standalone-build STANDALONE_JOBS=8
```

This builds `scripts/obj_dir/standalone/Varm7tdmis_standalone_tb` once.
Each test case is a separate invocation of that executable. Cases can run
concurrently when they use separate result files.

Here is a complete example that needs no assembler. The `@` offsets in the
hex image are **word indices**, while runner arguments and output PCs are
**byte addresses**. The reset vector branches to byte address `0x100`, the
program computes `5 + 7`, stores the result at `0x200`, and retires a final
NOP at `0x114`:

```sh
case_dir=$(mktemp -d /tmp/arm7-standalone.XXXXXX)
cat > "$case_dir/program.hex" <<'HEX'
@00000000
ea00003e
@00000040
e3a00005
e3a01007
e0802001
e3a03c02
e5832000
e1a00000
HEX

scripts/obj_dir/standalone/Varm7tdmis_standalone_tb \
  +PROGRAM_HEX="$case_dir/program.hex" \
  +RESULT_JSONL="$case_dir/result.jsonl" \
  +STOP_PC=114 +MEMORY_BASE=200 +MEMORY_LENGTH=4 +MAX_CYCLES=10000

cat "$case_dir/result.jsonl"
```

The completed result contains `registers.r2 = "0x0000000c"` and
`memory.bytes = "0c000000"`. Results are written to the specified JSONL file;
simulator diagnostics on stdout/stderr are not part of the result protocol.

| Argument | Meaning |
|---|---|
| `+PROGRAM_HEX=path` | Required `$readmemh` image of 32-bit words. Each word is stored little endian. Sparse `@word_index` offsets are supported. RAM is zero-filled before loading. |
| `+RESULT_JSONL=path` | Required result file, created or overwritten. Use a different path for every concurrent run. |
| `+STOP_PC=hex` | Required byte address. Capture state **after** the instruction at this PC retires without an exception, then stop. Append a NOP when the intended boundary is after the last tested instruction. |
| `+MEMORY_BASE=hex` | First byte of the memory window to capture; default `10000` (hexadecimal). |
| `+MEMORY_LENGTH=decimal` | Number of bytes to capture; default `128`. Must be positive and fit in RAM. |
| `+MAX_CYCLES=decimal` | Cycle bound after reset release; default `1000000`. Must be positive. |

The memory capacity is 256 KiB, mapped at byte addresses `0x00000000` through
`0x0003ffff`. Out-of-range accesses return a bus abort rather than wrapping
around to low RAM. The testbench has a `MEMORY_WORDS` elaboration parameter
for integrations that need a different power-of-two RAM size.

## Exception handling and scope

Execution starts at the real reset vector, in ARM Supervisor state. The
input image must initialize registers and RAM as needed, install its vector
table, and provide handlers if execution should continue after an exception.
The runner observes exception entry; it does not skip the instruction or
manufacture exception returns. A missing or looping handler eventually hits
the cycle bound.

All exception pulses observed through `VER_RETIRE_EXCEPTION_VALID` after
reset release are streamed as they occur, including repeated exceptions at
the same PC. Capture does not depend on an instruction being designated as
an intentional fault in advance. There is no fixed-size event array. A
consumer interested only in generated test instructions should join events
to its instruction address map; vector and handler code can also execute.

This harness uses little-endian flat RAM, always enables the CPU clock, holds
IRQ/FIQ inactive, disables external debug, and attaches no external
coprocessor. ARMv4T ARM and Thumb execution are supported by the CPU. This
runner adds no MMU, operating system, semihosting, device model, or ARMv5+
instructions. See the existing [CPU limitations](LIMITATIONS.md) and
[unpredictable-input policies](UNPREDICTABLE.md) for implementation scope.

## JSONL result contract

The first line has `kind: "header"`, schema `arm7tdmis-standalone-v1`,
`backend` and `execution_backend` equal to `arm7tdmi-sv`,
`reference_only: false`, `byte_order: "little"`, the requested stop PC,
memory capacity, and raw reason-code namespace `arm7tdmis.exception_e-v1`.

Each `kind: "exception"` line contains a zero-based `sequence`, `cycle`,
`raw_reason_code`, `cause`, `faulting_instruction`, and `pc`. Synchronous
instruction exceptions also carry `thumb`, `instruction_width` in bytes,
`instruction`, and little-endian `instruction_bytes`. A prefetch abort has
the failed-fetch PC but null instruction/width/state fields, because no
opcode was successfully fetched. An event without a paired faulting
instruction has a null PC and null instruction fields.

The raw enum is defined in `rtl/arm7tdmis_types_pkg.sv`:

| Raw code | Cause name |
|---|---|
| 0 | `reset` |
| 1 | `undefined` |
| 2 | `svc` |
| 3 | `prefetch_abort` |
| 4 | `data_abort` |
| 5 | `irq` |
| 6 | `fiq` |

The table documents the enum, including classes not injected by this
harness. These values are **backend raw codes**, not the cross-backend
`arm-cause-code-v1` numbers. A differential adapter must explicitly normalize
their cause names; comparing raw numbers across backends is incorrect.

The last line has `kind: "result"` and contains:

- `status: "completed"` and `complete: true` on a successful stop, or
  `status: "timeout"` and `complete: false` on the cycle bound.
- `cycles`, `retired_count`, `exception_count`, `exception_log_count`, and
  `exception_dropped_count`. The runner streams every observed event and
  reports zero drops; the count must agree with the number of exception
  lines. Retirement includes condition-failed and synchronously faulting
  instruction dispositions, as defined by VER-009.
- `last_retired_pc`, separate from the architectural register file.
- `registers.r0` through `registers.r14`, selected from the active register
  bank according to the captured CPSR mode. `registers.r15` is null and
  `r15_availability` is `unavailable`: VER-009's physical slot 15 is a layout
  hole and cannot be used as PC. `last_retired_pc` must not be substituted
  for architectural r15.
- `cpsr` and `spsrs`; the SPSR array order is FIQ, IRQ, Supervisor, Abort,
  Undefined. These are storage snapshots, subject to the CPU's documented
  [reset policy](PSR.md), not proof that software initialized every register.
- `memory.address`, `memory.length`, and `memory.bytes`, taken from the
  requested RAM window. The hexadecimal byte string contains exactly twice
  `length` characters, in increasing address order.

Each event is flushed before execution continues. A timeout preserves all
events already observed and writes an incomplete state snapshot before
exiting with an error. Invalid arguments, image load failures, or output
failures also exit with an error. A process killed externally may leave
only the flushed prefix. Consumers must require exit status zero, the
expected header, a completed result, contiguous event sequences, and closed
counts; missing or partial output must never be interpreted as a successful
run with no exceptions. Timeout snapshots are diagnostic state, not completed
test results.

## Directed verification

```sh
make -C scripts standalone-test STANDALONE_JOBS=8
```

The tests execute the compiled RTL with hand-encoded programs and TRM-derived
expectations. They cover arithmetic and memory export; ARM SVC, undefined,
absent-coprocessor and condition-failed instructions; Thumb exceptions and
return; unmapped data/fetch aborts; 3,000 streamed exceptions; timeout
preservation; and invalid inputs. The executable is run with an empty tool
search path to ensure no reference simulator or compiler is launched during
execution. These are directed runner tests, not independent differential
validation or a whole-core conformance claim.
