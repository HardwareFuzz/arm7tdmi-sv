//! File and hash checks for soak and randomized release campaigns.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{checked_artifact, read_json, repo_file};

fn campaign_artifact(root: &Path, group_root: &Path, artifact: &Value) -> Result<PathBuf, String> {
    let relative = artifact["path"]
        .as_str()
        .ok_or("constrained-random artifact is malformed")?;
    let path = group_root
        .join(relative)
        .canonicalize()
        .map_err(|_| "constrained-random artifact hash mismatch")?;
    let group_root = group_root
        .canonicalize()
        .map_err(|_| "constrained-random artifact group is missing")?;
    if !path.starts_with(&group_root) {
        return Err("constrained-random artifact escapes its seed".into());
    }
    if !path.is_file() || !path.starts_with(root) {
        return Err("constrained-random artifact hash mismatch".into());
    }
    let actual = super::super::file_entry(&path, root)?;
    if actual["bytes"] != artifact["bytes"] || actual["sha256"] != artifact["sha256"] {
        return Err("constrained-random artifact hash mismatch".into());
    }
    Ok(path)
}

fn add_soak(
    root: &Path,
    report_root: &Path,
    expected_commit: &str,
    files: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    let report_path = repo_file(
        root,
        &report_root.join("soak-report.json").to_string_lossy(),
    )?;
    let report = read_json(&report_path)?;
    super::super::validate_soak(&report, expected_commit)?;
    for input in report["inputs"]
        .as_object()
        .ok_or("soak report has a malformed input")?
        .values()
    {
        files.insert(checked_artifact(root, input, "soak input")?);
    }
    files.insert(report_path);
    Ok(())
}

fn add_random_events(
    root: &Path,
    report_root: &Path,
    expected_commit: &str,
    files: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    let report_path = repo_file(
        root,
        &report_root
            .join("random-events-report.json")
            .to_string_lossy(),
    )?;
    let report = read_json(&report_path)?;
    super::super::validate_random_events(&report, expected_commit)?;
    let inputs = report["inputs"]
        .as_object()
        .ok_or("random-event input manifest is incomplete")?;
    if inputs.keys().map(String::as_str).collect::<BTreeSet<_>>()
        != BTreeSet::from(["runner", "testbench", "binary"])
    {
        return Err("random-event input manifest is incomplete".into());
    }
    for input in inputs.values() {
        files.insert(checked_artifact(root, input, "random-event input")?);
    }
    for seed in report["seeds"]
        .as_array()
        .ok_or("random-event seeds are missing")?
    {
        let log = &seed["log"];
        if !log.is_object() {
            return Err("random-event seed log is malformed".into());
        }
        let path = checked_artifact(root, log, "random-event seed log")?;
        let actual = super::super::file_entry(&path, root)?;
        if actual["sha256"] != seed["output_sha256"] {
            return Err("random-event seed log hash mismatch".into());
        }
        files.insert(path);
    }
    files.insert(report_path);
    Ok(())
}

fn add_constrained_random(
    root: &Path,
    report_root: &Path,
    expected_commit: &str,
    release_grade: bool,
    files: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    let report_path = repo_file(
        root,
        &report_root
            .join("constrained-random-report.json")
            .to_string_lossy(),
    )?;
    let report = read_json(&report_path)?;
    super::super::validate_constrained_random(&report, expected_commit, release_grade)?;
    for input in report["inputs"]
        .as_object()
        .ok_or("constrained-random report has a malformed input")?
        .values()
    {
        files.insert(checked_artifact(root, input, "constrained-random input")?);
    }

    let artifact_root = report_root.join("constrained_random");
    for seed in report["seeds"]
        .as_array()
        .ok_or("constrained-random seeds are missing")?
    {
        let seed_number = seed["seed"]
            .as_u64()
            .ok_or("constrained-random seed is malformed")?;
        let seed_root = artifact_root.join(format!("seed-{seed_number:08x}"));
        let groups = [
            (
                seed_root.join("differential"),
                &seed["differential"]["artifacts"],
            ),
            (
                seed_root.join("policy_little"),
                &seed["policy_profiles"]["little"]["artifacts"],
            ),
            (
                seed_root.join("policy_big"),
                &seed["policy_profiles"]["big"]["artifacts"],
            ),
        ];
        for (group_root, artifacts) in groups {
            let artifacts = artifacts
                .as_object()
                .filter(|entries| !entries.is_empty())
                .ok_or("constrained-random artifact group is empty")?;
            for artifact in artifacts.values() {
                files.insert(campaign_artifact(root, &group_root, artifact)?);
            }
        }
    }
    files.insert(report_path);
    Ok(())
}

pub(super) fn add_files(
    root: &Path,
    report_root: &Path,
    phases: &BTreeSet<String>,
    expected_commit: &str,
    files: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    if phases.contains("soak") {
        add_soak(root, report_root, expected_commit, files)?;
    }
    if phases.contains("random-events") {
        add_random_events(root, report_root, expected_commit, files)?;
    }
    let full = phases.contains("random-validation");
    let quick = phases.contains("random-validation-quick");
    if full && quick {
        return Err("regression contains ambiguous random phases".into());
    }
    if full || quick {
        add_constrained_random(root, report_root, expected_commit, full, files)?;
    }
    Ok(())
}
