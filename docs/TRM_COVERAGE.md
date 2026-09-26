# ARM DDI 0234B complete coverage inventory

This is the executable whole-manual audit for the 242-page ARM7TDMI-S r4p3
Technical Reference Manual. It complements per-requirement traceability:
`verification/trm_coverage.json` proves that the review did not silently omit
a numbered section, subsection, table, figure/timing diagram, or Appendix A
signal, while `verification/traceability.json` maps every §31 requirement in
both directions to implementation and verification sources.

The locked source is `ARM DDI 0234B`, ARM7TDMI-S revision r4p3:

| Property | Audited value |
|---|---|
| PDF pages | 242 |
| PDF bytes | 1,477,711 |
| PDF SHA-256 | `bb9cd0e3f2b7e2fdca4ff961cdfc5c9c85c063842e491f8997a504d3241baa14` |
| Numbered sections and subsections | 207 |
| Numbered tables | 65 |
| Numbered figures and timing diagrams | 44 |
| Appendix A named signals | 45 |
| Total independently assigned inventory items | 361 |

## Disposition model

Every inventory item is assigned exactly once:

- `implemented-and-tested` means all owning §31 requirements are checked and
  the cited RTL/document/test paths exist.
- `integration-obligation` means the core-side behavior or adapter is checked,
  while the containing system owns the named external component or connection.
- `erratum-corrected` requires checked evidence under the frozen corrected
  errata policy.
- `external-out-of-scope` is an explicit selected-profile boundary, not a
  missing implementation. Examples are FPGA power rails, ASIC ATPG insertion,
  and hard-macro-only legacy pins.
- `open-requirement` must name at least one unchecked §31 row. It is coverage
  of a known gap, never evidence that the behavior is finished.

The validator rejects a changed PDF, any inventory difference or duplicate,
an unassigned or multiply assigned item, an unknown requirement, an absent
evidence path, an open group with no open owner, or a completed disposition
that cites an unchecked requirement.

## Important profile boundaries

Chapter 6 requires the ARM-side ETM7 signal mapping. The repository supplies
and tests that adapter but does not claim an ETM macrocell, trace encoder, or
trace storage. Appendix A's `SCANENABLE`, `SCANIN`, and `SCANOUT` describe
ASIC scan insertion; the FPGA profile deliberately provides only the named
`arm7tdmis_no_dft` compatibility wrapper and excludes it from FPGA sources.
`VDD` and `VSS` are device/board rails, not synthesizable RTL ports. Appendix
B Table B-2 lists legacy hard-macrocell signals that ARM7TDMI-S intentionally
removed; recreating them would not improve r4p3 conformance.

Chapter 8/Table 8-1 is retained as provisional synthesized-macrocell/silicon
timing guidance rather than a portable FPGA pin guarantee.
FPGA-009 closes its target-specific soft-FPGA disposition with a locked
37-row/five-figure contract, active raw-profile SDC mapping, mutation tests,
and four-corner fitted evidence; the full row-by-row result is in
[AC_TIMING.md](AC_TIMING.md). The system-speed debug abort and Debug Status
write ambiguity are likewise closed under DBG-008 and DBG-009 with dedicated
directed evidence rather than being hidden by the complete inventory.

## Reproducing the audit

Run:

```sh
make -C scripts trm-coverage
```

This writes `reports/generated/trm-coverage-report.json` with schema
`arm7tdmis-trm-coverage-v1`, the clean/dirty Git identity, every input hash,
inventory and disposition totals, and normalized group ownership. The phase
runs in quick, CI, and full regression. Release evidence independently checks
the report's clean same-commit identity and all input hashes before archiving
it. The checker and its mutation tests are implemented in Rust at
`verification/constrained_random_rs/src/trm_coverage.rs`.
