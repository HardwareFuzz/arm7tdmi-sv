# Chapter 7 Cycle-Conformance Matrix

This is the audited map from ARM DDI 0234B Table 7-2 and detailed
Tables 7-3 through 7-23 to RTL and fail-hard pin-level evidence. It closes
BUS-002/BUS-003 at the directed-evidence level and the exact legal-equivalence
cycle crosses required by VAL-004. The separate 32-seed all-class external-
event campaign closes VAL-005, and independent complete-domain encoding
coverage closes VAL-006. The mandatory formal phase separately closes
VAL-007/008 with 14 proofs and 77 named reachability covers.

## Oracle and phase convention

Chapter 7 is a pipelined bus description. In a numbered processor cycle,
`RDATA`, `WDATA`, and `ABORT` complete the preceding accepted address phase,
while `ADDR`, `WRITE`, `SIZE`, `PROT`, `LOCK`, `TRANS`, and `DMORE` already
describe the next address phase. An I phase can merge into a following S
phase at the same address. [RAW_BUS.md](RAW_BUS.md) gives the exact edge and
history rules.

`arm7tdmis_table7_core_phase_matrix_tb.sv` is the central full-phase oracle.
Its 17 base-family rows run in both endian configurations, once continuously
and once with deterministic 1-to-4-cycle CLKEN holds at every Execute phase,
for 68 reset-isolated cases. Every enabled instruction clock compares:

- Address-class: `ADDR`, `WRITE`, `SIZE`, `PROT`, `LOCK`, `TRANS`, and
  `DMORE`.
- Data/response: `WDATA`, phase-delayed `RDATA`, and `ABORT`.
- Coprocessor followers: `CPnMREQ`, `CPSEQ`, `CPnTRANS`, `CPnOPC`,
  `CPTBIT`, and `CPnI`.
- Architecture/events: `CLKEN`, instruction word and PC, last executed PC,
  CPSR, multicycle state, exception selection, `DBGACK`, `DBGnEXEC`, and
  `DBGINSTRVALID`.

During each inserted wait the oracle freezes architectural state, permits the
single initial output-settling cycle specified by Appendix B, then requires
the complete bus, coprocessor, trace/debug, and internal-state tuple to remain
stable on every further stopped edge and to return to the exact numbered
Chapter 7 phase when CLKEN is restored.

The independent `arm7tdmis_raw_bus_checker` is also bound to every directed
pin-level test. It evaluates legal N/S history, I/S merge, LDC/STC stream
start, control stability, DMORE, CP mirrors, reset, and CLKEN holds on every
clock. Specialized rows below extend the central oracle across PC
destinations, both instruction states, all multiply `m` values, block `n`
values, exceptions, stalls/aborts, and external coprocessor handshakes.

## Required cross-coverage gate

`verification/table7_cross.json` is the reviewed coverage model. It defines
nine required legal-equivalence crosses: base class × endian × stall;
class × condition outcome × privilege mode; register/PC × instruction and
destination state; multiply class × `m`; block class × `n`; coprocessor class
× `b/n`; memory class × endian × alignment; memory class × abort position;
and instruction/exception class × interrupt timing. “Legal-equivalence” is
important: `m` does not apply to loads, for example, and the manifest records
`not-applicable` instead of inventing an illegal Cartesian row.

The full regression runs every owning test first and then invokes the Rust
`arm7-constrained-random table7-cross` command. The gate accepts a row set only
when its named phase passed, the regression's recorded log SHA-256 matches the file,
the exact count-bearing PASS expression occurs in that log, and the owning
testbench SHA-256 is recorded. The resulting
`reports/generated/table7-cross-report.json` has schema
`arm7tdmis-table7-cross-v1` and reports 1,903 evidence rows against a
1,891-row required minimum, all nine required crosses covered, and
zero missing required cross bins. Some directed rows support more than one
orthogonal cross; the aggregate is an evidence-row count, not a claim of
1,903 unique programs.

## Table 7-2 summary cross-check

The implementation uses the Table 7-2 Execute-stage counts and the detailed
pin rows together:

| Category | Table 7-2 count |
|---|---|
| Condition fail; ordinary DP; DP register shift | `S`; `S`; `I+S` |
| DP to r15; register-shift DP to r15 | `N+2S`; `I+N+2S` |
| MUL; MLA; MULL; MLAL | `mI+S`; `I+mI+S`; `mI+I+S`; `I+mI+I+S` |
| B/BL; BX | `N+2S`; `N+2S` |
| LDR non-r15/r15; STR | `N+I+S`; `N+I+N+2S`; `N+N` |
| LDM non-r15/r15; STM | `N+(n-1)S+I+S`; `N+(n-1)S+I+N+2S`; `N+(n-1)S+I+N` |
| SWP; SWI/trap | `N+N+I+S`; `N+2S` |
| CDP; MCR; MRC | `bI+S`; `bI+C+N`; `bI+C+I+S` |
| LDC/STC | `bI+N+(n-1)S+N` |

The apparent STM `+I` bookkeeping token does not create an extra raw pin row:
Table 7-14 overlaps it with the address/data pipeline. This decision and the
same detailed-row-precedence rule for every category are implemented in the
bus-drive state machine and explained in [RAW_BUS.md](RAW_BUS.md).

## Detailed-table evidence

Every cited bench is in `scripts/Makefile`'s `INTEG_TESTS` manifest and
therefore in `make -C scripts regress`. “Directed result” means the named
bench must exit zero; a printed PASS line alone is not evidence.

| Table | Requirement and RTL path | Directed waveform evidence |
|---|---|---|
| 7-3 | B/BL drives target N followed by two S refill phases; BL also commits LR. Direct redirects use `early_flush_fetch`. | `arm7tdmis_table7_core_phase_matrix_tb.sv`, `arm7tdmis_fetch_sequence_tb.sv` |
| 7-4 | Thumb BL prefix is a data operation; the suffix links and performs the N+2S redirect. IRQ/PABT between halves cannot invent hidden state. | `arm7tdmis_thumb_bl_boundary_tb.sv`, `arm7tdmis_sequence_dependencies_tb.sv` |
| 7-5 | BX selects destination state/width from bit 0 and drives N+2S with aligned target addresses. | `arm7tdmis_bx_interwork_tb.sv`, `arm7tdmis_pc_write_alignment_tb.sv` |
| 7-6 | Ordinary DP/MRS/MSR uses S; register-shift uses I+S; r15 destinations add N+2S. | `arm7tdmis_table7_core_phase_matrix_tb.sv`, `arm7tdmis_thumb_reg_shift_bus_tb.sv`, `arm7tdmis_dp_pc_no_spsr_policy_tb.sv`, `arm7tdmis_exception_return_matrix_tb.sv` |
| 7-7 | MUL uses `m` internal phases and one merged successor S. `S_MUL_BUSY` terminates from the multiplier operand. | `arm7tdmis_table7_core_phase_matrix_tb.sv`, `arm7tdmis_multiply_matrix_tb.sv`, `arm7tdmis_thumb_multiply_bus_tb.sv` |
| 7-8 | MLA adds the accumulator I phase before its `m` phases and successor S. | `arm7tdmis_table7_core_phase_matrix_tb.sv`, `arm7tdmis_multiply_matrix_tb.sv` |
| 7-9 | UMULL/SMULL add the high-result I phase after `m`; every signed/unsigned `m` boundary is measured. | `arm7tdmis_table7_core_phase_matrix_tb.sv`, `arm7tdmis_multiply_matrix_tb.sv` |
| 7-10 | UMLAL/SMLAL add accumulator and high-result I phases around `m`; both halves and final flags commit once. | `arm7tdmis_multiply_matrix_tb.sv` |
| 7-11 | Loads drive data N, writeback I, successor S; an r15 destination adds target N+2S. Byte/halfword/signed variants share the phase shape with their SIZE/lane rules. | `arm7tdmis_table7_core_phase_matrix_tb.sv`, `arm7tdmis_single_ls_matrix_tb.sv`, `arm7tdmis_extra_ls_matrix_tb.sv`, `arm7tdmis_ldr_pc_bus_tb.sv` |
| 7-12 | Stores drive data N followed by the next opcode N, with write data delayed one phase and replicated/lane-selected for subwords. | `arm7tdmis_table7_core_phase_matrix_tb.sv`, `arm7tdmis_single_ls_matrix_tb.sv`, `arm7tdmis_extra_ls_matrix_tb.sv` |
| 7-13 | LDM drives first N, `n-1` S data addresses, merged I/S writeback, and target N+2S when r15 is loaded. Abort still completes the transfer count. | `arm7tdmis_table7_core_phase_matrix_tb.sv`, `arm7tdmis_block_ls_matrix_tb.sv`, `arm7tdmis_dmore_matrix_tb.sv`, `arm7tdmis_exception_return_matrix_tb.sv`, `arm7tdmis_ldm_abort_tb.sv` |
| 7-14 | STM drives first N, `n-1` S data addresses, then next-opcode N; data trails each address and requested base writeback survives abort. | `arm7tdmis_table7_core_phase_matrix_tb.sv`, `arm7tdmis_block_ls_matrix_tb.sv`, `arm7tdmis_dmore_matrix_tb.sv`, `arm7tdmis_stm_abort_tb.sv` |
| 7-15 | SWP/SWPB performs locked read N, locked write N, I, and successor S; every exit releases LOCK. | `arm7tdmis_table7_core_phase_matrix_tb.sv`, `arm7tdmis_swp_bus_matrix_tb.sv` |
| 7-16 | SWI/trap and every higher-priority exception use vector N plus two privileged ARM S refill phases from ARM and Thumb sources. | `arm7tdmis_table7_core_phase_matrix_tb.sv`, `arm7tdmis_exception_bus_matrix_tb.sv` |
| 7-17 | CDP supports zero-or-more busy I phases and exact ready completion without ghost commit; absent operations take Table 7-22. | `arm7tdmis_cp_cdp_protocol_tb.sv`, `arm7tdmis_cp_busy_interrupt_tb.sv` |
| 7-18 | LDC accepts after `b` I phases, drives first-word S stream start, `n-1` continuation S phases, and final N/cleanup with response/abort alignment. | `arm7tdmis_cp_ldc_stc_tb.sv`, `arm7tdmis_cp_ldc_stc_abort_tb.sv` |
| 7-19 | STC uses the corresponding variable-length S stream/final N sequence and external coprocessor write data. | `arm7tdmis_cp_ldc_stc_tb.sv`, `arm7tdmis_cp_ldc_stc_abort_tb.sv` |
| 7-20 | MRC supports busy I, accepted C, data I, successor S, r15 flag-only result, and consecutive operations. | `arm7tdmis_cp_reg_transfer_tb.sv`, `arm7tdmis_cp_reg_r15_tb.sv`, `arm7tdmis_cp_erratum15_tb.sv` |
| 7-21 | MCR supports busy I, accepted C, data/next-opcode N, phase-correct WDATA, and r15 PC+12 source. | `arm7tdmis_cp_reg_transfer_tb.sv`, `arm7tdmis_cp_reg_r15_tb.sv` |
| 7-22 | Undefined recognition adds the specified I phase, then enters the Table 7-16 N+2S trap sequence without side effects. | `arm7tdmis_table7_core_phase_matrix_tb.sv`, `arm7tdmis_exception_bus_matrix_tb.sv`, `arm7tdmis_cp15_undef_tb.sv` |
| 7-23 | Every condition-failed ARM class consumes exactly one S opcode cycle, reports not-executed debug status, and has no architectural/external side effect. | `arm7tdmis_cond_fail_matrix_tb.sv` |

## Reproducible focused result

Run the central oracle:

```sh
make -C scripts integ-table7_core_phase_matrix
```

Run all registered directed evidence, including every specialized row above:

```sh
make -C scripts integ
```

The release-facing `make -C scripts regress` starts from a clean build and
records the command status and hashed log for every named bench in
`reports/generated/regression.json`; its full profile runs `table7-cross`
after all directed integration phases. `make -C scripts release-evidence`
rejects a failed, dirty, cross-commit, under-counted, hash-mismatched, or
marker-free cross result before packaging it.
