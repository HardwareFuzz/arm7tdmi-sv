# arm7tdmi-sv

A from-scratch, synthesizable SystemVerilog reimplementation of the
**ARM7TDMI-S r4p3** processor. The implementation target is ARMv4T behavior
and the pin, cycle, debug, and coprocessor contracts in the included
ARM DDI 0234B technical reference manual.

The project targets eventual use in MiSTer and other FPGA systems. Cycle
conformance is a release requirement, not an unconditional claim about the
current tree.

## Status

**Not sign-off ready.** The canonical, checkbox-level status and immutable
release gate are in
[`TASKS.md` §31](TASKS.md#31-release-readiness-audit-and-immutable-sign-off-contract).
The historical table in §31.1 describes the old audited baseline and must not
be read as the current state.

The status words used here have the §31 meanings:

- **VERIFIED**: the named subsystem requirement has fail-hard evidence linked
  from its checked §31 item. It is not a whole-core release claim.
- **IMPLEMENTED-UNVERIFIED**: RTL exists, but the required evidence is absent.
- **PARTIAL**: some requirements or required evidence remain open.

| Area | Current audited status |
|---|---|
| ARM/Thumb architecture | **VERIFIED** for the directed requirements in §31.3, including all Thumb format families, reserved space, PC/PSR rules, operand policies, dependencies, and state changes. Independent QEMU differential, constrained-random, pinned public ARM/Thumb suite, compiler-program, sanitizer/X-state soak, exact Chapter 7 cycle crosses, exhaustive 234-bin ARMv4T functional coverage, and §31.10 formal closure at 14 proofs plus 77 covers all pass. |
| Exceptions and aborts | **VERIFIED** for §31.4: priority, ARM/Thumb link values, DABT+FIQ, reset/interrupt sampling, Table 7-16 entry/return buses, and per-beat LDM/STM/SWP abort behavior have fail-hard matrices. |
| Raw bus and cycle behavior | **VERIFIED** for §31.5. Endian lanes, CLKEN/ABORT behavior, LOCK/DMORE, redirects, exception cycles, every Chapter 7 family, and reusable protocol assertions are linked to full-phase or specialized scoreboards. |
| External coprocessor and CP14 | **VERIFIED** for §31.6: absent/ready/busy/abandonment, MCR/MRC/CDP/LDC/STC timing, CP14 DCC and Debug Abort Status, corrected errata 14/15 policy, and absence of internal CP15 are tested. |
| EmbeddedICE-RT, JTAG, and trace boundary | **VERIFIED** for §31.7/§31.8, including debugger-issued system-speed aborts, the §5.25 Debug Status `RO/WI` conflict disposition, the project-specific same-clock virtual-TCK transport, and the ARM-side ETM7 boundary. Asynchronous pod/GDB execution and an ETM macrocell are outside the selected CPU-boundary profile and are not claimed. |
| FPGA/MiSTer package | **PARTIAL**. The canonical valid/ready wrapper, versioned save states, two public bus adapters, option profiles, synchronized reset/CDC contract, structural CDC/RDC audit, independent all-top Slang lint, DMA arbitration, portable QIP/file lists, two checked standalone Quartus 17.0.2 flows, and a pinned, fully constrained official MiSTer-framework bitstream build exist. PocketStation integration and physical hardware evidence remain open. |
| Release evidence | **PARTIAL**. Regressions fail hard; an independent 77-retirement QEMU differential, 32 × 256 constrained-random campaign, 32-seed all-class external-event campaign, pinned public suite and GCC ARM/Thumb/interworking execution, a 256-seed sanitizer/X-state soak, exact Chapter 7 legal-equivalence cross coverage, exhaustive encoding/policy required-bin closure, an exhaustive whole-TRM inventory, complete Chapter 8 AC timing disposition, 14 formal proofs, 77 formal covers, bidirectional requirement/source traceability, a real-framework fit/STA/RBF report, machine-readable reports, mutation tests, and a hashed evidence archive exist. Independent review and board/system release gates remain open. |

No unchecked §31 requirement is implied by a checked neighboring row. In
particular, this repository is not yet advertised as a drop-in, issue-free
MiSTer CPU.

## Public integration choices

Most FPGA systems should instantiate
[`arm7tdmi_mister`](rtl/top/arm7tdmi_mister.sv). It exposes one synchronous
valid/ready memory request, buffers a response received while CPU clock-enable
is low, synchronizes asynchronous event inputs, and can compile out external
debug and coprocessor inputs. Its version-1 contract, timing diagrams,
parameters, reset behavior, endian lanes, error rules, DMA ownership, and
Wishbone/enable-done adapters are in
[`docs/INTEGRATION.md`](docs/INTEGRATION.md).
The opt-in architectural save/restore schema and containing-system snapshot
rules are in [`docs/SAVESTATE.md`](docs/SAVESTATE.md).

Systems that specifically need the ARM7TDMI-S-style address/data pipeline can
instantiate [`arm7tdmis_top`](rtl/top/arm7tdmis_top.sv). Its N/S/I/C phase,
CLKEN, ABORT, LOCK, DMORE, endian, and protection rules are in
[`docs/RAW_BUS.md`](docs/RAW_BUS.md).

The portable FPGA source package is:

- [`fpga/arm7tdmi_mister.f`](fpga/arm7tdmi_mister.f), for tools that accept a
  plain relative file list;
- [`fpga/arm7tdmi_mister.qip`](fpga/arm7tdmi_mister.qip), for Quartus
  projects;
- [`fpga/example/arm7tdmi_mister_example_top.sv`](fpga/example/arm7tdmi_mister_example_top.sv),
  a hierarchy-free wrapper example; and
- checked standalone and raw-conformance characterization projects under
  [`fpga/`](fpga/).

The checked example under
[`examples/mister_framework/`](examples/mister_framework/) builds this
package and a seven-group ARMv4T self-test ROM in a pinned official MiSTer
template. Blue/green/red video, a 32-bit signature barcode, and LED states
make board bring-up observable without JTAG. It is a CPU/framework build
qualification and physical-test payload, not a PocketStation subsystem or
claim that physical-board evidence has been captured.

## Implemented contracts

The detailed documents below are the maintained descriptions of behavior and
evidence. The §31 ledger remains authoritative if a summary conflicts with it.

- [`docs/PIPELINE.md`](docs/PIPELINE.md): F/D/E pipeline, PC values,
  multicycle states, exact tested cycle shapes, flushes, and store/load
  overlap.
- [`docs/PSR.md`](docs/PSR.md): CPSR/SPSR flags, privilege, physical banks,
  invalid-mode policy, and exception restore.
- [`docs/EXCEPTIONS.md`](docs/EXCEPTIONS.md): all seven exceptions, priority
  and link values, per-beat abort completion, and return forms.
- [`docs/COPROCESSOR.md`](docs/COPROCESSOR.md): external claim/busy/data
  protocol, CP14 ownership, absent internal CP15, and errata policy.
- [`docs/DEBUG.md`](docs/DEBUG.md): EmbeddedICE-RT, scan chains, halt/monitor
  restrictions, public synchronous transport, and CP14 DCC.
- [`docs/TRACE.md`](docs/TRACE.md): execute-status semantics and the external
  ETM7 Table 6-1 adapter; no trace macrocell is included.
- [`docs/UNPREDICTABLE.md`](docs/UNPREDICTABLE.md): every deterministic
  project policy for architecturally UNPREDICTABLE or UNKNOWN inputs.
- [`docs/VERIFICATION.md`](docs/VERIFICATION.md): fail-hard regression,
  coverage-report, mutation, and evidence-archive contracts.
- [`docs/TRACEABILITY.md`](docs/TRACEABILITY.md): bidirectional per-requirement
  evidence and complete RTL/test ownership mapping.
- [`docs/TRM_COVERAGE.md`](docs/TRM_COVERAGE.md): revision-locked inventory
  and disposition of every r4p3 section, table, figure, and signal.
- [`docs/WARNING_POLICY.md`](docs/WARNING_POLICY.md): the small audited set of
  lint exceptions and their expiry conditions.
- [`docs/ERRATA.md`](docs/ERRATA.md): all 15 published r4p3 errata and their
  corrected-default evidence.
- [`docs/SUPPORT.md`](docs/SUPPORT.md): supported API, simulator, FPGA-tool,
  framework, and system-integration profiles.
- [`docs/PERFORMANCE.md`](docs/PERFORMANCE.md): checked Cyclone V clock,
  CPU-enable, transaction-latency, resource, Fmax, and power data.
- [`docs/GENERIC_SOC.md`](docs/GENERIC_SOC.md): portable
  ROM/RAM/timer/UART/status integration, executable FPGA smoke program, and
  MiSTer pass/fail display protocol.
- [`docs/SAVESTATE.md`](docs/SAVESTATE.md): versioned architectural state
  handshake, exact physical-bank map, and deterministic restore contract.
- [`docs/LIMITATIONS.md`](docs/LIMITATIONS.md): concise prerelease blockers
  linked to the authoritative task IDs.
- [`docs/PROVENANCE.md`](docs/PROVENANCE.md): license, specification,
  third-party, test-program, firmware, and ROM handling.
- [`SECURITY.md`](SECURITY.md): debug exposure and containing-system security
  responsibilities.
- [`CHANGELOG.md`](CHANGELOG.md) and [`VERSION`](VERSION): semantic project
  version and user-visible release history.

ARMv5+ features are intentionally absent: no `BKPT`, `BLX`, `CLZ`, Q flag,
`MAS[1:0]`, `DBGRESTART`, or separate `DBGINSTR`. ARM7TDMI-S uses `SIZE[1:0]`
and `DBGINSTRVALID`; software breakpoints use EmbeddedICE-RT comparison.

## Build and verification

To execute your own ARMv4T image and export exceptions, registers, and RAM,
use the HardwareFuzz fork's [standalone runner](docs/STANDALONE.md):

```sh
make -C scripts standalone-build STANDALONE_JOBS=8
```

It runs the supplied program directly on the RTL without QEMU or expected
state files. The linked guide includes a complete example and the JSONL
result contract.

The supported simulator/linter is Verilator 5.x. The checked FPGA
characterization uses Quartus Lite 17.0.2. Run commands from the repository
root:

```sh
make -C scripts lint
make -C scripts lint-tb
make -C scripts unit
make -C scripts integ
make -C scripts run

make -C scripts lint-example
make -C scripts lint-generic-soc
make -C scripts lint-generic-soc-slang
make -C scripts generic-soc-rom-check
make -C scripts sim-generic-soc
make -C scripts quartus-analysis
make -C scripts quartus-compile
make -C scripts quartus-conformance-analysis
make -C scripts quartus-conformance-compile
make -C scripts fpga-quality

make -C scripts regress
make -C scripts soak
make -C scripts coverage
make -C scripts release-evidence
```

[`scripts/Makefile`](scripts/Makefile) is the single live manifest for every
registered unit, integration, errata, coverage, and FPGA phase. The README
does not duplicate those lists. `regress` starts clean and writes a
machine-readable result plus individual hashed logs under
`reports/generated/`; `coverage` writes raw databases, LCOV, and a
machine-readable summary; `release-evidence` verifies their commit and
source identity before creating a checksummed archive. Generated evidence is
intentionally ignored by Git and uploaded as a CI/release artifact.

A successful directed regression is necessary but not sufficient for v1.0.
PocketStation integration/soak, clean-checkout reproducibility, physical
hardware, and independent-review items in §31 remain release blockers.

## Repository layout

```text
rtl/
  core/ decode/ datapath/ debug/ jtag/ trace/ top/
  arm7tdmis_*_pkg.sv
tb/
  unit/ integration/ harness/ programs/
verification/
  reusable protocol checkers
scripts/
  Makefile, ordered source lists, regression/coverage/release tools
fpga/
  portable package, constraints, characterization projects, example
examples/generic_soc/
  synthesizable ROM/RAM/timer/UART/status system and ARMv4T smoke ROM
docs/
  maintained architecture, verification, and integration contracts
```

## License and specification

Repository software is distributed under [`LICENSE`](LICENSE). The included
`ARM_DDI_0234B_ARM7TDMI-S_r4p3_TRM.pdf` remains under its original Arm
copyright and is a specification reference, not project-licensed source.
The complete release inventory and SPDX conclusions are in
[`docs/PROVENANCE.md`](docs/PROVENANCE.md).
