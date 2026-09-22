# Architecture Documentation

These are historical implementation notes, not conformance evidence. `TASKS.md` §31 is
the canonical audited status and release gate; the ARM ARM/TRM/errata are the behavior
specifications. Neither the RTL nor these documents are a source of truth merely
because they agree with each other.

## Documents

- **[STANDALONE.md](STANDALONE.md)** — execute a supplied ARM/Thumb image on
  the RTL and capture observed exceptions plus final registers/RAM without
  a reference simulator or golden-state files.

- **[PIPELINE.md](PIPELINE.md)** — 3-stage F/D/E pipeline structure, stage registers, bus-cycle overlap in the E substate FSM, the `issue_fetch` `state_next` gate (one of the load-bearing decisions of the whole design), `de_q` staleness traps and the latch protocol.
- **[TABLE7_MATRIX.md](TABLE7_MATRIX.md)** — audited Table 7-2 summary
  and detailed Tables 7-3 through 7-23 mapped to the full-phase oracle and
  registered specialized evidence.

- **[DEBUG.md](DEBUG.md)** — EmbeddedICE-RT macrocell (r4p3 register map, WP comparators with TRM-correct XNOR+mask shape, CHAIN/RANGE coupling, debug-state FSM), JTAG TAP (16-state controller, IDCODE, scan chains 1+2), scan-chain-1 instruction-injection runtime path, CP14 DCC data flow.
- **[COPROCESSOR.md](COPROCESSOR.md)** — bare-core ownership and exact CP14
  decode, raw pipeline-following pins, CPA/CPB claim/busy/completion behavior,
  register and variable-length memory transfers, abandonment, and the frozen
  corrected policy for r4p3 errata 14 and 15.
- **[INTEGRATION.md](INTEGRATION.md)** — versioned canonical FPGA wrapper,
  valid/ready memory timing, CPU clock-enable bridge, byte lanes, reset/CDC
  ownership, and optional debug/coprocessor parameters.
- **[SAVESTATE.md](SAVESTATE.md)** — opt-in versioned CPU state schema,
  quiescence handshake, exact word map, restore rules, and whole-system
  snapshot responsibilities.
- **[GENERIC_SOC.md](GENERIC_SOC.md)** — synthesizable
  ROM/RAM/timer/UART/status example, seven-group ARMv4T smoke ROM,
  source-to-RTL equivalence, and MiSTer video/LED result protocol.
- **[RAW_BUS.md](RAW_BUS.md)** — raw `arm7tdmis_top` address/response edge
  convention, N/S/I/C history, CLKEN/ABORT sampling, endian lanes,
  LOCK/DMORE promises, Chapter 7 interpretation, and reusable checker.
- **[TRACE.md](TRACE.md)** — `DBGINSTRVALID`/`DBGnEXEC` semantics and the
  exact Chapter 6 external ETM7 signal-adapter/tie-off contract.
- **[ERRATA.md](ERRATA.md)** — official errata-list provenance/hash, the
  corrected-default policy, and complete conditions-to-test mapping for all
  15 entries.
- **[VERIFICATION.md](VERIFICATION.md)** — fail-hard regression ordering,
  intentional-failure sentinel, reproducibility metadata, and the
  `arm7tdmis-regression-v1` machine-readable result schema.
- **[TRACEABILITY.md](TRACEABILITY.md)** — bidirectional requirement-to-
  evidence and source/test-to-requirement mapping, generated-result schema,
  and fail-hard completeness rules.
- **[SUPPORT.md](SUPPORT.md)** — supported API, simulator, synthesis-tool,
  MiSTer, and PocketStation profiles.
- **[PERFORMANCE.md](PERFORMANCE.md)** — checked Cyclone V clock/CE,
  transaction-latency, resource, Fmax, power, reset, and endian budgets.
- **[LIMITATIONS.md](LIMITATIONS.md)** — prerelease limitations mapped to the
  authoritative open requirements.
- **[PROVENANCE.md](PROVENANCE.md)** — SPDX/license inventory and rules for
  specifications, third-party tests, firmware, BIOS, ROM, and reference code.
- **[WARNING_POLICY.md](WARNING_POLICY.md)** — mechanically enforced fatal
  warning/assertion policy and the owner, rationale, and expiry condition for
  every command-line or inline suppression class.
- **[PSR.md](PSR.md)** — CPSR/SPSR bit map, MSR field and privilege rules,
  deterministic invalid-mode and absent-SPSR policy, and the frozen CPSR.T
  behavior.
- **[REGISTER_BANKING.md](REGISTER_BANKING.md)** — complete 31-GPR and
  five-SPSR physical map, mode aliases, PC ownership, User-bank `^` rules,
  debug routing, and exhaustive ISA-017 evidence.
- **[UNPREDICTABLE.md](UNPREDICTABLE.md)** — normative ledger separating
  ARMv4T guarantees from every deterministic trap/result/RAZ-WI policy selected
  for architecturally UNPREDICTABLE or UNKNOWN behavior.

- **[MULTIPLY.md](MULTIPLY.md)** — Multiply forms (MUL/MLA/UMULL/SMULL/UMLAL/SMLAL), m-parameter cycle shaping, the UMLAL/SMLAL 2-cycle accumulator read across S_EXEC + S_MULL_ACC and the latches that make it work.

- **[EXCEPTIONS.md](EXCEPTIONS.md)** — All seven exception types (Reset/UNDEF/SWI/PABT/DABT/IRQ/FIQ), priority ordering, class/state-specific links, banked r14 writeback, SPSR save, the `data_abort_now` vs `data_abort_q` memory-completion logic, LDM DABT restart-safety, and the fully cross-product-tested DP/LDM exception-return mechanisms.

## Reading order

If you've never seen this codebase: PIPELINE → PSR → EXCEPTIONS → MULTIPLY →
COPROCESSOR → DEBUG. Each builds on the previous one's vocabulary.

If you're debugging a specific area: jump to the relevant doc; they're independent.

If you're modifying RTL: the docs are *not* exhaustive — they capture the load-bearing ideas. The packages in `rtl/*.sv` (instr / types / bus / debug / psr) have authoritative field definitions; the per-module headers explain that module's contract.

## TRM cross-references

The ARM ARM7TDMI-S r4p3 TRM (`ARM_DDI_0234B_ARM7TDMI-S_r4p3_TRM.pdf` at the repo root) is the architectural spec. Citations throughout the docs use chapter / section numbers (e.g., TRM §5.20.2 for the watchpoint XNOR+mask shape). When in doubt, the TRM wins.

`TASKS.md` is the implementation roadmap and contains a `§30` addendum that catches several real-world traps the TRM glosses over.
