//! VAL-005 external-event campaign. Each seed must independently exercise the
//! complete instruction, stall, and legal external-event masks.

use std::collections::BTreeSet;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use regex::Regex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use wait_timeout::ChildExt;

use crate::campaign::{git_text, repo_root, sha256, utc_now, write_json, Failure};

const SCHEMA: &str = "arm7tdmis-random-events-v1";
const MINIMUM_DECISIONS: u64 = 256;
const CLASS_BINS: [&str; 16] = [
    "undef",
    "dp",
    "msr",
    "mrs",
    "mul",
    "mull",
    "branch",
    "bx",
    "ldr_str",
    "ldrh_strh",
    "ldm_stm",
    "swp",
    "swi",
    "cdp",
    "mcr_mrc",
    "ldc_stc",
];
const EVENT_BINS: [&str; 10] = [
    "abort_opcode_read",
    "abort_data_read",
    "abort_data_write",
    "irq",
    "fiq",
    "reset",
    "dbgrq",
    "cp_ready",
    "cp_busy",
    "cp_absent",
];

#[derive(Debug, PartialEq, Eq)]
struct PassMarker {
    seed: u32,
    class_mask: u16,
    class_stall_mask: u16,
    event_mask: u16,
    decisions: u64,
}

fn parse_pass_marker(output: &str, expected_seed: u32) -> Result<PassMarker, String> {
    let pattern = Regex::new(
        r"(?m)^\[random_events\] PASS seed=([0-9]+) classes=([0-9a-f]{4}) stalls=([0-9a-f]{4}) events=([0-9a-f]{4}) decisions=([0-9]+)$",
    )
    .unwrap();
    let mut matches = pattern.captures_iter(output);
    let capture = matches
        .next()
        .ok_or("random-event output lacks one exact PASS marker")?;
    if matches.next().is_some() {
        return Err("random-event output lacks one exact PASS marker".to_owned());
    }
    let marker = PassMarker {
        seed: capture[1]
            .parse()
            .map_err(|_| "random-event PASS marker has an invalid seed")?,
        class_mask: u16::from_str_radix(&capture[2], 16).unwrap(),
        class_stall_mask: u16::from_str_radix(&capture[3], 16).unwrap(),
        event_mask: u16::from_str_radix(&capture[4], 16).unwrap(),
        decisions: capture[5]
            .parse()
            .map_err(|_| "random-event PASS marker has an invalid decision count")?,
    };
    if marker.seed != expected_seed {
        return Err("random-event PASS marker has the wrong seed".to_owned());
    }
    if marker.class_mask != u16::MAX {
        return Err("random-event PASS marker lacks an instruction class".to_owned());
    }
    if marker.class_stall_mask != u16::MAX {
        return Err("random-event PASS marker lacks a class stall".to_owned());
    }
    if marker.event_mask != 0x03ff {
        return Err("random-event PASS marker lacks an external event".to_owned());
    }
    if marker.decisions < MINIMUM_DECISIONS {
        return Err("random-event PASS marker is below the decision floor".to_owned());
    }
    Ok(marker)
}

fn seeds(base_seed: i64, count: usize) -> Vec<u32> {
    let mut state = (base_seed as u32) & 0x7fff_ffff;
    if state == 0 {
        state = 1;
    }
    let mut result = Vec::with_capacity(count);
    let mut seen = BTreeSet::new();
    while result.len() < count {
        if state != 0 && seen.insert(state) {
            result.push(state);
        }
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223) & 0x7fff_ffff;
    }
    result
}

fn absolute(path: PathBuf) -> Result<PathBuf, Failure> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn option_or(arguments: &[String], name: &str, default: &str) -> Result<String, Failure> {
    if arguments.iter().any(|argument| argument == name) {
        crate::option(arguments, name).map_err(Failure::other)
    } else {
        Ok(default.to_owned())
    }
}

fn file_entry(path: &Path) -> Result<Value, Failure> {
    let path = fs::canonicalize(path)?;
    let display = path.strip_prefix(repo_root()).unwrap_or(&path);
    Ok(json!({
        "path": display.to_string_lossy(),
        "bytes": fs::metadata(&path)?.len(),
        "sha256": sha256(&path)?,
    }))
}

fn bins(mask: u16, names: &[&str]) -> Vec<String> {
    names
        .iter()
        .enumerate()
        .filter(|(index, _)| mask & (1 << index) != 0)
        .map(|(_, name)| (*name).to_owned())
        .collect()
}

fn execute(binary: &Path, seed: u32, timeout: Duration) -> Result<(Value, String), Failure> {
    let command = vec![
        binary.to_string_lossy().into_owned(),
        format!("+SEED={seed}"),
        format!("+verilator+seed+{seed}"),
    ];
    let scratch = tempfile::tempfile()?;
    let stdout = scratch.try_clone()?;
    let stderr = scratch.try_clone()?;
    let start = Instant::now();
    let mut child = Command::new(binary)
        .args(&command[1..])
        .current_dir(repo_root())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()?;
    let status = child.wait_timeout(timeout)?;
    let timed_out = status.is_none();
    if timed_out {
        child.kill()?;
        child.wait()?;
    }
    let exit_code = status.and_then(|status| status.code());
    let mut scratch = scratch;
    scratch.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    scratch.read_to_end(&mut bytes)?;
    let mut output = String::from_utf8_lossy(&bytes).into_owned();
    if timed_out {
        output.push_str(&format!(
            "\n[random_events] TIMEOUT after {:.3} seconds",
            timeout.as_secs_f64()
        ));
    }
    let (marker, marker_error) = if !timed_out && exit_code == Some(0) {
        match parse_pass_marker(&output, seed) {
            Ok(marker) => (Some(marker), None),
            Err(error) => (None, Some(error)),
        }
    } else {
        (None, None)
    };
    let passed = marker.is_some();
    let mut result = json!({
        "seed": seed,
        "status": if passed { "passed" } else { "failed" },
        "command": command,
        "reproducer": command,
        "duration_seconds": (start.elapsed().as_secs_f64() * 1_000_000.0).round() / 1_000_000.0,
        "exit_code": exit_code,
        "timed_out": timed_out,
        "pass_marker_found": passed,
        "marker_error": marker_error,
        "output_sha256": format!("{:x}", Sha256::digest(output.as_bytes())),
    });
    if let Some(marker) = marker {
        result["decision_count"] = json!(marker.decisions);
        result["class_bins"] = json!(bins(marker.class_mask, &CLASS_BINS));
        result["class_stall_bins"] = json!(bins(marker.class_stall_mask, &CLASS_BINS));
        result["event_bins"] = json!(bins(marker.event_mask, &EVENT_BINS));
    }
    Ok((result, output))
}

pub fn random_events(arguments: &[String]) -> Result<(), String> {
    run(arguments).map_err(|error| error.to_string())
}

fn run(arguments: &[String]) -> Result<(), Failure> {
    let binary = fs::canonicalize(crate::option(arguments, "--binary").map_err(Failure::other)?)?;
    if !binary.is_file() {
        return Err(Failure::other("simulator binary is missing"));
    }
    let seed_count: usize = option_or(arguments, "--seeds", "32")?
        .parse()
        .map_err(|_| Failure::other("--seeds must be positive"))?;
    if seed_count == 0 {
        return Err(Failure::other("--seeds must be positive"));
    }
    let base_seed: i64 = option_or(arguments, "--base-seed", "1")?
        .parse()
        .map_err(|_| Failure::other("invalid --base-seed"))?;
    let timeout_seconds: f64 = option_or(arguments, "--timeout", "15")?
        .parse()
        .map_err(|_| Failure::other("invalid --timeout"))?;
    if !timeout_seconds.is_finite() || timeout_seconds <= 0.0 {
        return Err(Failure::other("--timeout must be positive and finite"));
    }
    let timeout = Duration::from_secs_f64(timeout_seconds);
    let root = repo_root();
    let output = absolute(PathBuf::from(option_or(
        arguments,
        "--output",
        &root
            .join("reports/generated/random-events-report.json")
            .to_string_lossy(),
    )?))?;
    let log_directory = absolute(PathBuf::from(option_or(
        arguments,
        "--log-directory",
        &root
            .join("reports/generated/random_events")
            .to_string_lossy(),
    )?))?;
    fs::create_dir_all(&log_directory)?;
    let runner = root.join("verification/constrained_random_rs/src/random_events.rs");
    let testbench = root.join("tb/integration/arm7tdmis_random_event_matrix_tb.sv");
    let commit = git_text(&["rev-parse", "HEAD"])?;
    let dirty = !git_text(&["status", "--porcelain"])?.is_empty();
    let seed_values = seeds(base_seed, seed_count);
    let mut results = Vec::new();
    let mut failures = Vec::new();

    for (index, seed) in seed_values.iter().enumerate() {
        println!("[random-events] seed {}/{}: {seed}", index + 1, seed_count);
        let (mut result, simulation_output) = execute(&binary, *seed, timeout)?;
        let log_path = log_directory.join(format!("seed-{seed}.log"));
        fs::write(&log_path, &simulation_output)?;
        result["log"] = file_entry(&log_path)?;
        let passed = result["status"] == "passed";
        results.push(result);
        if !passed {
            failures.push(*seed);
            eprintln!("{simulation_output}");
            break;
        }
    }

    let passed_results: Vec<_> = results
        .iter()
        .filter(|result| result["status"] == "passed")
        .collect();
    let covered = |key: &str| -> Vec<String> {
        let mut values = BTreeSet::new();
        for result in &passed_results {
            if let Some(names) = result[key].as_array() {
                for name in names {
                    if let Some(name) = name.as_str() {
                        values.insert(name.to_owned());
                    }
                }
            }
        }
        values.into_iter().collect()
    };
    let class_bins = covered("class_bins");
    let stall_bins = covered("class_stall_bins");
    let event_bins = covered("event_bins");
    let total_decisions: u64 = passed_results
        .iter()
        .filter_map(|result| result["decision_count"].as_u64())
        .sum();
    let expected_class: BTreeSet<_> = CLASS_BINS.iter().map(|name| (*name).to_owned()).collect();
    let expected_event: BTreeSet<_> = EVENT_BINS.iter().map(|name| (*name).to_owned()).collect();
    let status = if failures.is_empty()
        && results.len() == seed_count
        && class_bins.iter().cloned().collect::<BTreeSet<_>>() == expected_class
        && stall_bins.iter().cloned().collect::<BTreeSet<_>>() == expected_class
        && event_bins.iter().cloned().collect::<BTreeSet<_>>() == expected_event
    {
        "passed"
    } else {
        "failed"
    };
    let report = json!({
        "schema": SCHEMA,
        "created_utc": utc_now(),
        "status": status,
        "git": {"commit": commit, "dirty": dirty},
        "configuration": {
            "seed_count": seed_count,
            "base_seed": base_seed,
            "timeout_seconds": timeout_seconds,
            "minimum_decisions_per_seed": MINIMUM_DECISIONS,
            "event_constraint": "ABORT only on active responses; CPA/CPB only 00, 01, or 11",
        },
        "required_class_bins": CLASS_BINS,
        "required_event_bins": EVENT_BINS,
        "covered_class_bins": class_bins,
        "covered_class_stall_bins": stall_bins,
        "covered_event_bins": event_bins,
        "completed_seed_count": passed_results.len(),
        "total_decision_count": total_decisions,
        "inputs": {
            "runner": file_entry(&runner)?,
            "testbench": file_entry(&testbench)?,
            "binary": file_entry(&binary)?,
        },
        "seeds": results,
        "failures": failures,
    });
    write_json(&output, &report)?;
    if status == "passed" {
        println!("[random-events] PASS ({seed_count} seeds, {total_decisions} random decisions)");
        Ok(())
    } else {
        eprintln!(
            "[random-events] FAIL ({}/{seed_count} seeds)",
            passed_results.len()
        );
        Err(Failure::other("random-event campaign failed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_event_workload_and_release_wiring_remain_mandatory() {
        let root = repo_root();
        let read = |path: &str| fs::read_to_string(root.join(path)).unwrap();
        let testbench = read("tb/integration/arm7tdmis_random_event_matrix_tb.sv");
        for marker in [
            "$value$plusargs(\"SEED=%d\"",
            "INSTR_CLASS_COUNT = 16",
            "REQUIRED_CLASS_MASK",
            "REQUIRED_EVENT_MASK",
            "class_stall_mask",
            "lfsr",
            "CLKEN",
            "inject_abort",
            "u_mem.is_active_q",
            "nIRQ",
            "nFIQ",
            "nRESET",
            "DBGRQ",
            "CP_PRESENT_READY",
            "CP_PRESENT_BUSY",
            "CP_ABSENT",
            "ABORT && !u_mem.is_active_q",
            "[random_events] PASS",
            "$fatal",
        ] {
            assert!(
                testbench.contains(marker),
                "missing event workload marker: {marker}"
            );
        }
        assert!(!testbench.contains("CPA = 1'b1;\n        CPB = 1'b0;"));

        let makefile = read("scripts/Makefile");
        for marker in [
            "RANDOM_EVENT_SEEDS ?= 32",
            "random-events-build:",
            "random-events:",
            "$(RANDOM_RUST_BIN)) random-events",
            "arm7tdmis_random_event_matrix_tb",
        ] {
            assert!(
                makefile.contains(marker),
                "missing random-event build marker: {marker}"
            );
        }
        let regression = read("verification/constrained_random_rs/src/regression.rs");
        assert!(regression.contains("\"random-events\""));
        assert!(regression.contains("if !args.quick"));
        assert!(
            read("verification/constrained_random_rs/src/release_evidence/files/random.rs")
                .contains("random-events-report.json")
        );
        assert!(
            read(".github/workflows/verification.yml").contains("make -C scripts random-events")
        );
        assert!(read("verification/traceability.json").contains("VAL-005"));
        let tasks = read("TASKS.md");
        let block = tasks
            .split("**VAL-005:**")
            .nth(1)
            .unwrap()
            .split("**VAL-006:**")
            .next()
            .unwrap();
        for marker in ["32", "16", "ABORT", "IRQ", "FIQ", "DBGRQ"] {
            assert!(block.contains(marker), "missing VAL-005 marker: {marker}");
        }
        assert!(read("docs/VERIFICATION.md").contains("arm7tdmis-random-events-v1"));
    }

    #[test]
    fn marker_rejects_every_weakened_mask_and_ambiguous_success() {
        let valid =
            "[random_events] PASS seed=17 classes=ffff stalls=ffff events=03ff decisions=512\n";
        assert_eq!(parse_pass_marker(valid, 17).unwrap().decisions, 512);
        assert!(parse_pass_marker(&(valid.to_owned() + valid), 17).is_err());
        assert!(parse_pass_marker(valid, 18).is_err());
        for changed in [
            "classes=fffe",
            "stalls=7fff",
            "events=01ff",
            "decisions=255",
        ] {
            let original = match changed.split_once('=').unwrap().0 {
                "classes" => "classes=ffff",
                "stalls" => "stalls=ffff",
                "events" => "events=03ff",
                _ => "decisions=512",
            };
            assert!(parse_pass_marker(&valid.replace(original, changed), 17).is_err());
        }
    }

    #[test]
    fn seed_sequence_is_stable_nonzero_and_unique() {
        assert_eq!(&seeds(1, 4), &[1, 1_015_568_748, 1_586_005_467, 18_219_390]);
        let generated = seeds(0, 32);
        assert_eq!(generated.len(), 32);
        assert_eq!(generated.iter().copied().collect::<BTreeSet<_>>().len(), 32);
    }
}
