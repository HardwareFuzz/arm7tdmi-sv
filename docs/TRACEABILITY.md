# Bidirectional release traceability

The live traceability table is the generated
`reports/generated/traceability-report.json`, schema
`arm7tdmis-traceability-v1`. Generate and fail-hard validate it with:

```sh
make -C scripts traceability
```

The table has one row for every §31 requirement ID, including open
requirements. Each row contains:

| Direction | Recorded fields |
|---|---|
| Requirement → evidence | canonical TASKS section and external source family, RTL/FPGA implementation paths, directed/random/formal test paths or targets, a stable requirement acceptance-bin name, expected regression phases, ledger status, and latest local result |
| RTL → requirement | every tracked `rtl/*.sv` or `rtl/**/*.sv` file and one or more owning §31 IDs |
| Verification → requirement | every tracked SystemVerilog test/program, formal source, QEMU/compiler source, and Rust verification source maps to one or more owning §31 IDs |

The committed `verification/traceability.json` is the reviewed ownership map.
It contains prefix defaults, narrow exceptions for special/open requirements,
and reverse file rules. The Rust
`verification/constrained_random_rs/src/traceability_report.rs` combines that
map with the canonical checkboxes and evidence citations in `TASKS.md`; the task
ledger is not duplicated in the mapping.

## Acceptance and failure behavior

`--check` fails if any requirement lacks a source, implementation disposition,
test disposition, coverage bin, status, or result; if a mapping references an
unknown ID; or if any tracked RTL or verification file is unmapped. “N/A” and
“OPEN” are explicit dispositions, not empty cells. An open row remains open
in the generated table and cannot inherit a neighboring checked result.

When a regression report is available, each row records the report commit and
aggregate status plus the statuses of cited phases that were present. Without
one, the row says that no local report was supplied. The mandatory
`traceability` regression phase runs last, after smoke execution, so its
generated table sees every earlier phase in that run.

The release-evidence packager requires the traceability phase, checks the
report schema, commit identity, and empty unmapped lists, then hashes and
archives the report. The committed mapping and generator are part of the
regression source digest; immutable latest results belong to the
commit-addressed evidence archive.

## Scope of the reverse proof

The reverse map proves ownership at tracked source-file granularity. Features
within a shared module map forward through their individual requirement rows
and cited tests. Generated simulator/Quartus files and ignored local reports
are outputs rather than source features, so they are covered by their
producing requirement and release-manifest hashes instead of reverse source
rows.
