# Generic SoC and FPGA smoke ROM

`examples/generic_soc/arm7tdmi_generic_soc.sv` is a synthesizable,
framework-neutral example around the canonical `arm7tdmi_mister` API. It
contains no vendor primitive, generated clock, private hierarchy reference,
runtime firmware loader, or PocketStation-specific behavior. The same system
is the payload of the checked MiSTer framework core.

## Memory map

| Range | Device | Behavior |
|---|---|---|
| `0x0000_0000`–`0x0000_03ff` | ROM | Read-only, asynchronous 32-bit words; writes complete with `MEM_ERROR` |
| `0x1000_0000`–`0x1000_03ff` | RAM | 1 KiB asynchronous-read RAM with byte-enable writes |
| `0x2000_0000` | Timer count | Read-only 32-bit master-clock count |
| `0x2000_0004` | Timer compare | Word-readable/writable comparison value |
| `0x2000_0008` | Timer control | Bit 0 enables counting; bit 1 enables IRQ |
| `0x2000_000c` | Timer status | Bit 0 is pending; writing one clears pending and disables the timer |
| `0x2000_1000` | UART transmit | Word write enters the one-byte transmit holding register |
| `0x2000_1004` | UART status | Bit 0 is `UART_TX_READY`; bit 1 is `UART_TX_VALID` |
| `0x2000_2000` | Test status | Word-readable/writable `RUN!`, `PASS`, or `FAIL` code |
| `0x2000_2004` | Test signature | Word-readable/writable progress, failure, or final signature |

Unmapped requests complete with `MEM_ERROR`; they cannot deadlock the CPU.
Timer, UART, and test registers require word writes. A failed write has no
side effect. `TEST_STATUS` and `TEST_SIGNATURE` expose the two test registers
as ordinary module outputs, so a containing core never needs a hierarchical
probe.

`UART_TX_VALID` and `UART_TX_DATA` remain stable until a rising `CLK` edge
with `UART_TX_READY`. All UART signals and `CPU_CE` are synchronous to
`CLK`. The timer runs from `CLK`, independently of CPU stalls, and its
active-high level is connected to the wrapper-owned IRQ synchronizer.

## ROM test software

[`program.S`](../examples/generic_soc/program.S) is a freestanding
repository-authored ARMv4T program with ARM exception vectors and a Thumb-1
subroutine. It runs these board-oriented smoke groups:

| Signature while `RUN!` | Test group |
|---:|---|
| 1 | Word, byte, halfword, and signed-byte memory lanes |
| 2 | Arithmetic flags, condition execution, and barrel shifts |
| 3 | `MUL` and `UMULL` result paths |
| 4 | `STM`/`LDM`, stack integrity, `BL`, and ARM return |
| 5 | Locked `SWP` read/write behavior |
| 6 | ARM-to-Thumb-to-ARM `BX` interworking |
| 7 | Timer programming, synchronized IRQ entry, banked LR, and exception return |

The terminal protocol is:

| Outcome | Test status | Test signature | UART |
|---|---|---|---|
| Pass | `0x5041_5353` (`PASS`) | `0xa7d1_c0de` | `GIP` |
| Test failure | `0x4641_494c` (`FAIL`) | Group number 1–7 | `F` |
| Unexpected exception | `0x4641_494c` (`FAIL`) | `0x0000_00e1`–`0x0000_00e6` for Undefined, SWI, prefetch abort, data abort, reserved vector, or FIQ | `F` |

`G` means all non-interrupt groups passed, `I` is emitted by the IRQ handler,
and `P` follows publication of the permanent pass state. This is a concise
physical-bring-up smoke test, not a replacement for the repository's full
instruction, exception, bus, debug, formal, randomized, and differential
regressions.

## Source-to-ROM integrity

[`linker.ld`](../examples/generic_soc/linker.ld) locates the program at the
ARM reset vector and rejects an image larger than the 1 KiB ROM.
The Rust `generic-soc-rom-check` command compiles it with the checksum-installed Arm GNU
14.3.Rel1 toolchain using `-march=armv4t`, checks ARMv4T/Thumb-1 ELF
attributes and required symbols/instructions, and compares every little-endian
word against the synthesizable `rom_word` function. It also checks all unused
ROM words are zero. The frozen image is 812 bytes (203 words), SHA-256:

`ee9c06bc1720ce9e4f9f57da3e162acdfc0e42df403766f6d98bbeb9249c4900`

Run the fail-hard equivalence gate, or retain inspection artifacts:

```sh
make -C scripts generic-soc-rom-check
make -C scripts generic-soc-rom-artifacts
```

The second command writes ignored `program.elf`, `program.bin`, link map,
disassembly, and ELF attributes under
`reports/generated/generic-soc-rom/`. Synthesis uses the checked
SystemVerilog words, so it does not depend on a working directory, external
ROM file, or synthesis-time assembler.

## Simulation and frontend checks

The fail-hard testbench randomizes `CPU_CE`, deliberately backpressures all
three UART bytes, observes all seven progress values and the timer interrupt,
rejects `FAIL` immediately, and requires the final status/signature:

```sh
make -C scripts lint-generic-soc
make -C scripts lint-generic-soc-slang
make -C scripts generic-soc-rom-check
make -C scripts sim-generic-soc
```

The first compile uses Verilator 5.x. The independent frontend compile uses
Slang 11.0. CI downloads the official `v11.0` Linux archive and verifies
SHA-256
`951a170e10e25e54c91565030acfdfc11c3226714ebf225a18ad4166a898d8a4`
before use. All four commands are mandatory regression phases.

## MiSTer use

From a clean committed tree, build the official-template core:

```sh
make -C scripts mister-framework
```

The Rust `mister-framework` command checks the pinned framework commit and
tree, installs the local RTL overlay, runs Quartus, and validates the fit,
timing, warnings, and artifact hashes before publishing its report.

The loadable image is
`reports/generated/mister-framework/Template.rbf`. The MiSTer display is blue
while the ROM runs, permanently green on pass, and permanently red on
failure. The first 512 pixels of the top 32 video lines form 32 cells that
show `TEST_SIGNATURE` most-significant bit first: white is one and dark is
zero. Thus a pass photograph contains a green field and the barcode for
`a7d1c0de`; a red photograph identifies the failing group or exception code.
`LED_USER` is solid for pass, blinks rapidly for failure, and blinks slowly
while running. The OSD `Reset` entry or the framework reset button reruns the
ROM.

The checked Quartus build proves the exact core fits and meets its framework
timing contract. Loading the RBF and capturing a real display/board result is
still the separate FPGA-007 hardware-evidence task; this document does not
claim that a physical MiSTer was available during repository validation.
