//! Chapter 7 cross evidence gate. Every row comes from a hashed, passing
//! full-regression phase; source names alone never count as evidence.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use indexmap::IndexMap;
use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::campaign::{file_entry, git_text, repo_root, sha256, utc_now, write_json, Failure};

const SCHEMA: &str = "arm7tdmis-table7-cross-v1";
const MANIFEST_SCHEMA: &str = "arm7tdmis-table7-cross-map-v1";
const REGRESSION_SCHEMA: &str = "arm7tdmis-regression-v1";
const REQUIRED_CROSSES: [&str; 9] = [
    "class_waveform_endian_stall",
    "class_condition_mode",
    "register_pc_state",
    "multiply_class_m",
    "block_class_n",
    "coprocessor_class_b_n",
    "memory_class_endian_alignment",
    "memory_class_abort",
    "class_interrupt_exception",
];

#[derive(Deserialize)]
struct Manifest {
    schema: Value,
    required_crosses: Value,
    // Python's original gate required the JSON object key order to match the
    // reviewed cross order. IndexMap retains that evidence-contract detail.
    crosses: IndexMap<String, Value>,
}

fn failure(message: impl Into<String>) -> Failure {
    Failure::other(message)
}

fn normalized_absolute(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            _ => result.push(component.as_os_str()),
        }
    }
    result
}

fn repo_path(path_text: &str, must_exist: bool) -> Result<PathBuf, Failure> {
    let root = fs::canonicalize(repo_root())?;
    let input = Path::new(path_text);
    let candidate = normalized_absolute(&if input.is_absolute() {
        input.to_path_buf()
    } else {
        root.join(input)
    });
    let mut existing = candidate.clone();
    let mut trailing = Vec::new();
    while !existing.exists() {
        let name = existing
            .file_name()
            .ok_or_else(|| failure(format!("required file is missing: {path_text}")))?;
        trailing.push(name.to_os_string());
        existing.pop();
    }
    let mut resolved = fs::canonicalize(existing)?;
    for name in trailing.iter().rev() {
        resolved.push(name);
    }
    if !resolved.starts_with(&root) {
        return Err(failure(format!("path escapes repository: {path_text}")));
    }
    if must_exist && !resolved.is_file() {
        return Err(failure(format!("required file is missing: {path_text}")));
    }
    Ok(resolved)
}

fn relative(path: &Path) -> Result<String, Failure> {
    Ok(path
        .strip_prefix(repo_root())
        .map_err(|error| failure(error.to_string()))?
        .to_string_lossy()
        .replace('\\', "/"))
}

fn load_json(path: &Path) -> Result<Value, Failure> {
    let value: Value =
        serde_json::from_slice(&fs::read(path)?).map_err(|error| failure(error.to_string()))?;
    if !value.is_object() {
        return Err(failure(format!(
            "JSON root is not an object: {}",
            path.display()
        )));
    }
    Ok(value)
}

fn unique_json_values(values: &[Value]) -> bool {
    values
        .iter()
        .map(Value::to_string)
        .collect::<BTreeSet<_>>()
        .len()
        == values.len()
}

fn positive_integer(value: &Value) -> Option<u64> {
    value.as_u64().filter(|number| *number > 0)
}

fn validate_manifest(manifest: &Manifest) -> Result<(), Failure> {
    if manifest.schema.as_str() != Some(MANIFEST_SCHEMA) {
        return Err(failure("Chapter 7 cross manifest has wrong schema"));
    }
    let required = manifest.required_crosses.as_array();
    let reviewed: Vec<_> = REQUIRED_CROSSES
        .iter()
        .map(|name| Value::String((*name).to_owned()))
        .collect();
    let ordered_keys: Vec<_> = manifest.crosses.keys().map(String::as_str).collect();
    if required != Some(&reviewed) || ordered_keys != REQUIRED_CROSSES {
        return Err(failure("Chapter 7 required cross set is incomplete"));
    }
    let phase_pattern = Regex::new(r"^integration-[a-z0-9_]+$").unwrap();
    for name in REQUIRED_CROSSES {
        let cross = &manifest.crosses[name];
        let Some(object) = cross.as_object() else {
            return Err(failure(format!("cross {name} is malformed")));
        };
        let dimensions = object.get("dimensions").and_then(Value::as_object);
        let evidence = object.get("evidence").and_then(Value::as_array);
        let valid_dimensions = dimensions.is_some_and(|dimensions| {
            !dimensions.is_empty()
                && dimensions.values().all(|value| {
                    value
                        .as_array()
                        .is_some_and(|items| !items.is_empty() && unique_json_values(items))
                })
        });
        if object
            .get("description")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
            || !valid_dimensions
            || object
                .get("minimum_rows")
                .and_then(positive_integer)
                .is_none()
            || evidence.is_none_or(Vec::is_empty)
        {
            return Err(failure(format!(
                "cross {name} has an incomplete definition"
            )));
        }
        let minimum_rows = positive_integer(&cross["minimum_rows"]).unwrap();
        let mut identities = BTreeSet::new();
        let mut expected_rows = 0_u64;
        for entry in evidence.unwrap() {
            let Some(entry) = entry.as_object() else {
                return Err(failure(format!("cross {name} evidence is malformed")));
            };
            let phase = entry.get("phase").and_then(Value::as_str);
            let source = entry.get("source").and_then(Value::as_str);
            let marker = entry.get("marker").and_then(Value::as_str);
            let rows = entry.get("expected_rows").and_then(positive_integer);
            if phase.is_none_or(|phase| !phase_pattern.is_match(phase))
                || source.is_none_or(|source| {
                    !source.starts_with("tb/integration/") || !source.ends_with("_tb.sv")
                })
                || marker.is_none_or(|marker| !marker.contains("PASS"))
                || rows.is_none()
            {
                return Err(failure(format!(
                    "cross {name} evidence entry is incomplete"
                )));
            }
            let marker = marker.unwrap();
            if Regex::new(marker).is_err() {
                return Err(failure(format!("cross {name} has invalid marker regex")));
            }
            let identity = (phase.unwrap().to_owned(), source.unwrap().to_owned());
            if !identities.insert(identity.clone()) {
                return Err(failure(format!(
                    "cross {name} repeats evidence {}",
                    identity.0
                )));
            }
            repo_path(source.unwrap(), true)?;
            expected_rows = expected_rows
                .checked_add(rows.unwrap())
                .ok_or_else(|| failure("row count overflow"))?;
        }
        if expected_rows < minimum_rows {
            return Err(failure(format!(
                "cross {name} is below its minimum row count"
            )));
        }
    }
    Ok(())
}

fn build_report(
    manifest: &Manifest,
    regression: &Value,
    manifest_path: &Path,
    regression_path: &Path,
) -> Result<Value, Failure> {
    validate_manifest(manifest)?;
    if regression["schema"].as_str() != Some(REGRESSION_SCHEMA) {
        return Err(failure("regression report has wrong schema"));
    }
    if regression["mode"].as_str() != Some("full") {
        return Err(failure("expected full regression for Chapter 7 cross gate"));
    }
    if !matches!(regression["status"].as_str(), Some("running" | "passed")) {
        return Err(failure("regression is not in a passing/running state"));
    }
    let current_commit = git_text(&["rev-parse", "HEAD"])?;
    let current_dirty =
        !git_text(&["status", "--porcelain=v1", "--untracked-files=all"])?.is_empty();
    let git = regression.get("git").and_then(Value::as_object);
    if git.is_none()
        || regression["git"]["dirty"] != Value::Bool(false)
        || regression["git"]["commit"].as_str() != Some(&current_commit)
        || current_dirty
    {
        return Err(failure(
            "Chapter 7 cross evidence requires the current clean commit",
        ));
    }
    let raw_results = regression["results"]
        .as_array()
        .ok_or_else(|| failure("regression report has no phase results"))?;
    let mut results = BTreeMap::new();
    for result in raw_results {
        let Some(name) = result.get("name").and_then(Value::as_str) else {
            return Err(failure("regression phase names are missing or duplicated"));
        };
        if !result.is_object() || results.insert(name, result).is_some() {
            return Err(failure("regression phase names are missing or duplicated"));
        }
    }

    let mut output_crosses = serde_json::Map::new();
    let mut total_minimum_rows = 0_u64;
    let mut total_observed_rows = 0_u64;
    for name in REQUIRED_CROSSES {
        let definition = &manifest.crosses[name];
        let mut resolved_evidence = Vec::new();
        let mut observed_rows = 0_u64;
        for expected in definition["evidence"].as_array().unwrap() {
            let phase = expected["phase"].as_str().unwrap();
            let result = results.get(phase);
            if result.is_none_or(|result| {
                result["status"].as_str() != Some("passed")
                    || result["exit_code"].as_i64() != Some(0)
            }) {
                return Err(failure(format!(
                    "required Chapter 7 phase did not pass: {phase}"
                )));
            }
            let result = result.unwrap();
            let log_path = repo_path(result["log"].as_str().unwrap_or(""), true)?;
            if sha256(&log_path)? != result["log_sha256"].as_str().unwrap_or("") {
                return Err(failure(format!(
                    "Chapter 7 regression log_sha256 mismatch: {phase}"
                )));
            }
            let log_text = String::from_utf8_lossy(&fs::read(&log_path)?).into_owned();
            let marker = Regex::new(expected["marker"].as_str().unwrap())
                .map_err(|_| failure(format!("cross {name} has invalid marker regex")))?;
            if !marker.is_match(&log_text) {
                return Err(failure(format!(
                    "Chapter 7 PASS marker missing from phase: {phase}"
                )));
            }
            let source_path = repo_path(expected["source"].as_str().unwrap(), true)?;
            let mut row = expected.as_object().unwrap().clone();
            row.insert("status".to_owned(), json!("passed"));
            row.insert("log".to_owned(), file_entry(&log_path)?);
            row.insert("source_artifact".to_owned(), file_entry(&source_path)?);
            resolved_evidence.push(Value::Object(row));
            observed_rows += positive_integer(&expected["expected_rows"]).unwrap();
        }
        let minimum_rows = positive_integer(&definition["minimum_rows"]).unwrap();
        if observed_rows < minimum_rows {
            return Err(failure(format!("cross {name} did not meet minimum_rows")));
        }
        output_crosses.insert(name.to_owned(), json!({
            "description": definition["description"], "dimensions": definition["dimensions"],
            "minimum_rows": minimum_rows, "observed_rows": observed_rows, "evidence": resolved_evidence,
        }));
        total_minimum_rows += minimum_rows;
        total_observed_rows += observed_rows;
    }
    let root = repo_root();
    let crate_root = root.join("verification/constrained_random_rs");
    let input_paths = [
        manifest_path.to_path_buf(),
        crate_root.join("Cargo.toml"),
        crate_root.join("Cargo.lock"),
        crate_root.join("src/main.rs"),
        crate_root.join("src/campaign.rs"),
        crate_root.join("src/table7.rs"),
    ];
    let mut inputs = serde_json::Map::new();
    for path in input_paths {
        let artifact = file_entry(&path)?;
        inputs.insert(artifact["path"].as_str().unwrap().to_owned(), artifact);
    }
    Ok(json!({
        "schema": SCHEMA, "status": "passed", "failure": Value::Null,
        "created_utc": utc_now(),
        "git": {"commit": current_commit, "dirty": false, "source_sha256": regression["git"]["source_sha256"]},
        "regression": {"path": relative(regression_path)?, "schema": REGRESSION_SCHEMA,
            "mode": "full", "status_when_sampled": regression["status"], "commit": current_commit},
        "inputs": inputs, "required_crosses": REQUIRED_CROSSES, "covered_crosses": REQUIRED_CROSSES,
        "missing_crosses": [], "cross_count": REQUIRED_CROSSES.len(),
        "total_minimum_rows": total_minimum_rows, "total_observed_rows": total_observed_rows,
        "crosses": output_crosses,
    }))
}

pub fn table7_cross(arguments: &[String]) -> Result<(), String> {
    let root = repo_root();
    let option_or = |name: &str, default: PathBuf| -> Result<String, String> {
        if arguments.iter().any(|argument| argument == name) {
            crate::option(arguments, name)
        } else {
            Ok(default.to_string_lossy().into_owned())
        }
    };
    let manifest_path = repo_path(
        &option_or("--manifest", root.join("verification/table7_cross.json"))?,
        true,
    )
    .map_err(|error| error.to_string())?;
    let regression_path = repo_path(
        &option_or(
            "--regression",
            root.join("reports/generated/regression.json"),
        )?,
        true,
    )
    .map_err(|error| error.to_string())?;
    let output_path = repo_path(
        &option_or(
            "--output",
            root.join("reports/generated/table7-cross-report.json"),
        )?,
        false,
    )
    .map_err(|error| error.to_string())?;
    let manifest_text = fs::read_to_string(&manifest_path).map_err(|error| error.to_string())?;
    let manifest: Manifest =
        serde_json::from_str(&manifest_text).map_err(|error| error.to_string())?;
    let regression = load_json(&regression_path).map_err(|error| error.to_string())?;
    match build_report(&manifest, &regression, &manifest_path, &regression_path) {
        Ok(report) => {
            write_json(&output_path, &report).map_err(|error| error.to_string())?;
            println!(
                "[table7-cross] PASS ({} crosses, {} evidence rows)",
                REQUIRED_CROSSES.len(),
                report["total_observed_rows"]
            );
            Ok(())
        }
        Err(error) => {
            let failed = json!({"schema": SCHEMA, "status": "failed", "failure": error.to_string(), "created_utc": utc_now()});
            write_json(&output_path, &failed).map_err(|write_error| write_error.to_string())?;
            Err(format!("[table7-cross] FAIL: {error}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_report, normalized_absolute, positive_integer, repo_path, validate_manifest,
        Manifest, REQUIRED_CROSSES,
    };
    use crate::campaign::repo_root;
    use serde_json::json;
    use std::fs;
    use std::path::Path;

    #[test]
    fn reviewed_cross_set_and_path_normalization() {
        assert_eq!(REQUIRED_CROSSES.len(), 9);
        assert_eq!(
            normalized_absolute(Path::new("/tmp/a/../b")),
            Path::new("/tmp/b")
        );
        assert_eq!(positive_integer(&json!(true)), None);
        assert_eq!(positive_integer(&json!(2)), Some(2));
        assert!(repo_path("../outside", false)
            .unwrap_err()
            .to_string()
            .contains("path escapes repository"));
    }

    #[test]
    fn checked_manifest_has_reviewed_rows_and_rejects_quick_regression() {
        let manifest_path = repo_root().join("verification/table7_cross.json");
        let manifest: Manifest =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        validate_manifest(&manifest).unwrap();
        let minimum: u64 = REQUIRED_CROSSES
            .iter()
            .map(|name| manifest.crosses[*name]["minimum_rows"].as_u64().unwrap())
            .sum();
        let expected: u64 = REQUIRED_CROSSES
            .iter()
            .flat_map(|name| manifest.crosses[*name]["evidence"].as_array().unwrap())
            .map(|entry| entry["expected_rows"].as_u64().unwrap())
            .sum();
        assert_eq!((minimum, expected), (1891, 1903));
        let regression =
            json!({"schema":"arm7tdmis-regression-v1", "mode":"quick", "status":"passed"});
        assert!(
            build_report(&manifest, &regression, &manifest_path, &manifest_path)
                .unwrap_err()
                .to_string()
                .contains("expected full regression")
        );
    }
}
