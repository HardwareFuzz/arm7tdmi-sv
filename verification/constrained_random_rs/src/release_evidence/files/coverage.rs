//! Input, log, and artifact checks for public and coverage release phases.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::Value;

use super::{checked_artifact, read_json, repo_file};

fn checked_inputs(
    root: &Path,
    inputs: &Value,
    expected: &[&str],
    label: &str,
    match_key: bool,
    files: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    let inputs = inputs
        .as_object()
        .ok_or_else(|| format!("{label} input manifest is incomplete"))?;
    if inputs.keys().map(String::as_str).collect::<BTreeSet<_>>()
        != expected.iter().copied().collect()
    {
        return Err(format!("{label} input manifest is incomplete"));
    }
    for (key, entry) in inputs {
        if !entry.is_object() || (match_key && entry["path"] != *key) {
            return Err(format!("{label} input entry is malformed"));
        }
        files.insert(checked_artifact(root, entry, &format!("{label} input"))?);
    }
    Ok(())
}

fn scoped_artifact(
    root: &Path,
    scope: &Path,
    artifact: &Value,
    label: &str,
) -> Result<PathBuf, String> {
    let relative = artifact["path"]
        .as_str()
        .ok_or_else(|| format!("{label} artifact is malformed"))?;
    let path = scope
        .join(relative)
        .canonicalize()
        .map_err(|_| format!("{label} artifact hash mismatch"))?;
    let scope = scope
        .canonicalize()
        .map_err(|_| format!("{label} artifact directory is missing"))?;
    if !path.starts_with(&scope) || !path.starts_with(root) {
        return Err(format!("{label} artifact escapes its report directory"));
    }
    if !path.is_file() {
        return Err(format!("{label} artifact hash mismatch"));
    }
    let actual = super::super::file_entry(&path, root)?;
    if actual["bytes"] != artifact["bytes"] || actual["sha256"] != artifact["sha256"] {
        return Err(format!("{label} artifact hash mismatch"));
    }
    Ok(path)
}

fn marker_in_log(path: &Path, marker: &str) -> Result<bool, String> {
    let pattern =
        Regex::new(marker).map_err(|error| format!("invalid release PASS marker: {error}"))?;
    let contents = fs::read(path).map_err(|error| error.to_string())?;
    Ok(pattern.is_match(&String::from_utf8_lossy(&contents)))
}

fn phase_results(results: &[Value]) -> BTreeMap<&str, &Value> {
    results
        .iter()
        .filter_map(|result| Some((result["name"].as_str()?, result)))
        .collect()
}

fn add_public_suite(
    root: &Path,
    report_root: &Path,
    commit: &str,
    files: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    let report_path = repo_file(
        root,
        &report_root
            .join("public-suite-report.json")
            .to_string_lossy(),
    )?;
    let manifest_path = repo_file(
        root,
        &root
            .join("verification/public_suites.json")
            .to_string_lossy(),
    )?;
    let report = read_json(&report_path)?;
    let manifest = read_json(&manifest_path)?;
    super::super::validate_public_suite(&report, &manifest, commit)?;
    checked_inputs(
        root,
        &report["inputs"],
        &[
            "tb/integration/arm7tdmis_public_suite_tb.sv",
            "verification/constrained_random_rs/src/public_suite.rs",
            "verification/public_suites.json",
        ],
        "public-suite",
        false,
        files,
    )?;
    let artifact_root = report_root.join("public_suite");
    files.insert(scoped_artifact(
        root,
        &artifact_root,
        &report["upstream"]["license_artifact"],
        "public-suite",
    )?);
    for suite in report["suites"]
        .as_object()
        .ok_or("public-suite suites are missing")?
        .values()
    {
        for artifact in suite["artifacts"]
            .as_object()
            .ok_or("public-suite artifact is malformed")?
            .values()
        {
            files.insert(scoped_artifact(
                root,
                &artifact_root,
                artifact,
                "public-suite",
            )?);
        }
    }
    files.extend([report_path, manifest_path]);
    Ok(())
}

fn add_table7(
    root: &Path,
    report_root: &Path,
    commit: &str,
    results: &[Value],
    files: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    let report_path = repo_file(
        root,
        &report_root
            .join("table7-cross-report.json")
            .to_string_lossy(),
    )?;
    let manifest_path = repo_file(
        root,
        &root
            .join("verification/table7_cross.json")
            .to_string_lossy(),
    )?;
    let report = read_json(&report_path)?;
    let manifest = read_json(&manifest_path)?;
    super::super::validate_table7(&report, &manifest, commit)?;
    checked_inputs(
        root,
        &report["inputs"],
        &[
            "verification/table7_cross.json",
            "verification/constrained_random_rs/Cargo.toml",
            "verification/constrained_random_rs/Cargo.lock",
            "verification/constrained_random_rs/src/main.rs",
            "verification/constrained_random_rs/src/campaign.rs",
            "verification/constrained_random_rs/src/table7.rs",
        ],
        "Chapter 7 cross",
        true,
        files,
    )?;
    let result_by_name = phase_results(results);
    for cross in report["crosses"]
        .as_object()
        .ok_or("Chapter 7 report crosses are missing")?
        .values()
    {
        for evidence in cross["evidence"]
            .as_array()
            .ok_or("Chapter 7 evidence is missing")?
        {
            let phase = evidence["phase"]
                .as_str()
                .ok_or("Chapter 7 evidence has no phase")?;
            let result = result_by_name
                .get(phase)
                .ok_or_else(|| format!("Chapter 7 phase evidence mismatch: {phase}"))?;
            let log = &evidence["log"];
            let source = &evidence["source_artifact"];
            if result["status"] != "passed"
                || result["exit_code"] != 0
                || result["log"] != log["path"]
                || result["log_sha256"] != log["sha256"]
            {
                return Err(format!("Chapter 7 phase evidence mismatch: {phase}"));
            }
            let log_path = checked_artifact(root, log, "Chapter 7 log")?;
            let source_path = checked_artifact(root, source, "Chapter 7 source")?;
            if !marker_in_log(
                &log_path,
                evidence["marker"]
                    .as_str()
                    .ok_or("Chapter 7 marker is missing")?,
            )? {
                return Err(format!("Chapter 7 PASS marker missing: {phase}"));
            }
            files.extend([log_path, source_path]);
        }
    }
    files.extend([report_path, manifest_path]);
    Ok(())
}

fn add_functional(
    root: &Path,
    report_root: &Path,
    commit: &str,
    results: &[Value],
    files: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    let report_path = repo_file(
        root,
        &report_root
            .join("functional-coverage-report.json")
            .to_string_lossy(),
    )?;
    let manifest_path = repo_file(
        root,
        &root
            .join("verification/functional_coverage.json")
            .to_string_lossy(),
    )?;
    let report = read_json(&report_path)?;
    let manifest = read_json(&manifest_path)?;
    super::super::functional::validate(&report, &manifest, commit)?;
    checked_inputs(
        root,
        &report["inputs"],
        &[
            "verification/functional_coverage.json",
            "verification/constrained_random_rs/src/functional_coverage.rs",
        ],
        "functional-coverage",
        true,
        files,
    )?;
    let result_by_name = phase_results(results);
    for (phase, evidence) in report["evidence"]
        .as_object()
        .ok_or("functional-coverage evidence is missing")?
    {
        let result = result_by_name
            .get(phase.as_str())
            .ok_or_else(|| format!("functional-coverage phase mismatch: {phase}"))?;
        let log = &evidence["log"];
        let source = &evidence["source"];
        if result["status"] != "passed"
            || result["exit_code"] != 0
            || result["log"] != log["path"]
            || result["log_sha256"] != log["sha256"]
        {
            return Err(format!("functional-coverage phase mismatch: {phase}"));
        }
        let log_path = checked_artifact(root, log, "functional-coverage log")?;
        let source_path = checked_artifact(root, source, "functional-coverage source")?;
        if !marker_in_log(
            &log_path,
            evidence["marker"]
                .as_str()
                .ok_or("functional-coverage marker is missing")?,
        )? {
            return Err(format!("functional-coverage evidence mismatch: {phase}"));
        }
        files.extend([log_path, source_path]);
    }
    files.extend([report_path, manifest_path]);
    Ok(())
}

pub(super) fn add_files(
    root: &Path,
    report_root: &Path,
    phases: &BTreeSet<String>,
    commit: &str,
    results: &[Value],
    files: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    if phases.contains("public-suite") {
        add_public_suite(root, report_root, commit, files)?;
    }
    if phases.contains("table7-cross") {
        add_table7(root, report_root, commit, results, files)?;
    }
    if phases.contains("functional-coverage") {
        add_functional(root, report_root, commit, results, files)?;
    }
    Ok(())
}
