//! Fail-first contract for VAL-004 Chapter 7 cross-coverage closure.

use std::collections::BTreeSet;
use std::fs;

use regex::Regex;
use serde_json::Value;

use crate::campaign::repo_root;

fn read(relative: &str) -> String {
    fs::read_to_string(repo_root().join(relative))
        .unwrap_or_else(|error| panic!("cannot read {relative}: {error}"))
}

#[test]
fn manifest_defines_every_required_legal_cross() {
    let path = repo_root().join("verification/table7_cross.json");
    assert!(path.is_file(), "missing {}", path.display());
    let manifest: Value = serde_json::from_str(&read("verification/table7_cross.json")).unwrap();
    assert_eq!(manifest["schema"], "arm7tdmis-table7-cross-map-v1");
    let required = [
        "class_waveform_endian_stall",
        "class_condition_mode",
        "register_pc_state",
        "multiply_class_m",
        "block_class_n",
        "coprocessor_class_b_n",
        "memory_class_endian_alignment",
        "memory_class_abort",
        "class_interrupt_exception",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<BTreeSet<_>>();
    let listed = manifest["required_crosses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(listed, required);
    let crosses = manifest["crosses"].as_object().unwrap();
    assert_eq!(crosses.keys().cloned().collect::<BTreeSet<_>>(), required);
    let phase = Regex::new(r"^integration-[a-z0-9_]+$").unwrap();
    for (name, cross) in crosses {
        assert!(
            cross["minimum_rows"].as_u64().is_some_and(|rows| rows > 0),
            "{name}: minimum_rows"
        );
        assert!(
            cross["dimensions"]
                .as_object()
                .is_some_and(|items| !items.is_empty()),
            "{name}: dimensions"
        );
        let evidence = cross["evidence"].as_array().unwrap();
        assert!(!evidence.is_empty(), "{name}: evidence");
        for row in evidence {
            let phase_name = row["phase"].as_str().unwrap();
            assert!(
                phase.is_match(phase_name),
                "{name}: invalid phase {phase_name}"
            );
            assert!(
                row["marker"].as_str().unwrap().contains("PASS"),
                "{name}: marker"
            );
            let source = row["source"].as_str().unwrap();
            assert!(
                repo_root().join(source).is_file(),
                "{name}: missing {source}"
            );
        }
    }
}

#[test]
fn runner_is_fail_hard_same_commit_and_hash_validated() {
    let runner = read("verification/constrained_random_rs/src/table7.rs");
    for evidence in [
        "const SCHEMA: &str = \"arm7tdmis-table7-cross-v1\"",
        "arm7tdmis-regression-v1",
        "arm7tdmis-table7-cross-map-v1",
        "required_crosses",
        "minimum_rows",
        "log_sha256",
        "source_sha256",
        "expected full regression",
        "[table7-cross] PASS",
    ] {
        assert!(runner.contains(evidence), "table7 runner omits {evidence}");
    }
}

#[test]
fn cross_gate_is_full_regression_release_evidence() {
    let makefile = read("scripts/Makefile");
    let regression = read("verification/constrained_random_rs/src/regression.rs");
    let release_files =
        read("verification/constrained_random_rs/src/release_evidence/files/coverage.rs");
    let release_validator = read("verification/constrained_random_rs/src/release_evidence.rs");
    let tasks = read("TASKS.md");
    let verification = read("docs/VERIFICATION.md");
    let table7 = read("docs/TABLE7_MATRIX.md");
    for evidence in [
        "table7-cross:",
        "$(abspath $(RANDOM_RUST_BIN)) table7-cross",
        "table7-cross-report.json",
    ] {
        assert!(makefile.contains(evidence), "Makefile omits {evidence}");
    }
    for (label, source, evidence) in [
        ("regression", &regression, "\"table7-cross\""),
        ("regression", &regression, "if !args.quick"),
        (
            "release validator",
            &release_validator,
            "arm7tdmis-table7-cross-v1",
        ),
        ("release files", &release_files, "table7-cross-report.json"),
        ("tasks", &tasks, "- [x] **VAL-004:**"),
        ("verification", &verification, "Chapter 7 cross coverage"),
        ("Table 7 docs", &table7, "zero missing required cross bins"),
    ] {
        assert!(source.contains(evidence), "{label} omits {evidence}");
    }
}
