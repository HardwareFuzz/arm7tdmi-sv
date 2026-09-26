//! Contracts tying published FPGA performance to checked input hashes.

use std::collections::BTreeSet;
use std::fs;
use std::process::Command;

use regex::Regex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::campaign::repo_root;

fn read(relative: &str) -> String {
    fs::read_to_string(repo_root().join(relative))
        .unwrap_or_else(|error| panic!("cannot read {relative}: {error}"))
}

fn comma(value: i64) -> String {
    let digits = value.to_string();
    digits
        .chars()
        .rev()
        .enumerate()
        .flat_map(|(index, digit)| {
            if index > 0 && index % 3 == 0 {
                vec![',', digit]
            } else {
                vec![digit]
            }
        })
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}

fn integer(value: &Value) -> i64 {
    value.as_i64().expect("expected integer")
}

fn decimal(value: &Value) -> f64 {
    value.as_f64().expect("expected decimal")
}

#[test]
fn published_characterization_is_bound_to_hashed_inputs() {
    let baseline: Value =
        serde_json::from_str(&read("verification/fpga_characterization.json")).unwrap();
    assert_eq!(baseline["schema"], "arm7tdmis-fpga-characterization-v2");
    assert_eq!(baseline["status"], "passed");
    assert_eq!(baseline["device"], "5CSEBA6U23I7");
    assert_eq!(baseline["clock_mhz"], 25);
    assert_eq!(
        baseline["clock_scope"],
        "canonical MiSTer wrapper and optional-feature profiles"
    );
    assert_eq!(
        baseline["evidence"],
        json!({
            "commit": "67ff42448d364081435b37647e1623b4265f63e6",
            "commands": [
                "make -C scripts quartus-compile",
                "make -C scripts quartus-conformance-compile",
                "make -C scripts quartus-option-characterization"
            ]
        })
    );

    let root = repo_root();
    let manifest = root.join("fpga/arm7tdmi_mister.f");
    let manifest_sources = fs::read_to_string(&manifest)
        .unwrap()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with(['#', '+']))
        .map(|line| {
            manifest
                .parent()
                .unwrap()
                .join(line)
                .canonicalize()
                .unwrap_or_else(|error| panic!("cannot resolve {line}: {error}"))
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<BTreeSet<_>>();
    let fixed_inputs = [
        "fpga/arm7tdmi_mister.f",
        "fpga/arm7tdmi_mister.qip",
        "fpga/arm7tdmi_mister.qsf",
        "fpga/arm7tdmi_mister.sdc",
        "fpga/arm7tdmis_conformance.qsf",
        "fpga/arm7tdmis_conformance.sdc",
        "fpga/arm7tdmi_option_none.qsf",
        "fpga/arm7tdmi_option_debug.qsf",
        "fpga/arm7tdmi_option_coprocessor.qsf",
        "fpga/arm7tdmi_option_both.qsf",
        "fpga/arm7tdmi_options.sdc",
        "fpga/example/arm7tdmi_mister_example_top.sv",
        // These two Python checkers are historical inputs to the pinned evidence.
        // The live versions have been replaced by Rust and are read from Git.
        "scripts/quartus_option_characterization.py",
        "scripts/quartus_report_check.py",
    ];
    let expected = manifest_sources
        .into_iter()
        .chain(fixed_inputs.into_iter().map(str::to_owned))
        .collect::<BTreeSet<_>>();
    let inputs = baseline["inputs"].as_object().unwrap();
    assert_eq!(inputs.keys().cloned().collect::<BTreeSet<_>>(), expected);
    for (relative, expected_hash) in inputs {
        let contents = if relative == "scripts/quartus_option_characterization.py"
            || relative == "scripts/quartus_report_check.py"
        {
            let revision = format!(
                "{}:{relative}",
                baseline["evidence"]["commit"].as_str().unwrap()
            );
            let output = Command::new("git")
                .arg("show")
                .arg(&revision)
                .current_dir(&root)
                .output()
                .unwrap_or_else(|error| panic!("cannot inspect {revision}: {error}"));
            assert!(output.status.success(), "git show failed for {revision}");
            output.stdout
        } else {
            fs::read(root.join(relative))
                .unwrap_or_else(|error| panic!("cannot read {relative}: {error}"))
        };
        assert_eq!(
            format!("{:x}", Sha256::digest(contents)),
            expected_hash.as_str().unwrap(),
            "input hash mismatch for {relative}"
        );
    }

    let profiles = baseline["profiles"].as_object().unwrap();
    assert_eq!(
        profiles.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "trimmed",
            "conformance",
            "none",
            "debug",
            "coprocessor",
            "both"
        ])
    );
    for (name, target_mhz) in [
        ("trimmed", 25),
        ("conformance", 16),
        ("none", 25),
        ("debug", 25),
        ("coprocessor", 25),
        ("both", 25),
    ] {
        let profile = &profiles[name];
        assert_eq!(profile["target_clock_mhz"], target_mhz, "{name}");
        assert!(
            decimal(&profile["fmax_mhz"]) > decimal(&profile["target_clock_mhz"]),
            "{name}: Fmax below target"
        );
        assert!(
            decimal(&profile["minimum_setup_slack_ns"]) >= 0.0,
            "{name}: setup"
        );
        assert!(
            decimal(&profile["minimum_hold_slack_ns"]) >= 0.0,
            "{name}: hold"
        );
        assert_eq!(profile["resources"]["memory_bit"], 0, "{name}");
        assert_eq!(profile["resources"]["dsp"], 6, "{name}");
        assert!(profile["power_confidence"]
            .as_str()
            .unwrap()
            .contains("Low"));
    }

    let performance = read("docs/PERFORMANCE.md");
    let integration = read("docs/INTEGRATION.md");
    let tasks = read("TASKS.md");
    for (label, document) in [
        ("performance", &performance),
        ("integration", &integration),
        ("tasks", &tasks),
    ] {
        assert!(
            document.contains("verification/fpga_characterization.json"),
            "{label} omits characterization evidence"
        );
    }
    for name in ["trimmed", "conformance"] {
        let profile = &profiles[name];
        for value in [
            comma(integer(&profile["resources"]["alm"])),
            comma(integer(&profile["resources"]["register"])),
            comma(integer(&profile["clock_enable_registers"])),
            format!("{:.2} MHz", decimal(&profile["fmax_mhz"])),
            format!("{:.2} mW", decimal(&profile["power_mw"]["total"])),
        ] {
            assert!(
                performance.contains(&value),
                "{name}: performance omits {value}"
            );
        }
    }
    for name in ["none", "debug", "coprocessor", "both"] {
        let profile = &profiles[name];
        for value in [
            comma(integer(&profile["resources"]["alm"])),
            comma(integer(&profile["resources"]["register"])),
            comma(integer(&profile["clock_enable_registers"])),
            format!("{:.2} MHz", decimal(&profile["fmax_mhz"])),
            format!("{:.2} mW", decimal(&profile["power_mw"]["core_dynamic"])),
        ] {
            assert!(
                performance.contains(&value),
                "{name}: performance omits {value}"
            );
        }
    }
    let conformance = &profiles["conformance"];
    for value in [
        format!("{} MHz", integer(&conformance["target_clock_mhz"])),
        comma(integer(&conformance["resources"]["alm"])),
        comma(integer(&conformance["resources"]["register"])),
        format!("+{:.3} ns", decimal(&conformance["minimum_setup_slack_ns"])),
        format!("+{:.3} ns", decimal(&conformance["minimum_hold_slack_ns"])),
    ] {
        assert!(integration.contains(&value), "integration omits {value}");
    }
    let combined = integration + &tasks;
    for stale in [
        "uses the same clock and input-window assumptions",
        "5,005 ALMs",
        "3,799 registers",
        "+3.546 ns",
        "+0.158 ns",
        "producing clean 180-phase release",
    ] {
        assert!(
            !combined.contains(stale),
            "stale characterization claim: {stale}"
        );
    }
}

#[test]
fn full_flows_run_power_before_fail_hard_report_check() {
    let makefile = read("scripts/Makefile");
    assert!(makefile.contains("QUARTUS_POW ?= quartus_pow"));
    let target_line = Regex::new(r"^[a-zA-Z][^:]*:").unwrap();
    for target in ["quartus-compile", "quartus-conformance-compile"] {
        let mut in_target = false;
        let mut body = String::new();
        for line in makefile.lines() {
            if line.starts_with(&format!("{target}:")) {
                in_target = true;
            } else if in_target && target_line.is_match(line) {
                break;
            }
            if in_target {
                body.push_str(line);
                body.push('\n');
            }
        }
        assert!(!body.is_empty(), "missing Make target {target}");
        let power = body.find("$(QUARTUS_POW)").expect("missing PowerPlay step");
        let check = body
            .find("quartus-report-check")
            .expect("missing report check");
        assert!(
            power < check,
            "{target}: PowerPlay must precede report check"
        );
    }
}

#[test]
fn performance_document_publishes_checked_limits() {
    let performance = read("docs/PERFORMANCE.md");
    for heading in [
        "Clock and CPU enable",
        "Request latency",
        "Resources and clock enables",
        "Power estimate",
        "Reset and endian configuration",
        "Reproducing characterization",
    ] {
        let pattern = Regex::new(&format!(r"(?m)^## .*{}", regex::escape(heading))).unwrap();
        assert!(pattern.is_match(&performance), "missing heading {heading}");
    }
    for evidence in [
        "5CSEBA6U23I7",
        "Quartus Lite 17.0.2",
        "25 MHz",
        "5,000 ALMs",
        "7,500 ALMs",
        "verification/fpga_characterization.json",
        "Low",
        "vectorless",
        "not a board-power estimate",
        "BIG_ENDIAN",
        "MEM_READY",
    ] {
        assert!(
            performance.contains(evidence),
            "performance omits {evidence}"
        );
    }
}

#[test]
fn fpga_005_has_report_and_document_evidence() {
    let tasks = read("TASKS.md");
    assert!(tasks.contains("- [x] **FPGA-005:**"));
    let block = tasks
        .split_once("**FPGA-005:**")
        .unwrap()
        .1
        .split_once("**FPGA-006:**")
        .unwrap()
        .0;
    for evidence in [
        "docs/PERFORMANCE.md",
        "quartus-report-check",
        "PowerPlay",
        "clock-enable",
        "Fmax",
        "characterization_contract.rs",
    ] {
        assert!(block.contains(evidence), "FPGA-005 omits {evidence}");
    }
}
