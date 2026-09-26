//! Checked-in FPGA quality evidence and timing-constraint contracts.

use std::fs;
use std::path::Path;

use regex::Regex;
use serde_json::Value;

use crate::campaign::repo_root;

fn read(relative: &str) -> String {
    fs::read_to_string(repo_root().join(relative))
        .unwrap_or_else(|error| panic!("cannot read {relative}: {error}"))
}

fn commands(path: &Path) -> Vec<String> {
    let source = fs::read_to_string(path).unwrap();
    let mut result = Vec::new();
    let mut current = String::new();
    let mut brace_depth = 0_i64;
    let mut bracket_depth = 0_i64;
    for raw in source.lines() {
        let mut line = raw.split('#').next().unwrap().trim();
        if line.is_empty() {
            continue;
        }
        let continued = line.ends_with('\\');
        if continued {
            line = line.trim_end_matches('\\').trim_end();
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(line);
        brace_depth += line.matches('{').count() as i64 - line.matches('}').count() as i64;
        bracket_depth += line.matches('[').count() as i64 - line.matches(']').count() as i64;
        if !continued && brace_depth == 0 && bracket_depth == 0 {
            result.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        result.push(current);
    }
    result
}

fn assert_matches(text: &str, pattern: &str) {
    assert!(
        Regex::new(pattern).unwrap().is_match(text),
        "missing pattern {pattern:?}"
    );
}

fn assert_no_match(text: &str, pattern: &str) {
    assert!(
        !Regex::new(pattern).unwrap().is_match(text),
        "unexpected pattern {pattern:?}"
    );
}

#[test]
fn fpga_quality_is_mandatory_documented_release_evidence() {
    let makefile = read("scripts/Makefile");
    let regression = read("verification/constrained_random_rs/src/regression.rs");
    let release = read("verification/constrained_random_rs/src/release_evidence/files/fpga.rs");
    let verification = read("docs/VERIFICATION.md");
    let tasks = read("TASKS.md");
    let manifest: Value =
        serde_json::from_str(&read("verification/cdc_rdc_manifest.json")).unwrap();
    for evidence in [
        "lint-independent:",
        "cdc-rdc:",
        "fpga-quality:",
        "independent-lint",
        "cdc-rdc",
    ] {
        assert!(makefile.contains(evidence), "Makefile omits {evidence}");
    }
    for marker in ["\"lint-independent\"", "\"cdc-rdc\""] {
        assert!(regression.contains(marker), "regression omits {marker}");
    }
    for evidence in ["independent-lint.json", "cdc-rdc.json"] {
        assert!(
            release.contains(evidence),
            "release evidence omits {evidence}"
        );
    }
    for evidence in ["arm7tdmis-cdc-rdc-v1", "make -C scripts fpga-quality"] {
        assert!(
            verification.contains(evidence),
            "verification omits {evidence}"
        );
    }
    assert_eq!(manifest["schema"], "arm7tdmis-cdc-rdc-manifest-v1");
    assert!(tasks.contains("- [x] **FPGA-004:**"));
}

#[test]
fn wrapper_uses_quartus_portable_synchronizer_attributes() {
    let wrapper = read("rtl/top/arm7tdmi_mister.sv");
    let attribute = "-name SYNCHRONIZER_IDENTIFICATION FORCED_IF_ASYNCHRONOUS";
    assert_eq!(wrapper.matches(attribute).count(), 12);
    assert!(!wrapper.contains("-name SYNCHRONIZER_IDENTIFICATION FORCED\""));
}

#[test]
fn raw_top_sdc_uses_only_real_clock_and_ports() {
    let source = read("scripts/arm7tdmis.sdc");
    let commands = commands(&repo_root().join("scripts/arm7tdmis.sdc")).join(" ");
    assert!(source.contains("arm7tdmis_top"));
    assert_matches(&commands, r"create_clock\b.*get_ports\s+\{?CLK\}?");
    assert_no_match(&commands, r"\bDBGTCK\b");
    assert_no_match(&commands, r"get_ports[^\]]*\b(?:SE|SI|SO)\b");
}

#[test]
fn raw_synchronous_inputs_are_timed() {
    let false_paths = commands(&repo_root().join("scripts/arm7tdmis.sdc"))
        .into_iter()
        .filter(|command| command.starts_with("set_false_path"))
        .collect::<Vec<_>>()
        .join(" ");
    for port in ["nIRQ", "nFIQ", "DBGTCKEN"] {
        assert_no_match(&false_paths, &format!(r"\b{port}\b"));
    }
}

#[test]
fn fpga_sdc_constrains_real_clock_and_async_boundaries() {
    let commands = commands(&repo_root().join("fpga/arm7tdmi_mister.sdc"));
    let false_paths = commands
        .iter()
        .filter(|command| command.starts_with("set_false_path"))
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(" ");
    let commands = commands.join(" ");
    assert_matches(&commands, r"create_clock\b.*get_ports\s+\{?CLK\}?");
    assert_no_match(&commands, r"\bDBGTCK\b");
    for port in ["RESET_N", "IRQ_ASYNC", "FIQ_ASYNC"] {
        assert_matches(&false_paths, &format!(r"\b{port}\b"));
    }
    for port in [
        "DEBUG_ENABLE_ASYNC",
        "DBGRQ_ASYNC",
        "DBGBREAK_ASYNC",
        "DBGEXT_ASYNC",
    ] {
        assert_no_match(&commands, &format!(r"\b{port}\b"));
    }
    for port in ["CPU_CE", "MEM_READY", "MEM_RDATA", "MEM_ERROR"] {
        assert_no_match(&false_paths, &format!(r"\b{port}\b"));
    }
}
