# FPGA performance and integration budget

This is the checked version `0.9.0-dev` characterization. All profiles target
Cyclone V `5CSEBA6U23I7` with Quartus Lite 17.0.2. The standalone profiles
use their supplied virtual-boundary constraints; the separate official
MiSTer-template result uses its real framework clocks, placement, and I/O.
The immutable machine-readable snapshot is
`verification/fpga_characterization.json`; it binds these published results
to the SHA-256 of every RTL, project, constraint, top, and report-checker
input used by the three fresh characterization flows at commit `67ff42444d0d`.

The pinned official MiSTer template at commit
`69b8a2acc6d84dd313b5abcba6a17155287ed3d8` builds `sys_top` with a
12.500 MHz core clock and fitter seed 4. Its checked full-project result is
14,188 ALMs, 21,415 registers, 384,488 memory bits, 39 DSP blocks, and three
PLLs, with +0.031 ns minimum setup and +0.060 ns minimum hold slack across
four corners. This includes the complete framework, not just the CPU, so its
resources are not comparable to the standalone rows below. The build report
and 2,665,704-byte RBF (SHA-256
`9405e30c869cd9190603f71434e20093ba584a535b5a61792b3a2602f874068a`)
are validated and archived by the release-evidence flow.

## Clock and CPU enable

The checked canonical-wrapper `CLK` is 25 MHz (40 ns). The raw conformance
profile is checked at 16 MHz (62.5 ns) because it applies every applicable
Chapter 8/Table 8-1 boundary budget, including the long combinational
`DBGRNG` response. There is one real clock and no generated or gated clock.
`CPU_CE`/raw `CLKEN` may be high on every rising edge for a maximum 1:1
master-clock-to-CPU-advance ratio, or may insert any finite number of disabled
edges. One enabled edge can advance at most one CPU cycle; the effective CPU
rate is therefore `CLK frequency * enabled-edge fraction`.

The minimum reported same-clock Fmax across the slow timing models is
27.80 MHz for the trimmed canonical-wrapper profile and 16.50 MHz for the
raw feature-complete profile. These figures have 2.80 MHz and 0.50 MHz
headroom over their checked 25 MHz and 16 MHz clocks, respectively. They are
post-fit characterization results for virtual boundary pins, not a promise
that a particular board or framework will reach those rates.

## Request latency

The canonical wrapper presents one outstanding request. It captures a raw
address phase on an enabled CPU edge, so the earliest external completion is
a later `CLK` edge with `MEM_VALID && MEM_READY`. `MEM_READY` is independent
of `CPU_CE`: a completion received while CE is low is buffered and consumed
exactly once on a later enabled edge. There is no fixed maximum memory
latency, but a live target must eventually respond.

Back-to-back completions and request capture may share an enabled edge.
`MEM_MORE`, `MEM_SEQUENTIAL`, and `MEM_LOCK` do not combine handshakes; every
beat completes separately. The raw interface instead shares `CLKEN` between
the core and its pipelined response, as specified in
[RAW_BUS.md](RAW_BUS.md).

## Resources and clock enables

| Profile | Fitted ALMs | Registers | Registers with clock enable | M10K / memory bits | DSP blocks | Enforced budget |
|---|---:|---:|---:|---:|---:|---|
| Canonical trimmed wrapper (`ENABLE_DEBUG=0`, `ENABLE_COPROCESSOR=0`) | 3,500 | 2,537 | 2,142 | 0 / 0 | 6 | 5,000 ALMs, 4,096 registers, 8 DSPs, 0 memory bits |
| Raw feature-complete `arm7tdmis_top` | 5,038 | 3,313 | 3,267 | 0 / 0 | 6 | 7,500 ALMs, 6,000 registers, 8 DSPs, 0 memory bits |

The trimmed profile retains 1,500 ALMs, 1,559 registers, and two DSP blocks
of its project budget. The raw profile retains 2,462 ALMs, 2,687 registers,
and two DSP blocks. Neither profile infers MLAB/M10K storage; all
architectural banks are registers. Both infer the six DSP blocks used by the
multiplier. Quartus reports clock-enable inference on 2,142 of 2,165
synthesis registers in the trimmed profile and 3,267 of 3,313 in the raw
profile.

The two rows are different integration surfaces and must not be subtracted
to claim an isolated optional-feature cost.

## Optional-feature costs

Four additional fits select the same `arm7tdmi_mister` top, device, source
manifest, 25 MHz clock constraint, virtual boundary pins, and little-endian
setting. Only `ENABLE_DEBUG` and `ENABLE_COPROCESSOR` vary:

| Profile | Debug | Coprocessor | Fitted ALMs | Registers | CE registers | Memory bits | DSPs | Worst slow-model Fmax | Core dynamic power |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| `none` | 0 | 0 | 3,499 | 2,538 | 2,142 | 0 | 6 | 27.55 MHz | 29.06 mW |
| `debug` | 1 | 0 | 4,747 | 3,602 | 3,049 | 0 | 6 | 26.67 MHz | 34.60 mW |
| `coprocessor` | 0 | 1 | 3,765 | 2,647 | 2,316 | 0 | 6 | 28.63 MHz | 31.51 mW |
| `both` | 1 | 1 | 4,996 | 3,841 | 3,234 | 0 | 6 | 26.75 MHz | 38.31 mW |

The isolated fitted deltas from `none` are:

| Enabled option set | ALM delta | Register delta | Memory-bit delta | DSP delta | Fmax delta | Core-dynamic-power delta |
|---|---:|---:|---:|---:|---:|---:|
| `debug` | +1,248 | +1,064 | 0 | 0 | -0.88 MHz | +5.54 mW |
| `coprocessor` | +266 | +109 | 0 | 0 | +1.08 MHz | +2.45 mW |
| `both` | +1,497 | +1,303 | 0 | 0 | -0.80 MHz | +9.25 mW |

Fitter variation means the combined delta need not equal the sum of the two
single-option deltas, and a positive Fmax delta is not an architectural
speedup. These are like-for-like implementation measurements, not an
additive cost model. The machine-readable source is
`reports/generated/quartus-options.json`.

## Power estimate

PowerPlay vectorless estimation at the checked profile clock assumptions
reports:

| Profile | Total | Core dynamic | Core static | I/O |
|---|---:|---:|---:|---:|
| Canonical trimmed wrapper | 452.41 mW | 31.29 mW | 412.45 mW | 8.67 mW |
| Raw feature-complete (16 MHz) | 445.09 mW | 24.09 mW | 412.39 mW | 8.61 mW |

The PowerPlay confidence is **Low** because no workload VCD/toggle file or
board thermal model is supplied. The selected SoC device also emits Quartus
warning 215050 for absent HPS activity; HPS is disabled in both projects.
The report checker allows only that identified power warning and rejects
other critical warnings. These numbers are useful fabric-characterization
estimates, but this is not a board-power estimate or a thermal sign-off.

## Reset and endian configuration

Assert `RESET_N` low for at least two `CLK` rising edges. Reset assertion is
asynchronous; architectural release is synchronized and takes two rising
edges after deassertion. The raw top uses the corresponding active-low
`nRESET` rule.

`BIG_ENDIAN=0` selects little endian and `BIG_ENDIAN=1` selects big endian at
synthesis. The setting is static for an instance and cannot change while
running. It changes byte/halfword lane placement, not the resource budget or
clocking contract.

## Reproducing characterization

Run all complete checked flows from the repository root:

```sh
make -C scripts quartus-compile
make -C scripts quartus-conformance-compile
make -C scripts quartus-option-characterization
```

Each target runs synthesis, fit, assembly, four-corner TimeQuest, and
vectorless PowerPlay. The first two call the Rust `quartus-report-check`
command directly; the option target applies the same checker to all four fits
through the Rust `quartus-option-characterization` command. Each option profile uses Quartus
Auto Fit so a single placement that misses the common 0.25 ns synchronous
input-hold contract is retried without weakening that boundary requirement.
The checked snapshot records the producing commit and commands and hashes all
historical characterization inputs. The two Python checker files named in that
snapshot are verified from the recorded Git commit, preserving their original
hashes after the live entry points moved to Rust. Current source, constraint,
project, or top changes still make `make -C scripts harness-unit` fail until
fresh fit evidence is reviewed and deliberately published. The Rust checker
is covered by its own parser and mutation tests; the historical fit does not
claim to have been run with the Rust checker.
The checks fail on missing
stages or images, unexpected top/device, resource-budget overruns, missing
clock-enable/Fmax/power evidence, negative slack, unconstrained setup/hold
paths, and unapproved critical warnings.
