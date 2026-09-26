# Fatal warning and assertion policy

Every Verilator compile uses `-Wall`; Verilator therefore returns nonzero for
all warnings that are not listed below. Every simulation compile uses
`--assert`. Tests use `$fatal` for failed checks and timeouts, never the
nonfatal `$error` system task. The harness proves that a real simulator
`$fatal` propagates through Make and the Rust regression runner as a nonzero
top-level result.

`verification/constrained_random_rs/src/hdl_source_contract.rs` enforces the complete command-line and
inline allowlists, paired `lint_off`/`lint_on` pragmas, fatal flags, and the
absence of `$error`. Adding a warning class requires updating that test and
this ledger in the same reviewed change.

## Reviewed exceptions

| Warning | Scope | Owner | Rationale | Review trigger / expiry |
|---|---|---|---|---|
| `UNUSEDPARAM` | Verilator command line for all elaborations | RTL package maintainers | The five shared packages are public APIs. A raw core, wrapper, unit bench, and integration bench legitimately consume different subsets, so a constant unused in one selected top can be required by another. The unsuppressed raw-top audit reports only these package constants. | Expires if the package API is split per consumer or the selected lint tool can distinguish a public library constant from dead local configuration. Re-audit on every Verilator major-version change. |
| `SYNCASYNCNET` | Integration-build command line and a few legacy local bench scopes | Verification maintainers | External `nRESET` is asynchronously asserted only by `arm7tdmis_reset_sync`, while pin-level checkers, counters, and behavioral memory also sample the same stimulus synchronously. This intentional mixed observation exists only in simulation; CDC ownership and the sole asynchronous RTL use are separately checked. | Expires when all synchronous bench observers consume a distinct synchronized reset without weakening raw reset-contract checks. Re-audit if reset topology changes. |
| `DECLFILENAME` | Local scope in parameterized integration-test files | Verification test owner | Some reset-per-case benches declare a small helper module in the same file as the manifest top. The module names intentionally differ from the filename and are not production compilation units. | Expires if helpers move to one-module-per-file sources or become shared infrastructure. |
| `UNUSEDSIGNAL` | Local drain scopes only | Owning RTL/test module maintainer | Public pins, reserved storage, and policy fields can be intentionally unobserved in a particular wrapper or test. Each scope explicitly reduces those named signals into a no-op drain so omission is visible in review; the pragma suppresses only the drain wire itself. | Expires when the public/reserved field is removed, gains a functional consumer, or the lint tool gains a portable intentional-unused attribute. |

`UNOPTFLAT` was previously disabled for every integration build. An
unsuppressed build of the largest debug-system-speed bench and representative
multi-instance benches produced no such warning, so the stale exception was
removed. There is no blanket waiver for combinational loops, latches, width
errors, pin mismatches, unsupported constructs, or assertion failures.
