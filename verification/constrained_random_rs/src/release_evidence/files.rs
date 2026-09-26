//! Release file selection and independent hash checks.

mod coverage;
mod fpga;
mod random;

use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Value};

use super::{file_entry, validate_basic, validate_formal};

#[derive(Deserialize)]
struct PrefixRequest {
    regression: Value,
    coverage: Value,
    regression_path: PathBuf,
    coverage_path: PathBuf,
}

fn read_json(path: &Path) -> Result<Value, String> {
    let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("{}: {error}", path.display()))
}

fn repo_file(root: &Path, value: &str) -> Result<PathBuf, String> {
    let candidate = Path::new(value);
    let candidate = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        root.join(candidate)
    };
    let resolved = candidate
        .canonicalize()
        .map_err(|_| format!("evidence file is missing: {value}"))?;
    if !resolved.starts_with(root) {
        return Err(format!("evidence path escapes repository: {value}"));
    }
    if !resolved.is_file() {
        return Err(format!("evidence file is missing: {value}"));
    }
    Ok(resolved)
}

fn checked_artifact(root: &Path, artifact: &Value, label: &str) -> Result<PathBuf, String> {
    let path_text = artifact["path"]
        .as_str()
        .ok_or_else(|| format!("{label} artifact path is missing"))?;
    let path = repo_file(root, path_text)?;
    let actual = file_entry(&path, root)?;
    if actual["bytes"] != artifact["bytes"] || actual["sha256"] != artifact["sha256"] {
        return Err(format!("{label} artifact hash mismatch"));
    }
    Ok(path)
}

fn add_directory_files(
    root: &Path,
    directory: &Path,
    files: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    if !directory.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_dir()
        {
            add_directory_files(root, &entry.path(), files)?;
        } else if entry.path().is_file() {
            files.insert(repo_file(root, &entry.path().to_string_lossy())?);
        }
    }
    Ok(())
}

fn required_phase(phases: &BTreeSet<String>, name: &str, message: &str) -> Result<(), String> {
    if phases.contains(name) {
        Ok(())
    } else {
        Err(message.to_owned())
    }
}

pub(super) fn validate_prefix(
    root: &Path,
    regression: &Value,
    coverage: &Value,
    regression_path: &Path,
    coverage_path: &Path,
) -> Result<BTreeSet<PathBuf>, String> {
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let expected_commit = regression["git"]["commit"]
        .as_str()
        .ok_or("regression report has no commit")?;
    validate_basic(regression, coverage, expected_commit)?;
    let results = regression["results"]
        .as_array()
        .filter(|rows| !rows.is_empty())
        .ok_or("regression report has no phase results")?;
    if regression["result_count"] != results.len() {
        return Err("regression result_count does not match phase results".into());
    }
    let mut files = BTreeSet::from([
        repo_file(&root, &regression_path.to_string_lossy())?,
        repo_file(&root, &coverage_path.to_string_lossy())?,
    ]);
    let mut phase_names = BTreeSet::new();
    for result in results {
        if result["status"] != "passed" || result["exit_code"] != 0 {
            return Err("regression contains a non-passing phase".into());
        }
        let name = result["name"]
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or("regression phase has no name")?;
        phase_names.insert(name.to_owned());
        let path = repo_file(
            &root,
            result["log"]
                .as_str()
                .ok_or("regression phase has no log")?,
        )?;
        let actual = file_entry(&path, &root)?;
        if actual["sha256"] != result["log_sha256"] {
            return Err(format!("regression log hash mismatch: {}", result["log"]));
        }
        files.insert(path);
    }

    let artifacts = coverage["artifacts"]
        .as_object()
        .filter(|entries| !entries.is_empty())
        .ok_or("coverage report has no artifacts")?;
    for (label, artifact) in artifacts {
        let path = repo_file(
            &root,
            artifact["path"]
                .as_str()
                .ok_or("coverage artifact has no path")?,
        )?;
        let actual = file_entry(&path, &root)?;
        if actual["bytes"] != artifact["bytes"] {
            return Err(format!("coverage {label} size mismatch"));
        }
        if actual["sha256"] != artifact["sha256"] {
            return Err(format!("coverage {label} hash mismatch"));
        }
        files.insert(path);
    }
    if coverage["tests"].as_array().is_none_or(Vec::is_empty) {
        return Err("coverage report lists no measured tests".into());
    }
    let coverage_dir = coverage_path
        .parent()
        .ok_or("coverage report has no parent")?;
    add_directory_files(&root, coverage_dir, &mut files)?;

    if regression["mode"] == "full" {
        for (phase, message) in [
            (
                "mister-framework",
                "full regression is missing mandatory MiSTer framework evidence",
            ),
            (
                "table7-cross",
                "full regression is missing mandatory Chapter 7 cross evidence",
            ),
            (
                "random-events",
                "full regression is missing mandatory randomized-event evidence",
            ),
            (
                "functional-coverage",
                "full regression is missing mandatory functional coverage",
            ),
            (
                "formal",
                "full regression is missing mandatory formal evidence",
            ),
        ] {
            required_phase(&phase_names, phase, message)?;
        }
    }

    let report_root = root.join("reports/generated");
    if phase_names.contains("formal") {
        let report_path = repo_file(
            &root,
            &report_root.join("formal-report.json").to_string_lossy(),
        )?;
        let manifest_path = repo_file(
            &root,
            &root
                .join("verification/formal_requirements.json")
                .to_string_lossy(),
        )?;
        let report = read_json(&report_path)?;
        let manifest = read_json(&manifest_path)?;
        validate_formal(&report, &manifest, expected_commit)?;
        for result in report["results"]
            .as_object()
            .ok_or("formal result map is incomplete")?
            .values()
        {
            for field in ["log", "source"] {
                files.insert(checked_artifact(
                    &root,
                    &result[field],
                    &format!("formal {field}"),
                )?);
            }
            if let Some(witnesses) = result["witnesses"].as_array() {
                for witness in witnesses {
                    files.insert(checked_artifact(&root, witness, "formal cover witness")?);
                }
            }
        }
        for field in ["manifest", "sby_file", "runner_log"] {
            files.insert(checked_artifact(
                &root,
                &report[field],
                &format!("formal {field}"),
            )?);
        }
        files.extend([report_path, manifest_path]);
    }

    required_phase(
        &phase_names,
        "trm-coverage",
        "regression did not run mandatory TRM coverage",
    )?;
    let trm_report_path = repo_file(
        &root,
        &report_root
            .join("trm-coverage-report.json")
            .to_string_lossy(),
    )?;
    let trm_manifest_path = repo_file(
        &root,
        &root
            .join("verification/trm_coverage.json")
            .to_string_lossy(),
    )?;
    let trm_report = read_json(&trm_report_path)?;
    crate::trm_coverage::validate_report(
        &trm_report,
        &root,
        &trm_manifest_path,
        Some(expected_commit),
        true,
    )
    .map_err(|error| format!("TRM coverage report validation failed: {error}"))?;
    files.extend([trm_report_path, trm_manifest_path]);

    required_phase(
        &phase_names,
        "ac-timing",
        "regression did not run mandatory AC timing disposition",
    )?;
    let ac_report_path = repo_file(
        &root,
        &report_root.join("ac-timing-report.json").to_string_lossy(),
    )?;
    let ac_manifest_path = repo_file(
        &root,
        &root.join("verification/ac_timing.json").to_string_lossy(),
    )?;
    let ac_report = read_json(&ac_report_path)?;
    crate::ac_timing::validate_report(
        &ac_report,
        &root,
        &ac_manifest_path,
        Some(expected_commit),
        true,
    )
    .map_err(|error| format!("AC timing disposition report validation failed: {error}"))?;
    files.extend([ac_report_path, ac_manifest_path]);

    required_phase(
        &phase_names,
        "traceability",
        "regression did not run mandatory traceability",
    )?;
    let traceability_path = repo_file(
        &root,
        &report_root
            .join("traceability-report.json")
            .to_string_lossy(),
    )?;
    let traceability = read_json(&traceability_path)?;
    if traceability["schema"] != "arm7tdmis-traceability-v1" {
        return Err("traceability report has wrong schema".into());
    }
    if traceability["git"]["dirty"] != false {
        return Err("traceability report describes a dirty source tree".into());
    }
    if traceability["git"]["commit"] != expected_commit {
        return Err("traceability report commit does not match regression".into());
    }
    for field in [
        "unmapped_rtl",
        "unmapped_verification",
        "unknown_requirement_references",
    ] {
        if traceability[field]
            .as_array()
            .is_none_or(|rows| !rows.is_empty())
        {
            return Err("traceability report contains unmapped evidence".into());
        }
    }
    let inputs = traceability["inputs"]
        .as_object()
        .ok_or("traceability report has no inputs")?;
    for input in inputs.values() {
        let path = repo_file(
            &root,
            input["path"]
                .as_str()
                .ok_or("traceability input has no path")?,
        )?;
        let actual = file_entry(&path, &root)?;
        if actual["sha256"] != input["sha256"] {
            return Err(format!(
                "traceability input hash mismatch: {}",
                input["path"]
            ));
        }
    }
    files.insert(traceability_path);

    if phase_names.contains("quartus-option-characterization") {
        files.insert(repo_file(
            &root,
            &report_root.join("quartus-options.json").to_string_lossy(),
        )?);
    }
    for (phase, directory, missing) in [
        (
            "integration-qemu_diff",
            "qemu_diff",
            "QEMU differential metadata is missing",
        ),
        (
            "integration-compiler",
            "compiler",
            "compiler-program metadata is missing",
        ),
    ] {
        if phase_names.contains(phase) {
            let directory = report_root.join(directory);
            if !directory.join("metadata.json").is_file() {
                return Err(missing.into());
            }
            add_directory_files(&root, &directory, &mut files)?;
        }
    }
    random::add_files(
        &root,
        &report_root,
        &phase_names,
        expected_commit,
        &mut files,
    )?;
    coverage::add_files(
        &root,
        &report_root,
        &phase_names,
        expected_commit,
        results,
        &mut files,
    )?;
    fpga::add_files(
        &root,
        &report_root,
        &phase_names,
        expected_commit,
        &mut files,
    )?;
    Ok(files)
}

pub(super) fn prefix_command() -> Result<String, String> {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|error| error.to_string())?;
    let request: PrefixRequest = serde_json::from_str(&input)
        .map_err(|error| format!("invalid release files request: {error}"))?;
    let root = crate::campaign::repo_root();
    let paths = validate_prefix(
        &root,
        &request.regression,
        &request.coverage,
        &request.regression_path,
        &request.coverage_path,
    )?;
    serde_json::to_string(&json!({"files": paths})).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_mandatory_phase_and_mismatched_log_fail_before_optional_reports() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        let report_root = root.join("reports/generated");
        fs::create_dir_all(report_root.join("coverage")).unwrap();
        let log_path = report_root.join("phase.log");
        fs::write(&log_path, b"PASS\n").unwrap();
        let regression_path = report_root.join("regression.json");
        let coverage_path = report_root.join("coverage/coverage.json");
        fs::write(&regression_path, b"{}").unwrap();
        fs::write(&coverage_path, b"{}").unwrap();
        let log_hash = super::super::sha256_bytes(b"PASS\n");
        let mut regression = json!({
            "schema": "arm7tdmis-regression-v1", "status": "passed",
            "git": {"commit": "abc", "dirty": false},
            "mode": "quick", "result_count": 1,
            "results": [{"name": "unit", "status": "passed", "exit_code": 0,
                "log": "reports/generated/phase.log", "log_sha256": log_hash}],
        });
        let coverage = json!({
            "schema": "arm7tdmis-coverage-v1", "git": {"commit": "abc", "dirty": false},
            "artifacts": {"log": {"path": "reports/generated/phase.log", "bytes": 5, "sha256": log_hash}},
            "tests": ["unit"],
        });
        assert!(validate_prefix(
            root,
            &regression,
            &coverage,
            &regression_path,
            &coverage_path
        )
        .unwrap_err()
        .contains("TRM coverage"));
        regression["results"][0]["log_sha256"] = json!("0".repeat(64));
        assert!(validate_prefix(
            root,
            &regression,
            &coverage,
            &regression_path,
            &coverage_path
        )
        .unwrap_err()
        .contains("regression log hash mismatch"));
    }
}
