# Verification and regression evidence

`make -C scripts regress` is the release-facing directed regression entry point.
It always removes the Verilator build directory first, then runs:

1. Raw-core RTL, canonical MiSTer wrapper, portable FPGA example, and
   testbench lint, plus independent all-public-top Slang lint and structural
   CDC/RDC analysis.
2. Quartus analysis and complete compile/report checks for both FPGA profiles.
3. Harness unit tests, the intentional-failure sentinel, and raw-bus-checker
   mutation matrix.
4. Every unit test in the `UNIT_TESTS` manifest.
5. Every directed test in the `INTEG_TESTS` manifest.
6. The pinned public ARM/Thumb suite and the 32 × 256 constrained-random
   campaign.
7. The 256-seed sanitizer/X-state soak.
8. The revision-locked complete ARM DDI 0234B inventory gate.
9. The mixed-instruction smoke test and bidirectional traceability gate.

## Formal proof and reachability evidence

`make -C scripts formal` checksum-installs the Linux x86-64 OSS CAD Suite
2026-07-29 release and runs the reviewed matrix in
`verification/formal_requirements.json`. The archive SHA-256 is
`89ea1152ea84bc600f18cc685f721d534d1f018e09831662787865a3d79ce4aa`;
the accepted tools are SBY v0.67, Yosys 0.67, and Boolector 3.2.4.

The matrix contains 14 named proof obligations:

- architectural writes, conditional suppression, register-bank isolation,
  PC alignment, and CPSR flag preservation;
- CLKEN bus stability, SWP LOCK lifetime, exception priority, and abort
  writeback suppression;
- single-outstanding MiSTer request/response ownership, bounded bridge
  completion, TAP transitions, and reset/debug synchronizer behavior; and
- whole-core return to Execute under bounded interface fairness.

The core safety, alignment, and liveness tasks use ABC PDR. Stall, bridge,
register-file, PSR, TAP, and CDC/reset tasks use Boolector bounded proof plus
k-induction. The MiSTer harness proves the production bridge against an
arbitrary raw-core output model; it does not assume that the present CPU keeps
uncaptured raw phases stable. Its fairness model requires `CPU_CE` and an
advertised memory request to be serviced within four clocks. The core
liveness model similarly bounds CLKEN and ready/busy coprocessor responses
while forbidding the architecturally illegal `{CPA,CPB}=2'b10` response.

Reachability is a separate fail-closed result, not inferred from a task's exit
code. The 80-step core task must name all 17 implemented FSM states, all 40
implemented transitions, and 13 exception entry/return points in its SBY
`PASS` summary. The 32-step debug task must name seven debug
entry/restart/system-speed points. Prefetch- and data-abort returns are
distinguished by formal-only origin tracking even though both execute in
Abort mode. The aggregate is therefore 77 exact cover obligations.

`scripts/formal_report.py` writes `reports/generated/formal-report.json` using
schema `arm7tdmis-formal-v1`. It records the clean commit state, pinned tool
identities, manifest/SBY hashes, and a source and task-log hash for every
obligation. A report passes only with all 14 proofs, all 77 covers, and an
empty uncovered list. Formal is a mandatory non-quick regression phase,
runs in scheduled/manual CI, and is archived by release evidence.
`scripts/release_evidence.py` revalidates the report rather than trusting its
status, rejecting dirty or stale runs, tool drift, missing obligations,
uncovered points, or hash changes.

The runner stops at the first failing phase and returns nonzero. The intentional
failure sentinel contains a real SystemVerilog `$fatal`; the enclosing target
passes only when Verilator propagates that failure as a nonzero process status.
This catches accidental loss of simulator failures in shell or result handling.

`make -C scripts mutation-self-test` is the fail-closed architectural mutation
gate. Compile-time hooks that are absent from normal builds suppress GPR
writeback, suppress CPSR flag writes, suppress exception PSR entry, or invert
the CPnMREQ bus contract. The sequence-dependency, flags, six-exception LR/PSR,
and raw-bus tests must respectively kill those mutants. A nonzero compile or
simulation exit is insufficient: each row must contain its expected
architectural diagnostic, so a syntax error cannot masquerade as mutation
coverage. `scripts/tests/test_mutation_harness.py` also proves that a survivor
and a wrong detector fail orchestration.

`.github/workflows/verification.yml` runs the fail-hard open-source regression,
the raw-bus mutation matrix, representative architectural simulations, and
measured RTL coverage for pushes and pull requests. It publishes a
commit-addressed evidence artifact only after the reports pass clean-tree and
same-commit validation. Its Monday schedule and manual-dispatch path also run
the four-class architectural mutation suite.

## Machine-readable result

Each run atomically updates `reports/generated/regression.json` using schema
`arm7tdmis-regression-v1`. It records:

- the full Git commit, dirty status, and SHA-256 of local source/build inputs;
- Verilator, Python, GNU Make, Git, and host-platform versions;
- build variant and deterministic seed;
- the complete lint, harness, unit, integration, and smoke manifests;
- each phase's command, timestamps, duration, exit status, log path, and
  SHA-256; and
- the aggregate `running`, `passed`, `failed`, or `interrupted` status.

Per-phase logs live beside the report under `reports/generated/logs/`. Generated
reports are ignored because an ordinary developer run is not immutable release
evidence. A release workflow must copy the selected report and logs to its
versioned artifact archive together with the corresponding clean source commit.
The final `traceability` phase writes
`reports/generated/traceability-report.json`; its bidirectional completeness
and result semantics are defined in [TRACEABILITY.md](TRACEABILITY.md).
The preceding `trm-coverage` phase writes
`reports/generated/trm-coverage-report.json`; it locks all 207 numbered
sections/subsections, 65 tables, 44 figures/timing diagrams, and 45 Appendix A
signals to an explicit §31 disposition as defined in
[TRM_COVERAGE.md](TRM_COVERAGE.md).
The non-quick `soak` phase separately writes
`reports/generated/soak-report.json` using schema `arm7tdmis-soak-v1`.
The mandatory `public-suite` phase writes
`reports/generated/public-suite-report.json` using schema
`arm7tdmis-public-suite-v1`.

## Chapter 7 cross coverage

`make -C scripts regress` runs the full Chapter 7 aggregate only after every
directed integration phase has passed. `verification/table7_cross.json`
defines nine required legal-equivalence crosses over instruction family,
register/PC and state, multiply `m`, block/coprocessor `n`, coprocessor `b`,
condition/mode, endian/alignment, deterministic CLKEN holds, abort position,
and interrupt/exception timing. It excludes only dimensions that do not apply
to an instruction class; exclusions do not satisfy a required bin.

`verification/table7_cross.py` rejects quick, failed, dirty, stale, duplicate,
missing-log, hash-mismatched, under-counted, or missing-marker evidence. It
resolves each manifest entry to a passing `integration-*` result in the live
full regression, independently hashes its log and owning testbench, and
requires the count-bearing PASS expression. The report schema
`arm7tdmis-table7-cross-v1` records 1,903 evidence rows, a 1,891-row minimum,
all nine required crosses, and an empty missing-cross list in
`reports/generated/table7-cross-report.json`. Release packaging repeats the
structural checks, hashes the referenced sources/logs, and reconnects each
entry to the same regression phase. Quick CI deliberately omits this aggregate
because it does not run all owning matrices.

## Independent lint and CDC/RDC closure

`make -C scripts fpga-quality` is the FPGA-004 open-source quality gate. The
first half runs checksum-installed Slang 11.0.0+7ddf4059f as an independent
SystemVerilog elaborator over five public synthesis tops: `arm7tdmis_top`,
`arm7tdmi_mister`, `arm7tdmis_no_dft`,
`arm7tdmi_mister_example_top`, and `arm7tdmi_generic_soc`. Every compile must
return zero errors and zero warnings. Schema
`arm7tdmis-independent-lint-v1` records the exact executable and source
hashes, command, diagnostic JSON hash, and result for every top.

The second half runs `scripts/cdc_rdc_check.py` against the reviewed
`verification/cdc_rdc_manifest.json`. The structural audit inventories every
production `always_ff`, rejects any clock except `CLK`, rejects unapproved or
multiple asynchronous resets, forbids legacy `always @` and `always_latch`,
verifies both reset-release synchronizer instances, and proves that each of
the six `_ASYNC` event inputs feeds exactly one marked first stage whose only
consumer is its marked second stage. Its mutation unit test injects an extra
clock, a combinationally generated reset, and first-stage fanout and requires
all three to fail.

The resulting `arm7tdmis-cdc-rdc-v1` report currently accounts for 37
sequential source blocks, 15 asynchronous-reset blocks, one clock domain,
six event synchronizers, two reset-release instances, and zero violations.
The audit exposed direct asynchronous deassertion of wrapper-owned state;
`arm7tdmi_mister` now uses a parallel two-stage `wrapper_reset_n` path so the
memory bridge, event synchronizers, debug transport, TAP, and raw
architectural domain release on a clock edge together. Both quality phases
run in quick and full regression. Release archiving rejects missing, dirty,
stale, nonzero-warning, structurally incomplete, or input-hash-mismatched
reports.

`make -C scripts regress-quick` exercises the same metadata, clean-build, lint,
harness, and smoke path with one unit plus the mandatory QEMU differential and
pinned-compiler integration tests. Its report is explicitly marked
`"mode": "quick"` and cannot be mistaken for the full run.

`make -C scripts regress-ci` is the open-source CI form. It also uses quick
selection, sets `"scope": "simulation-only"`, records an empty FPGA phase
manifest, and never probes for or invokes Quartus. It is durable evidence for
the phases it lists, but it is not a substitute for a clean full
`"scope": "release"` report containing both FPGA profiles.

## Coverage and immutable artifact

`make -C scripts coverage` builds four named integration tests with Verilator
line, branch, expression, toggle, and user coverage instrumentation:
`retire_interface`, `sequence_dependencies`, `arm_exception_lr`, and
`debug_system_speed`. It preserves each raw database, merges them into
`reports/generated/coverage/coverage.dat`, writes standard LCOV to
`coverage.info`, and atomically publishes schema `arm7tdmis-coverage-v1` as
`coverage.json`.

The JSON identifies the exact tests, Git commit, dirty state, source-input
SHA-256, Verilator version, raw/LCOV hashes, per-point totals for all elaborated
sources and RTL-only sources, and per-file LCOV line/branch totals. The report
states its limit explicitly: these four tests are measured structural coverage,
not the zero-uncovered-bin functional closure required by VAL-006.

After regression and coverage have both run on the same clean commit,
`make -C scripts release-evidence` validates their schemas, passing status,
clean-tree markers, and exact commit identity. It emits:

- `reports/generated/release-manifest.json`, with size and SHA-256 for every
  included regression, TRM-inventory, traceability, coverage, and (when the
  mandatory full-regression phase runs) MiSTer-framework report, every
  map/fit/STA/assembly artifact and RBF, phase log, and coverage database,
  plus the semantic
  project version, Git tree hash, regression source hash, canonical tool
  version hash, available tool-executable hashes, and specification/license
  hashes;
- `arm7tdmis-release-evidence-<commit>.tar.gz`; and
- a sibling `.sha256` checksum.

The GitHub artifact contains both the inspectable files and this self-contained
archive. Generated local reports remain ignored; immutable retention belongs
to the commit-addressed CI/release artifact store.

## Architectural retirement contract

Define `ARM7TDMIS_VERIFICATION` when elaborating `arm7tdmis_top` to add the
`VER_RETIRE_*` ports. The production and FPGA file lists do not define this
symbol, so the interface and its identity latches are absent from synthesis.
The `integ-retire_interface` target is the canonical opt-in example.

`VER_RETIRE_VALID` is a one-clock pulse registered on the enabled edge that
applies an instruction's final architectural effect. A condition-failed
instruction is still one disposition. A synchronous trap or abort is one
disposition with `VER_RETIRE_EXCEPTION_VALID`; a busy coprocessor instruction
abandoned before completion does not retire. An asynchronous exception that
has no paired instruction may assert only `VER_RETIRE_EXCEPTION_VALID`.
Consumers sample the identity and snapshots after the event edge:

- `VER_RETIRE_PC`, `VER_RETIRE_OPCODE`, `VER_RETIRE_THUMB`, and
  `VER_RETIRE_CONDITION_PASS` identify the instruction. Thumb opcodes are
  normalized into bits 15:0 independent of address lane and endianness.
- `VER_RETIRE_INJECTED` distinguishes scan-chain debug-speed instructions.
- `VER_RETIRE_EXCEPTION_VALID` and `VER_RETIRE_EXCEPTION` use the
  `exception_e` values in `arm7tdmis_types_pkg`.
- `VER_RETIRE_GPRS` is the post-event 31-slot physical regfile map documented
  in `arm7tdmis_regfile.sv`; slot 15 is the intentional PC layout hole.
  `VER_RETIRE_CPSR` and `VER_RETIRE_SPSRS` are post-event storage snapshots.
  SPSR slices 0 through 4 are FIQ, IRQ, Supervisor, Abort, and Undefined.

`arm7tdmis_retire_interface_tb` consumes only these CPU outputs. Its eleven
exact events cross ARM and Thumb, a false condition, STR and LDR completion,
BX interworking, normalized halfwords, and SWI entry. It checks the loaded
destination, SVC link bank, CPSR, and saved pre-exception SPSR from the public
snapshots. Memory initialization remains testbench hierarchy; architectural
CPU observation does not.

## Independent QEMU differential

For direct execution of an externally supplied image, the
[standalone runner](STANDALONE.md) exports VER-009 exception events and final
state without invoking this differential or consuming its expected files.

`make -C scripts integ-qemu_diff` builds
`verification/programs/qemu_diff.S` for `-march=armv4t`, executes the linked
image with `qemu-system-arm` on the ARM926 model using one guest instruction
per translation block, then runs the same image on this RTL. QEMU is an
external implementation: `qemu_armv4t_reference.py` imports no RTL package,
decoder, or project expected-value function.

The measured region has 77 consecutive retirements. It crosses ARM and Thumb,
passed and failed conditions, immediate and register shifts, multiply,
word/byte/halfword/signed loads and stores, LDM/STM, SWP, ARM BL, Thumb BL,
and both directions of interworking. For every instruction the RTL
scoreboard compares the instruction PC/state, post-state r0-r14, and the
architectural CPSR fields. QEMU represents the ARMv4T two-halfword Thumb BL
pair as one translated instruction; the generator records this normalization
and splits that one log event into the architectural prefix LR write and
suffix branch without replacing QEMU's final state. The scoreboard also
compares five final memory words to the values independently loaded by QEMU.

The comparison deliberately uses only behavior shared by ARM7TDMI and QEMU's
ARM926: no ARMv5 encoding, exception/platform behavior, unaligned access, or
architecturally UNPREDICTABLE input is generated. QEMU's later A bit and
other non-ARM7 PSR extensions are masked; NZCV, T, I, F, and mode are not.
Every run writes `metadata.json` beside `program.hex` and `expected.hex` with
the exact Clang/QEMU identities, symbols, commands, normalization, event
count, and SHA-256 hashes. Generated files live under
`reports/generated/qemu_diff/` and are included in release-evidence archives.
The test is first in `INTEG_TESTS`, so quick open-source CI and full local
regression both regenerate rather than trusting a committed golden trace.

## Constrained-random differential campaign

`make -C scripts random-validation` builds two hierarchy-independent
scoreboards once, then runs 32 deterministic seeds with 256 generated random
instructions per seed. The shared-ARMv4T lane starts at the architectural
vector table and uses QEMU ARM926 with one instruction per translation block.
It covers ARM and Thumb instructions, register and flag choices,
producer/consumer dependencies, aligned mixed-width memory operations, every
privileged register-bank mode, and ARM/Thumb SVC and undefined entry/return.
`arm7tdmis_random_diff_tb.sv` compares each retirement's instruction PC/state,
post-state active r0-r14 bank, architectural CPSR fields, and synchronous
exception cause, then compares five directly observed memory words to values
independently loaded by QEMU. A release run must contain at least 8,192 such
retirements.

The second lane isolates behavior on which ARM926 is not an ARM7 oracle.
For every seed, `constrained_random.py` generates separate little- and
big-endian programs with all four legacy word-load alignments, byte lanes,
aligned halfwords, signed loads, stores, and dependent mutations. Its compact
architecture-derived memory model predicts only those permitted memory
effects. `arm7tdmis_random_policy_tb.sv` compares every generated result plus
the final source array and never reads CPU hierarchy.

Schema `arm7tdmis-constrained-random-v1` records the clean commit, exact
Clang/QEMU and simulator identities, generator/testbench hashes, all required
coverage bins, per-seed event totals, source/ELF/binary/trace/expected/log
hashes, and an exact single-seed reproducer. Full regression runs the 32 × 256
campaign; quick CI runs `random-validation-quick` with two 64-instruction
seeds. Release evidence independently revalidates the report strength, every
input, and every generated artifact hash.

## Constrained-random external-event campaign

`make -C scripts random-events` builds
`arm7tdmis_random_event_matrix_tb.sv` once and runs 32 deterministic,
nonrepeating seeds. Every seed independently decodes all 16 normalized
instruction classes and freezes each at an LFSR-selected two-to-five-cycle
CLKEN hold. The scoreboard checks architectural pipeline state and every
Chapter 7/debug/coprocessor output through the hold and first-cycle output
settling rule.

The same seed uses rejection sampling to place ABORT only on live opcode,
data-read, and data-write responses; delays IRQ, FIQ, and DBGRQ to random
cycles; selects the instruction class and duration of an in-flight reset; and
varies a legal coprocessor busy interval before completion. Separate ready
and absent cases complete the `{CPA,CPB}` coverage; `2'b10` is never
generated. Each event must reach its expected abort/interrupt mode, reset
state, debug acknowledge, completion, or Undefined path.

Schema `arm7tdmis-random-events-v1` records exact runner/testbench/binary
hashes, 16 class bins, 16 matching class-stall bins, ten event bins, at least
256 per-cycle random decisions per seed, logs, and exact reproducers. The
full regression and release archive require a clean same-commit 32-seed
report. `test_random_event_contract.py` mutation-tests the masks, seed
strength, decision floor, status, and provenance.

## ARMv4T required-bin functional coverage

`make -C scripts functional-coverage` is a full-regression aggregate rather
than simulator line coverage. `functional_coverage.py` independently
enumerates all 4,096 ARM `[27:20] × [7:4]` decode rows, all 4,096
`cond=1111` policy rows, and every one of the 65,536 original Thumb
halfwords. The complete-domain RTL comparisons in `reserved_decode_tb.sv`
and `unpredictable_decode_tb.sv` must have passed in the same regression:
they freeze all 16 ARM allocation totals, 60,416 non-reserved versus 5,120
reserved Thumb words, 1,066 ARM static policy traps, 448 Thumb static policy
traps, and 12 deliberately defined policy rows.

`verification/functional_coverage.json` expands to 234 required bins. It
names every ARM condition, data-processing opcode, shifter form, multiply,
PSR transfer, branch, addressing/transfer, swap, and coprocessor family; all
19 Thumb formats, 16 format-4 ALU operations, 14 conditions, and format
subfamilies; plus reserved and selected UNPREDICTABLE-policy execution paths.
Conditions, register-list semantics, and exceptional policies close only
when their 13 directed execution phases also pass. Thus exhaustive static
allocation cannot substitute for execution behavior, and a passing directed
test cannot hide an omitted encoding family.

Schema `arm7tdmis-functional-coverage-v1` records all domain totals, required
and covered bin lists, zero uncovered bins, cited exclusions, exact evidence
markers, and SHA-256 values for every owning log/source. Full regression and
release evidence require a clean same-commit report.
`test_functional_coverage_contract.py` independently mutates domains, groups,
bins, evidence, exclusions, status, and provenance.

## Public ARMv4T suite

`make -C scripts public-suite` fetches the MIT-licensed
`jsmolka/gba-suite` repository at commit
`a7113b67e63f83a9b321696ddd7042ccfad6c881`. The runner refuses a checkout
unless the origin, commit, tree, clean state, license SHA-256, and both source
ROM SHA-256 values exactly match `verification/public_suites.json`.

The generated execution images remove bytes 0x04–0xbf from the non-executable
GBA cartridge header and replace only the manifest-enumerated words that
enter ARM3-only 26-bit compatibility behavior or ARMv4T UNPREDICTABLE classes
for which this project already has a different documented deterministic
policy. The runner checks every other payload byte against upstream and
checks the complete patched-image hashes. This is an explicit normalization
boundary, not evidence for the skipped policy cases; their repository-owned
directed matrices remain authoritative.

`arm7tdmis_public_suite_tb` supplies only the reset trampoline, memory regions,
and deterministic DISPSTAT vblank source needed by the exercisers. It checks
the upstream zero-result register, visible-VRAM FNV signature, idle PC,
minimum trace length, instruction-state counts, and absence of exceptions or
debug injection through public CPU pins and `VER_RETIRE_*`; it does not read
internal hierarchy. The frozen passing measurements are:

| Image | Retirements | ARM | Thumb | VRAM signature | Idle PC |
|---|---:|---:|---:|---|---|
| ARM | 8,596 | 8,588 | 8 | `0xe8d71fb2` | `0x08001ec4` |
| Thumb | 6,691 | 6,152 | 539 | `0xe8d71fb2` | `0x08000aac` |

The atomically written report records source/license/patch provenance, input
and generated-artifact hashes, commands, metrics, and exact reproducers.
Both quick CI and full regression regenerate the 15,287-retirement result.
Release archiving validates the same clean commit, exact manifest, both
instruction-state floors, frozen signatures, and every artifact hash. The
proprietary Arm Validation Suite is neither used nor claimed.

## Pinned ARM/Thumb compiler program

`make -C scripts integ-compiler` installs Arm GNU Toolchain 14.3.Rel1 for
Linux x86-64 into the ignored `.tools/` cache. The installer downloads the
official `arm-none-eabi` archive and verifies SHA-256
`8f6903f8ceb084d9227b9ef991490413014d991874a1e34074443c2a72b14dbd`
before extraction. A cached install is reused only when its metadata,
compiler executable, release, digest, and reported GCC 14.3.1 identity agree.

The build uses `-march=armv4t -mthumb-interwork -ffreestanding`: startup and
one C translation unit use `-marm`, while a second C translation unit uses
`-mthumb`. ARM calls Thumb for a loop and mixed-width stores; Thumb calls back
to an ARM arithmetic function on every loop iteration. The build fails if the
linked image loses `Tag_CPU_arch: v4T`, lacks any expected function, or
contains ARMv5 `BLX`. It records the exact commands, GCC identity, source and
output hashes, disassembly, ELF attributes, link map, binary, and simulator
hex under `reports/generated/compiler/`.

`arm7tdmis_compiler_tb` loads that hex into the public raw-bus memory model.
It requires retirements in both instruction states and both ARM-to-Thumb and
Thumb-to-ARM transitions, rejects exceptions/debug injection, and accepts
completion only after the C-written mailbox proves loop/arithmetic, stack and
call behavior, plus compiler-generated word, halfword, and byte accesses.
This test is mandatory in quick CI and full regression. Its generated build
directory is included in release-evidence archives when the phase ran.

## Deterministic sanitizer soak

`make -C scripts soak` builds the canonical MiSTer-wrapper integration test
with Verilator `--x-assign unique --x-initial unique` and instruments the
generated C++ with AddressSanitizer and UndefinedBehaviorSanitizer. It then
runs 256 deterministic, nonzero 16-bit seeds. Each seed controls both
Verilator's unique X values and the testbench LFSR that independently stalls
CPU enable and memory readiness. The ARM workload repeats its mixed word,
byte, halfword, and load/store transaction sequence 64 times, so the test
also continuously checks request stability and completion while CPU enable
is low.

Every simulator child has a 30-second timeout, fail-fast `ASAN_OPTIONS` and
`UBSAN_OPTIONS`, and must exit zero with its exact seed-specific PASS marker.
The atomically written `arm7tdmis-soak-v1` report records the Git state,
simulator/host identity, sanitizer and X configuration, binary and testbench
SHA-256, and each seed's command, duration, exit status, output hash, and PASS
status. Any failure stops the run, saves its complete log, then greedily
clears seed bits only while the same timeout, sanitizer class, or normalized
testbench diagnostic remains reproducible. It emits a JSON reproducer with
that failure signature, the minimized seed, exact environment, command,
timeout, and binary hash.

The soak is mandatory in full (non-quick) regression. Scheduled and manually
dispatched CI execute it independently and use an always-run artifact upload
to retain `soak-report.json` plus any minimized failure reproducer. Release
evidence accepts a full-run soak only when all 256 seeds are unique and
passing, the report names the same clean commit, and all recorded input
hashes still match.

## Fitted-netlist architectural simulation

`make -C scripts postfit-sim` creates temporary Quartus Lite 17.0.2 projects
for `arm7tdmi_mister` on `5CSEBA6U23I7`, one with `BIG_ENDIAN=0` and one with
`BIG_ENDIAN=1`. Each project runs synthesis, fit, and the EDA netlist writer
with `--functional`; the generated Verilog is then compiled and executed as a
structural netlist by Verilator. The runner passes
`--write_settings_files=off`, so Quartus cannot append simulation assignments
to a checked production QSF.

The simulation-only synthesis profile sets `AUTO_DSP_RECOGNITION OFF`. This
keeps its functional netlist to five transparent primitive families:
`dffeas`, `cyclonev_lcell_comb`, the input/output buffers, and
`cyclonev_clkena`. The repository-authored clean-room zero-delay models in
`verification/intel_cyclonev_postfit_primitives.sv` implement only that
reviewed subset. The runner rejects any new primitive, `cyclonev_mac`,
encrypted model dependency, or `altera_mf` dependency. Production DSP
inference, timing, fit, and programming-image generation remain independently
required by FPGA-003/005; this special profile is functional post-fit
simulation, not a substitute for those production reports or an SDF timing
claim.

`arm7tdmi_postfit_tb.sv` observes only the fitted wrapper ports. For each
endian netlist it cancels a held request with asynchronous reset, accepts a
response while `CPU_CE` is low, then independently randomizes CPU enables and
memory readiness. It requires stable valid/ready payloads, multi-cycle
backpressure, reset-vector fetch, code and data transactions, correct
byte/halfword endian lanes, an architectural memory signature, IRQ vector
entry and handler signature, and word/byte/halfword writes.

Schema `arm7tdmis-postfit-v1` records both netlist hashes and primitive
inventories, all input hashes, exact executable identities, commands,
transaction metrics, and hashed map/fit/EDA/build/simulation logs in
`reports/generated/postfit-report.json`. Only the temporary profile's
intentional missing-board-pin and missing-SDC critical-warning IDs are
allowed; all other critical warnings fail. The two fitted netlists themselves
are temporary and are not redistributed. Full regression requires the phase,
and release evidence requires a passing clean same-commit report with both
endian profiles and every recorded hash intact.

## Exhaustive encoding evidence

`reserved_decode_tb` is deliberately exhaustive rather than sample-based. It
checks all 4,096 ARMv4T decode-bit combinations, every one of those
combinations under the implementation's ARMv4 `cond=1111` trap policy, and
all 65,536 original Thumb halfwords. Fixed allocation totals make an
accidentally weakened reference map fail hard. The independent
`arm7tdmis_reserved_execute_tb` then executes one representative from every
reserved family through the pin-level memory interface and checks exception
state and absence of memory/coprocessor side effects.

Allocated encodings whose operands are architecturally UNPREDICTABLE are not
folded into that Undefined reference map. `unpredictable_decode_tb` separately
enumerates 1,066 ARM trap-policy rows, 448 Thumb trap-policy rows, and 12
deterministic executable rows. That enumeration exhausts the affected
MRS/MSR/BX/SWP and register-offset extra-transfer SBZ/SBO fields rather than
sampling canonical encodings. `arm7tdmis_unpredictable_trap_tb` carries 24
representatives through the public pins. The normative distinction and
complete policy inventory are in [UNPREDICTABLE.md](UNPREDICTABLE.md).

## Unaligned and endian evidence

`arm7tdmis_unaligned_access_matrix_tb` is a 72-row reset-per-case pin-level
matrix. Its 64 data rows cover LDR, STR, LDRH, STRH, LDRSH, LDRSB, SWP, and
SWPB at address low bits 0 through 3 in both endian configurations. Each row
checks architectural data, memory, transfer width/direction/address, and SWP
lock lifetime. The eight remaining rows feed every two-bit target suffix to BX
in both endian configurations and require aligned ARM/Thumb fetches of the
correct width. Suffix `10` is explicitly the selected ISA-016
clear-low-bits policy; the other suffixes exercise architecture-defined BX
state/alignment behavior.

The test distinguishes specification from policy. ARMv4T word-load rotation
is architectural. Odd halfword behavior is labeled architecturally
UNPREDICTABLE and checked only as this implementation's deterministic r4p3
bus/lane policy. `arm7tdmis_thumb_halfword_policy_tb` adds 20 Thumb rows for
register/immediate LDRH and STRH plus LDRSH in both lanes and endian modes.
`arm7tdmis_thumb_word_unaligned_policy_tb` adds 48 Thumb register-offset,
immediate-offset, and SP-relative word LDR/STR rows across every address suffix
and both endian modes. Its 12 aligned rows are controls; the remaining 36
freeze the ordinary rotate-on-load/aligned-word-store datapath as project
policy. `arm7tdmis_ldr_pc_unaligned_policy_tb` separately checks all eight
address-suffix/endian combinations for ARM LDR-to-PC. Six rows freeze the
unaligned-data-address policy: ordinary LDR rotation followed by the
architecture-defined pre-v5 LDR-to-PC result alignment.

## ARM single-transfer evidence

Three reset-per-case matrices close the ARM Addressing Mode 2 and Mode 3
functional space:

- `arm7tdmis_single_ls_matrix_tb` executes all 64 combinations of
  P/U/B/W/L and immediate/register offset. Register-offset rows exercise LSL,
  LSR, ASR, and the encoded `ROR #0` RRX case.
- `arm7tdmis_extra_ls_matrix_tb` executes STRH, LDRH, LDRSB, and LDRSH for
  every P/U/W and immediate/register combination (64 rows). The 16
  `P=0,W=1` rows explicitly freeze an ordinary privileged post-index transfer
  at the original base followed by deterministic writeback; ARMv4T labels
  that Mode-3 combination UNPREDICTABLE.
- `arm7tdmis_single_ls_policy_tb` executes 12 defined alias/r15 cases and 14
  statically detectable ARMv4T UNPREDICTABLE operand combinations.

Every ordinary row checks the transfer address, size, direction, privilege,
lock state, load/store data, destination, and base writeback. Unsafe operand
rows require the project's deterministic precise-Undefined policy, including
the exact LR/SPSR/handler state, no data cycle, no successor retirement, and
unchanged memory. The policy is implementation behavior, not an architectural
promise about UNPREDICTABLE encodings.

## Register-banking evidence

ARM's count is 31 physical GPRs: 30 banked r0-r14 locations plus the one
shared PC. The flat regfile array contains those 30 locations and one
inaccessible layout hole at slot 15; PC is owned by the core.
`regfile_banking_tb` performs 599 operations against an independent map. It
checks every r0-r14 view in all seven modes, every physical slot after every
normal/debug/force-User mutation, all three read ports, User/System aliases,
FIQ r8-r14, all r13/r14 banks, and shared PC reads in ARM and Thumb state.

`psr_banking_tb` independently performs 47 operations across all five physical
SPSRs. It proves current-mode write/read/restore selection, every
exception-target save index, all seven mode transitions, User/System
read-as-zero/write-ignore behavior, CLKEN suppression, and reset priority.

`arm7tdmis_register_banking_matrix_tb` carries the architectural User-bank
path through public pins. Its ten reset-per-case rows execute STM^ and LDM^
from FIQ, IRQ, Supervisor, Abort, and Undefined mode. Each row seeds both
banks through instructions, checks seven address phases and seven pipelined
data responses, and compares every flat register slot, all five SPSRs, CPSR,
and memory. System is privileged but has no SPSR; ARMv4T requires S=0 there.
The existing User/System S-form rows in `arm7tdmis_block_ls_policy_tb`
therefore verify the selected precise-Undefined policy rather than a defined
User-bank transfer. The normative map is in
[REGISTER_BANKING.md](REGISTER_BANKING.md).

## ARM block-transfer evidence

`arm7tdmis_block_ls_matrix_tb` executes 272 reset-per-case rows. Its 256
one-hot rows cover every P/U/W/L combination and each individual r0-r15 list
bit with a distinct base register. Sixteen eight-register rows cover
IA/IB/DA/DB, W, and L with a multibeat list. The checks include exact word
addresses, ascending register/address order, N then S transfer
classification, data direction and values, privilege, LOCK, final base, the
ARM r15 store value, and LDM-to-r15 pipeline redirection.

`arm7tdmis_block_ls_policy_tb` adds 21 rows for base-in-list outcomes,
privileged User-bank transfers, STM^ with r15, and LDM^+PC with and without
writeback. It also freezes the implementation policy for statically
detectable ARMv4T UNPREDICTABLE operands: empty lists, Rn=r15, invalid S/W
forms, and S forms in User/System take a precise Undefined exception before
the first data beat. Defined and deterministic-policy rows require their
architectural register, bank, memory, writeback, CPSR, PC, and bus results.

The base-in-list policy distinguishes architecture from implementation. STM
writeback with Rn lowest stores the original base as ARMv4T specifies; a
non-lowest Rn stores the deterministic updated value. LDM writeback with Rn in
the list retains the r4p3-compatible Base Updated result. The independent
`arm7tdmis_stm_base_list_tb`, `arm7tdmis_ldm_abort_base_list_tb`,
`arm7tdmis_ldm_pc_tb`, and `arm7tdmis_pc_write_alignment_tb` regressions cover
the adjacent all-addressing-mode, abort, CPSR-restore, and refill rules.

## ARM swap evidence

The SWP/SWPB rows in `arm7tdmis_unaligned_access_matrix_tb` cover both swap
widths at address low bits 0 through 3 in little- and big-endian
configurations. They check the ARMv4T word-read rotation and aligned
word-write rules, all byte lanes, exact address/size/direction, loaded and
stored data, and exactly two locked accepted transfers.

`arm7tdmis_swp_policy_tb` adds 18 reset-per-case rows. Distinct operands and
the architecturally defined Rd=Rm exchange execute in both Supervisor and
User modes. Rn=Rd, Rn=Rm, and r15 in each operand position take the project's
precise Undefined policy before a data cycle or LOCK assertion. These are
implementation outcomes for ARMv4T UNPREDICTABLE operands, not architectural
guarantees.

`arm7tdmis_swp_bus_matrix_tb` runs 16 word/byte rows across normal completion,
independent read- and write-response CLKEN stalls, read and write Data
Aborts, reset in either response phase, and DBGRQ between the locked
transfers. It requires an uninterrupted same-address read/write pair, stable
pins and state through stalls, correct cancellation/writeback behavior, and
LOCK release on every exit. A read abort must cancel the write address;
either abort suppresses Rd and memory modification. The independent
`arm7tdmis_swp_read_abort_tb`, `arm7tdmis_swpb_data_tb`, cycle, and
debug-injection tests preserve neighboring behavior.

## ARM condition-failure evidence

`arm7tdmis_cond_fail_matrix_tb` executes 1,092 independent rows: Supervisor
and User modes, all 14 ARM conditions that can evaluate false, and 39
representative instruction paths. The path set spans all 16 decode classes,
every multicycle detour, register/flag/PC/base/memory destinations,
policy-Undefined routes, SWI, ready external CDP/MCR/MRC/LDC/STC, and every
supported CP14 side-effect type.

Each row checks both pipelined halves of r4p3 Table 7-23. The returned opcode
and saved address-class tuple are the sequential word read at instruction
PC+8; the simultaneously outgoing address is the sequential opcode read at
PC+12. Opcode/data and User/privileged PROT, LOCK, the coprocessor-following
signals, `DBGINSTRVALID`, and `DBGnEXEC` are exact. The next instruction must
reach Execute one clock later. Snapshots prove all 31 physical GPRs, CPSR,
all five SPSRs, the complete test memory, and CP14 DCC/debug-abort state are
unchanged, with no exception, coprocessor request, DMORE, or LOCK activity.

`arm7tdmis_pabt_pipeline_tb` covers the sequence-specific r4p3 erratum-11
boundary. A condition-failed UDF retires without an exception; a following
unconditional SWI selects SWI, and a following aborted opcode selects PABT.
Both cases check the selected source, handler, mode, LR, SPSR, and absence of
a false Undefined exception. The project deliberately implements this
architecturally corrected behavior only; it has no defect-emulation mode.

## Exception-return evidence

`arm7tdmis_exception_return_matrix_tb` executes 60 reset-per-row cases: all
five modes that own an SPSR, both ARM and Thumb destinations, and six return
forms spanning direct `MOVS`, immediate `SUBS #4/#8`, the separate
register-controlled-shifter path for both operations, and
`LDMIA sp!,{r4,pc}^`.

Each row writes a distinct complete SPSR and independently checks its physical
bank plus the source mode's banked LR/SP. Deliberately misaligned loaded PC
values prove destination-state alignment. The first redirected opcode address,
SIZE, N-cycle classification, and restored User privilege are exact; the
target then reaches Execute with the expected opcode, PC/state, and full CPSR.
Snapshots prove all SPSRs, memory, and every non-destination physical GPR are
unchanged. LDM rows additionally require the exact two data beats, loaded
register, and writeback to the source exception-mode SP after CPSR restoration.
The focused `arm7tdmis_ldm_pc_tb` and
`arm7tdmis_pc_write_alignment_tb` provide independent neighboring coverage.

## Instruction-sequence evidence

`arm7tdmis_sequence_dependencies_tb` runs 16 reset-per-case programs rather
than isolated instructions. It places consumers immediately after producers
for DP Rn, shifted Rm, register-controlled Rs, flags, LDR data, post-index
bases, MUL, both UMULL halves, LDM data/writeback, and store data. A separate
mode sequence changes System → FIQ → Supervisor and uses each banked SP in
the immediately following instruction.

The redirect rows chain B to `MOV pc` to B, B to `LDR pc` to B, and ARM BX to
Thumb BX to ARM B. They count and validate every destination and reject any
retirement of a flushed successor. The self-modifying row overwrites an
opcode that was already eligible for prefetch, executes an explicit branch
refill, and requires the patched instruction rather than the stale captured
word. This checks the code-coherence contract in `INTEGRATION.md`; it does not
claim implicit snooping of instructions already in the pipeline.

`arm7tdmis_thumb_bl_boundary_tb` separately permits an IRQ after the prefix
has committed and injects PABT on the suffix fetch. Both handlers return to
the suffix, which reaches the same target and link value using the preserved
User LR. A third row enters an orphan suffix with a seeded LR. That last
outcome freezes an implementation policy for architecturally UNPREDICTABLE
software, not a portable ARM guarantee.
`arm7tdmis_thumb_bl_pair_policy_tb` covers the complementary orphan prefix:
the prefix value remains in User LR and the ordinary non-suffix successor
retires without a branch. Together the rows demonstrate that there is no
required hidden inter-halfword BL state. Four consecutive external MRC
transfers and their independent responses remain covered by
`arm7tdmis_cp_erratum15_tb`.

Measured structural coverage reporting, exact Chapter 7 legal-equivalence
cross coverage, mutation testing of architectural controls, independent
QEMU/compiler execution, constrained-random differential/policy testing, the
pinned public ARM/Thumb suite, deterministic sanitizing soak, encoding-level
required-bin functional coverage, and formal proof/reachability evidence are
implemented above. The remaining independent-review and physical-integration
release gates remain explicit in `TASKS.md` §31.
