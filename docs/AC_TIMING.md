# Chapter 8 AC timing disposition

This is the complete FPGA disposition of ARM DDI 0234B r4p3 §8.1, §8.2,
Figures 8-1 through 8-5, Table 8-1, and Appendix B.3. The machine-readable
source is `verification/ac_timing.json` (schema
`arm7tdmis-ac-timing-map-v1`).

## Scope of the source numbers

Table 8-1 is titled “Provisional AC parameters.” It expresses target values
as percentages of `CLK` at maximum operating frequency and directs the
integrator to the silicon supplier for details. Appendix B.3 says the timing
constraints balance the synthesized core’s area against its external timing.
The values are therefore provisional synthesized-macrocell/silicon targets,
not immutable timing guarantees at arbitrary FPGA pins.

The raw conformance profile makes one explicit target choice: Cyclone V
`5CSEBA6U23I7`, Quartus Lite 17.0.2, 16 MHz, and a 62.5 ns `CLK`. For applicable
synchronous inputs it uses:

```text
max input delay = negative Table 8-1 setup fraction, relative to capture edge
min input delay = 0.25 ns target clock-skew margin for a Table 8-1 0% hold
```

For applicable synchronous outputs it uses:

```text
max output delay = 62.5 ns - Table 8-1 clock-to-valid fraction
min output delay = 0 ns; the fitted design must report positive hold slack
```

This translation reserves the original fraction for the FPGA’s internal
port-to-register or register-to-port path. It is a checked target constraint,
not a claim that the original percentage applies to every containing MiSTer
core. A containing design must replace the virtual boundary with constraints
for its real clocks, bridges, memories, pins, and receiver requirements.
The full raw profile includes the combinational EmbeddedICE-RT range
comparators and is deliberately characterized below the separate 25 MHz
trimmed MiSTer profile; no extra architectural cycle is inserted merely to
inflate the raw-profile timing number.

## Complete Table 8-1 mapping

The source spelling is preserved, including `tohdbctl`.

| Symbol | Source parameter | Min | Max | Figure / raw mapping | 16 MHz SDC disposition |
|---|---|---:|---:|---|---|
| `tcyc` | CLK cycle time | 100% | – | primary `CLK` | 62.500 ns clock |
| `tisclken` | CLKEN setup | 40% | – | Fig. 8-1 `CLKEN` | input max −25 ns |
| `tihclken` | CLKEN hold | – | 0% | Fig. 8-1 `CLKEN` | input min 0.25 ns |
| `tisabort` | ABORT setup | 15% | – | Fig. 8-1 `ABORT` | input max −9.375 ns |
| `tihabort` | ABORT hold | – | 0% | Fig. 8-1 `ABORT` | input min 0.25 ns |
| `tisrdata` | RDATA setup | 10% | – | Fig. 8-1 `RDATA[*]` | input max −6.25 ns |
| `tihrdata` | RDATA hold | – | 0% | Fig. 8-1 `RDATA[*]` | input min 0.25 ns |
| `tovaddr` | CLK to ADDR valid | – | 90% | Fig. 8-1 `ADDR[*]` | output max 6.25 ns |
| `tohaddr` | ADDR hold | >0% | – | Fig. 8-1 `ADDR[*]` | output min 0 ns + positive fitted hold slack |
| `tovctl` | CLK to control valid | – | 90% | Fig. 8-1 / App. B.3 `WRITE`, `SIZE`, `PROT`, `LOCK` | output max 6.25 ns |
| `tohctl` | Control hold | >0% | – | same control group | output min 0 ns + positive fitted hold slack |
| `tovtrans` | CLK to TRANS valid | – | 50% | Fig. 8-1 `TRANS[*]` | output max 31.25 ns |
| `tohtrans` | TRANS hold | >0% | – | Fig. 8-1 `TRANS[*]` | output min 0 ns + positive fitted hold slack |
| `tovwdata` | CLK to WDATA valid | – | 40% | Fig. 8-1 `WDATA[*]` | output max 37.5 ns |
| `tohwdata` | WDATA hold | >0% | – | Fig. 8-1 `WDATA[*]` | output min 0 ns + positive fitted hold slack |
| `tiscpstat` | CPA/CPB setup | 20% | – | Fig. 8-2 `CPA`, `CPB` | input max −12.5 ns |
| `tihcpstat` | CPA/CPB hold | – | 0% | Fig. 8-2 `CPA`, `CPB` | input min 0.25 ns |
| `tovcpctl` | CLK to coprocessor control valid | – | 80% | Fig. 8-2 `CPnMREQ`, `CPSEQ`, `CPnOPC`, `CPnTRANS`, `CPTBIT` | output max 12.5 ns |
| `tohcpctl` | Coprocessor control hold | >0% | – | same coprocessor group | output min 0 ns + positive fitted hold slack |
| `tovcpni` | CLK to CPnI valid | – | 40% | Fig. 8-2 `CPnI` | output max 37.5 ns |
| `tohcpni` | CPnI hold | >0% | – | Fig. 8-2 `CPnI` | output min 0 ns + positive fitted hold slack |
| `tisexc` | nFIQ/nIRQ/nRESET setup | 10% | – | Fig. 8-3 `nFIQ`, `nIRQ`; reset replacement below | input max −6.25 ns |
| `tihexc` | nFIQ/nIRQ/nRESET hold | – | 0% | Fig. 8-3 `nFIQ`, `nIRQ`; reset replacement below | input min 0.25 ns |
| `tiscfg` | CFGBIGEND setup | 10% | – | Fig. 8-3 `CFGBIGEND` | input max −6.25 ns |
| `tihcfg` | CFGBIGEND hold | – | 0% | Fig. 8-3 `CFGBIGEND` | input min 0.25 ns |
| `tisdbgstat` | Debug-status inputs setup | 10% | – | Fig. 8-4 `DBGRQ`, `DBGBREAK`, `DBGEXT[*]` | input max −6.25 ns |
| `tihdbgstat` | Debug-status inputs hold | – | 0% | same debug-input group | input min 0.25 ns |
| `tovdbgctl` | CLK to debug-control valid | – | 40% | Fig. 8-4 `DBGACK`, `DBGCOMMTX`, `DBGCOMMRX` | output max 37.5 ns |
| `tohdbctl` | Debug-control hold | >0% | – | same first debug-output group | output min 0 ns + positive fitted hold slack |
| `tistcken` | DBGTCKEN setup | 40% | – | Fig. 8-5 `DBGTCKEN` | input max −25 ns |
| `tihtcken` | DBGTCKEN hold | – | 0% | Fig. 8-5 `DBGTCKEN` | input min 0.25 ns |
| `tistctl` | DBGTDI/DBGTMS setup | 35% | – | Fig. 8-5 `DBGTDI`, `DBGTMS` | input max −21.875 ns |
| `tihtctl` | DBGTDI/DBGTMS hold | – | 0% | same scan-control group | input min 0.25 ns |
| `tovtdo` | CLK to DBGTDO valid | – | 20% | Fig. 8-5 `DBGTDO` | output max 50 ns |
| `tohtdo` | DBGTDO hold | >0% | – | Fig. 8-5 `DBGTDO` | output min 0 ns + positive fitted hold slack |
| `tovdbgstat` | CLK to debug-status valid | – | 40% | Fig. 8-4 `DBGRNG[*]` | output max 37.5 ns |
| `tohdbgstat` | Debug-status hold | >0% | – | Fig. 8-4 `DBGRNG[*]` | output min 0 ns + positive fitted hold slack |

Table 8-1 says that a displayed 0% includes hold to the active edge plus
maximum internal clock-buffer skew. The FPGA translation therefore does not
advertise “zero physical delay”: source 0% input rows receive a 0.25 ns
target clock-skew margin, while source >0% output rows use a 0 ns external
boundary and rely on fail-hard fitted hold analysis to prove positive slack.

## Figure 8-4 naming conflict

The table calls its input pair `tisdbgstat`/`tihdbgstat`, while Figure 8-4
labels the same input arrows `tisdbgctl`/`tihdbgctl`. The table also contains
`tovdbgctl`/`tohdbctl` and `tovdbgstat`/`tohdbgstat`, while the figure labels
both visible output groups with the latter pair. The contract preserves all
six table symbols and records the figure aliases. The first 40% output pair
maps to `DBGACK`/DCC handshake status and the second to `DBGRNG`; both receive
the same numeric target, so the naming ambiguity cannot silently omit or
weaken either group.

## Integration-owned and non-table boundaries

- Clock: the containing design supplies `CLK`; this profile creates one real
  clock and no generated/gated clock.
- Reset: `nRESET` and `DBGnTRST` assert asynchronously. The architectural
  reset synchronizer owns `nRESET` release; raw debug-reset release belongs to
  the containing integration. They are false-pathed instead of pretending
  that the `tisexc` synchronous percentage describes asynchronous assertion.
- Debug synchronization: the raw profile times `DBGRQ`, `DBGBREAK`,
  `DBGEXT`, `DBGTCKEN`, `DBGTDI`, and `DBGTMS` as synchronous. An external
  pod TCK or asynchronous event needs a containing synchronizer/handshake.
- Test-only: Appendix A’s `SCANENABLE`, `SCANIN`, and `SCANOUT` are ASIC ATPG
  insertion signals. The FPGA profile excludes them and supplies
  `arm7tdmis_no_dft` only as an explicit compatibility tie-off.
- Power: `VDD` and `VSS` are FPGA-device/board rails, not RTL timing ports.
- Non-table raw ports: Appendix A defines `DBGEN` as a static tie HIGH when
  debug is used or LOW otherwise, so the raw profile false-paths this
  integration-owned strap rather than inventing cycle-level timing.
  `DBGnEXEC`, `DBGINSTRVALID`, `DBGnTDOEN`, and `DMORE` retain a
  target-specific 0–5 ns output window. No ARM percentage is invented for
  signals absent from Table 8-1.

## Selected MiSTer framework timing

The real framework integration makes a separate, board-specific timing
choice. It targets the official MiSTer `Template_MiSTer` project on
`5CSEBA6U23I7`, with the core PLL producing an exact 12.500 MHz / 80.000 ns
`clk_sys`. [`examples/mister_framework/Template.sdc`](../examples/mister_framework/Template.sdc)
layers these constraints on the framework's own SDC:

- derived PLL clocks and uncertainty for all framework domains;
- both 148.54 MHz scaled-video and 12.5 MHz direct-video capture clocks at
  the HDMI DDIO output, including the forwarded-clock inversion and only the
  two inactive clock-mux cross-pair exceptions;
- ADV7513 video setup/hold requirements of 1.0/0.7 ns;
- audio MCLK, the framework's divide-by-16 I2S clock, and ADV7513
  I2S/LRCLK setup/hold requirements of 2.0/2.0 ns; and
- exact, enumerated exceptions for asynchronous open-drain/status inputs and
  non-clocked board/status outputs, with no catch-all port wildcard.

A clean Quartus Lite 17.0.2 four-corner fit at fitter seed 4 reports
+0.031 ns minimum setup and +0.060 ns minimum hold slack. All five TimeQuest
unconstrained categories—clocks, input ports, input-port paths, output ports,
and output-port paths—are zero for setup and hold. The checked parser permits
only the four exact optional-filter diagnostics already present in the pinned
upstream `sys_top.sdc`; any additional ignored constraint or critical warning
fails the build.

## Reproducing the contract

Run:

```sh
make -C scripts ac-timing
make -C scripts quartus-conformance-compile
make -C scripts mister-framework
```

The first command validates all 37 rows, all five figures, the exact PDF
identity, every percentage and signal group, the Figure 8-4 ambiguity, the
physical-scope caveat, integration ownership, and the corresponding active
SDC commands. It writes `reports/generated/ac-timing-report.json` with schema
`arm7tdmis-ac-timing-v1`. The Rust checker and independent source-row
reference live in `verification/constrained_random_rs/src/ac_timing.rs` and
`verification/constrained_random_rs/src/ac_timing_reference.json`. The second
runs fitted TimeQuest analysis; the
existing report checker rejects negative setup/hold slack and unconstrained
paths. The third performs the pinned official-framework compile and applies
the stricter real-framework report checks described above.
