//! Fail-hard completeness checks for the ARM7TDMI-S Chapter 7 matrix.

use std::collections::BTreeSet;
use std::fs;

use regex::Regex;

use crate::campaign::repo_root;

fn read(relative: &str) -> String {
    fs::read_to_string(repo_root().join(relative))
        .unwrap_or_else(|error| panic!("cannot read {relative}: {error}"))
}

#[test]
fn every_detailed_table_has_exactly_one_evidence_row() {
    let path = repo_root().join("docs/TABLE7_MATRIX.md");
    assert!(path.is_file(), "missing {}", path.display());
    let matrix = read("docs/TABLE7_MATRIX.md");
    assert!(matrix.contains("Table 7-2"));
    for table in 3..24 {
        let marker = format!("| 7-{table} |");
        assert_eq!(
            matrix.matches(&marker).count(),
            1,
            "expected one evidence row for Table 7-{table}"
        );
    }
}

#[test]
fn every_cited_bench_exists_and_is_registered() {
    assert!(repo_root().join("docs/TABLE7_MATRIX.md").is_file());
    let matrix = read("docs/TABLE7_MATRIX.md");
    let makefile = read("scripts/Makefile");
    let citation = Regex::new(r"`(arm7tdmis_[a-z0-9_]+_tb\.sv)`").unwrap();
    let benches = citation
        .captures_iter(&matrix)
        .map(|capture| capture[1].to_owned())
        .collect::<BTreeSet<_>>();
    assert!(benches.len() >= 12, "only {} cited benches", benches.len());
    for filename in benches {
        let name = filename
            .strip_prefix("arm7tdmis_")
            .unwrap()
            .strip_suffix("_tb.sv")
            .unwrap();
        let path = repo_root().join("tb/integration").join(&filename);
        assert!(path.is_file(), "missing cited bench: {}", path.display());
        let pattern = Regex::new(&format!(r"\b{}\b", regex::escape(name))).unwrap();
        assert!(
            pattern.is_match(&makefile),
            "{name} is not in integration manifest"
        );
    }
}

#[test]
fn core_oracle_scores_complete_bus_and_state_contract() {
    let oracle = read("tb/integration/arm7tdmis_table7_core_phase_matrix_tb.sv");
    for observation in [
        "ADDR",
        "WRITE",
        "SIZE",
        "PROT",
        "LOCK",
        "TRANS",
        "WDATA",
        "RDATA",
        "ABORT",
        "DMORE",
        "CPnMREQ",
        "CPSEQ",
        "CPnTRANS",
        "CPnOPC",
        "CPTBIT",
        "CPnI",
        "CLKEN",
        "pc_q",
        "cpsr",
        "state_q",
        "de_q.instr",
        "any_exc_fires",
        "DBGACK",
        "DBGnEXEC",
        "DBGINSTRVALID",
    ] {
        assert!(
            oracle.contains(observation),
            "core oracle omits {observation}"
        );
    }
}

#[test]
fn core_oracle_crosses_endian_and_waits_at_every_phase() {
    let oracle = read("tb/integration/arm7tdmis_table7_core_phase_matrix_tb.sv");
    for evidence in [
        "ENDIAN_COUNT = 2",
        "STALL_PROFILE_COUNT = 2",
        ".CFGBIGEND(CFGBIGEND)",
        "stall_cycles",
        "check_stalled_phase",
        "endian x %0d stall profiles",
    ] {
        assert!(oracle.contains(evidence), "core oracle omits {evidence}");
    }
    assert!(!oracle.contains(".CFGBIGEND(1'b0)"));
}

#[test]
fn bus_closure_is_tied_to_executable_matrix() {
    let tasks = read("TASKS.md");
    for evidence in [
        "- [x] **BUS-002:**",
        "- [x] **BUS-003:**",
        "TABLE7_MATRIX.md",
        "table7_core_phase_matrix",
    ] {
        assert!(tasks.contains(evidence), "tasks omit {evidence}");
    }
}
