//! Release manifest construction shared with the release-evidence gate.
//!
//! This module only constructs a manifest after the caller has validated the
//! regression and coverage reports. Keeping the manifest builder separate
//! makes its file and tool hashes independently testable while the remaining
//! release validators are migrated.

#[cfg(test)]
mod closure_contracts;
mod files;
mod functional;
mod package;
#[cfg(test)]
mod release_hygiene_contracts;
mod run;
#[cfg(test)]
mod validation_contracts;

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const MANIFEST_SCHEMA: &str = "arm7tdmis-release-evidence-v1";
const TRM: &str = "ARM_DDI_0234B_ARM7TDMI-S_r4p3_TRM.pdf";

#[derive(Debug, Deserialize)]
struct ManifestRequest {
    candidates: Vec<PathBuf>,
    git_commit: String,
    source_sha256: String,
    tools: BTreeMap<String, String>,
    created_utc: String,
    #[serde(default)]
    output: Option<PathBuf>,
    #[serde(default)]
    regression_path: Option<PathBuf>,
    #[serde(default)]
    coverage_path: Option<PathBuf>,
}

fn sha256_bytes(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn json_sha256(value: &Value) -> Result<String, String> {
    // serde_json::Map is key-sorted without preserve_order. This matches the
    // existing Python manifest's compact sort_keys=True, ensure_ascii=False
    // representation for the JSON data in release manifests.
    Ok(sha256_bytes(
        serde_json::to_vec(value).map_err(|error| error.to_string())?,
    ))
}

fn git_tree_sha256(root: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .args(["ls-tree", "-r", "--full-tree", "HEAD"])
        .current_dir(root)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "git ls-tree failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(sha256_bytes(output.stdout))
}

fn file_entry(path: &Path, root: &Path) -> Result<Value, String> {
    let resolved = path
        .canonicalize()
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let relative = resolved.strip_prefix(root).map_err(|_| {
        format!(
            "release evidence file escapes repository: {}",
            resolved.display()
        )
    })?;
    if !resolved.is_file() {
        return Err(format!(
            "release evidence file is missing: {}",
            resolved.display()
        ));
    }
    let contents = fs::read(&resolved).map_err(|error| error.to_string())?;
    Ok(json!({
        "path": relative.to_string_lossy().replace('\\', "/"),
        "bytes": contents.len(),
        "sha256": sha256_bytes(contents),
    }))
}

#[cfg(unix)]
fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable(path: &Path) -> bool {
    path.is_file()
}

fn locate_executable(command: &Path) -> Option<PathBuf> {
    if command.is_absolute() {
        return executable(command).then(|| command.to_path_buf());
    }
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|directory| directory.join(command))
        .find(|candidate| executable(candidate))
}

fn tool_executable_manifest(request: &ManifestRequest) -> Result<Value, String> {
    let mut executables = serde_json::Map::new();
    for label in request.tools.keys() {
        let command = match label.as_str() {
            "clang" => PathBuf::from("clang"),
            "git" => PathBuf::from("git"),
            "make" => PathBuf::from("make"),
            "qemu" => PathBuf::from("qemu-system-arm"),
            "quartus" => PathBuf::from("quartus_map"),
            "rustc" => PathBuf::from("rustc"),
            "verilator" => PathBuf::from("verilator"),
            _ => continue,
        };
        let command_text = command.to_string_lossy().into_owned();
        let entry = if let Some(located) = locate_executable(&command) {
            let resolved = located.canonicalize().map_err(|error| error.to_string())?;
            let contents = fs::read(&resolved).map_err(|error| error.to_string())?;
            json!({
                "command": command_text,
                "path": resolved.to_string_lossy(),
                "bytes": contents.len(),
                "sha256": sha256_bytes(contents),
            })
        } else {
            json!({"command": command_text, "status": "not-found"})
        };
        executables.insert(label.clone(), entry);
    }
    Ok(Value::Object(executables))
}

fn build_manifest(root: &Path, request: &ManifestRequest) -> Result<Value, String> {
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let version = fs::read_to_string(root.join("VERSION"))
        .map_err(|error| error.to_string())?
        .trim()
        .to_owned();
    let executable_manifest = tool_executable_manifest(request)?;
    let tools = serde_json::to_value(&request.tools).map_err(|error| error.to_string())?;
    let mut candidates = BTreeSet::new();
    for path in &request.candidates {
        candidates.insert(path.canonicalize().map_err(|error| error.to_string())?);
    }
    let files = candidates
        .iter()
        .map(|path| file_entry(path, &root))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(json!({
        "schema": MANIFEST_SCHEMA,
        "version": version,
        "created_utc": request.created_utc,
        "git_commit": request.git_commit,
        "source": {
            "git_commit": request.git_commit,
            "git_tree_sha256": git_tree_sha256(&root)?,
            "source_sha256": request.source_sha256,
        },
        "tools": {
            "versions": tools,
            "versions_sha256": json_sha256(&tools)?,
            "executables": executable_manifest,
            "executables_sha256": json_sha256(&executable_manifest)?,
        },
        "specifications": [file_entry(&root.join(TRM), &root)?, file_entry(&root.join("LICENSE"), &root)?],
        "files": files,
    }))
}

const RANDOM_EVENT_CLASS_BINS: [&str; 16] = [
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
const RANDOM_EVENT_BINS: [&str; 10] = [
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
const TABLE7_REQUIRED_CROSSES: [&str; 9] = [
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
struct ValidationRequest {
    kind: String,
    report: Value,
    #[serde(default)]
    coverage: Option<Value>,
    #[serde(default)]
    manifest: Option<Value>,
    #[serde(default)]
    release_grade: Option<bool>,
    expected_commit: String,
}

fn string_set(value: &Value) -> Option<BTreeSet<String>> {
    value
        .as_array()?
        .iter()
        .map(|item| Some(item.as_str()?.to_owned()))
        .collect()
}

fn required_bins(value: &Value, expected: &[&str]) -> bool {
    value.as_array().is_some_and(|rows| {
        rows.len() == expected.len()
            && string_set(value) == Some(expected.iter().map(|item| (*item).to_owned()).collect())
    })
}

fn validate_basic(
    regression: &Value,
    coverage: &Value,
    expected_commit: &str,
) -> Result<(), String> {
    if regression["schema"] != "arm7tdmis-regression-v1" {
        return Err("regression report has wrong schema".into());
    }
    if regression["status"] != "passed" {
        return Err("regression report is not passed".into());
    }
    if coverage["schema"] != "arm7tdmis-coverage-v1" {
        return Err("coverage report has wrong schema".into());
    }
    for (label, report) in [("regression", regression), ("coverage", coverage)] {
        if report["git"]["dirty"] != false {
            return Err(format!("{label} evidence describes a dirty source tree"));
        }
        if report["git"]["commit"] != expected_commit {
            return Err(format!(
                "{label} evidence commit does not match {expected_commit}"
            ));
        }
    }
    Ok(())
}

fn validate_soak(soak: &Value, expected_commit: &str) -> Result<(), String> {
    if soak["schema"] != "arm7tdmis-soak-v1" {
        return Err("soak report has wrong schema".into());
    }
    if soak["status"] != "passed"
        || soak["failures"]
            .as_array()
            .is_none_or(|rows| !rows.is_empty())
    {
        return Err("soak report is not passed".into());
    }
    if soak["git"]["dirty"] != false {
        return Err("soak report describes a dirty source tree".into());
    }
    if soak["git"]["commit"] != expected_commit {
        return Err("soak report commit does not match regression".into());
    }

    let configuration = &soak["configuration"];
    let sanitizer_environment = &configuration["environment"];
    let sanitizers = &configuration["sanitizers"];
    let dependencies = soak["tools"]["binary_dependencies"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    let timeout = configuration["timeout_seconds"].as_f64();
    if configuration["x_assignment"] != "unique"
        || configuration["x_initialization"] != "unique"
        || !required_bins(sanitizers, &["address", "undefined"])
        || !sanitizer_environment["ASAN_OPTIONS"]
            .as_str()
            .is_some_and(|value| value.contains("halt_on_error=1"))
        || !sanitizer_environment["UBSAN_OPTIONS"]
            .as_str()
            .is_some_and(|value| value.contains("halt_on_error=1"))
        || timeout.is_none_or(|value| !value.is_finite() || value <= 0.0)
        || !dependencies.contains("libasan")
        || !dependencies.contains("libubsan")
    {
        return Err("soak report lacks sanitizer/X-state evidence".into());
    }

    let seeds = soak["seeds"]
        .as_array()
        .ok_or("soak report has a malformed seed list")?;
    if seeds.iter().any(|entry| !entry.is_object()) {
        return Err("soak report has a malformed seed list".into());
    }
    let mut unique = BTreeSet::new();
    for entry in seeds {
        let seed = entry["seed"]
            .as_u64()
            .ok_or("soak report does not contain 256 unique seeds")?;
        unique.insert(seed);
    }
    if configuration["seed_count"]
        .as_u64()
        .is_none_or(|count| count < 256 || count != seeds.len() as u64)
        || soak["completed_seed_count"] != seeds.len() as u64
        || unique.len() != seeds.len()
    {
        return Err("soak report does not contain 256 unique seeds".into());
    }
    if seeds.iter().any(|entry| {
        entry["status"] != "passed" || entry["exit_code"] != 0 || entry["pass_marker_found"] != true
    }) {
        return Err("soak report contains a non-passing seed".into());
    }
    Ok(())
}

fn validate_random_events(report: &Value, expected_commit: &str) -> Result<(), String> {
    if report["schema"] != "arm7tdmis-random-events-v1" {
        return Err("random-event report has wrong schema".into());
    }
    if report["status"] != "passed"
        || report["failures"]
            .as_array()
            .is_none_or(|rows| !rows.is_empty())
    {
        return Err("random-event report is not passed".into());
    }
    if report["git"]["dirty"] != false {
        return Err("random-event report describes a dirty source tree".into());
    }
    if report["git"]["commit"] != expected_commit {
        return Err("random-event report commit does not match regression".into());
    }

    if !required_bins(&report["required_class_bins"], &RANDOM_EVENT_CLASS_BINS)
        || !required_bins(&report["required_event_bins"], &RANDOM_EVENT_BINS)
    {
        return Err("random-event required-bin manifest was weakened".into());
    }
    for (field, expected) in [
        ("covered_class_bins", RANDOM_EVENT_CLASS_BINS.as_slice()),
        (
            "covered_class_stall_bins",
            RANDOM_EVENT_CLASS_BINS.as_slice(),
        ),
        ("covered_event_bins", RANDOM_EVENT_BINS.as_slice()),
    ] {
        if string_set(&report[field])
            != Some(expected.iter().map(|item| (*item).to_owned()).collect())
        {
            return Err("random-event aggregate coverage is incomplete".into());
        }
    }

    let configuration = &report["configuration"];
    let seeds = report["seeds"]
        .as_array()
        .ok_or("random-event report lacks 32 unique strong seeds")?;
    let seed_count = configuration["seed_count"].as_u64();
    let minimum_decisions = configuration["minimum_decisions_per_seed"].as_u64();
    let mut unique = BTreeSet::new();
    for entry in seeds {
        let seed = entry["seed"]
            .as_u64()
            .ok_or("random-event report lacks 32 unique strong seeds")?;
        unique.insert(seed);
    }
    if seeds.iter().any(|entry| !entry.is_object())
        || seed_count.is_none_or(|count| count < 32 || count != seeds.len() as u64)
        || report["completed_seed_count"] != seeds.len() as u64
        || unique.len() != seeds.len()
        || minimum_decisions.is_none_or(|count| count < 256)
    {
        return Err("random-event report lacks 32 unique strong seeds".into());
    }

    let minimum_decisions = minimum_decisions.unwrap();
    let mut total_decisions = 0_u64;
    for entry in seeds {
        let decisions = entry["decision_count"].as_u64().unwrap_or(0);
        total_decisions = total_decisions
            .checked_add(decisions)
            .ok_or("random-event decision total overflows")?;
        if entry["status"] != "passed"
            || entry["exit_code"] != 0
            || entry["pass_marker_found"] != true
            || decisions < minimum_decisions
            || string_set(&entry["class_bins"])
                != Some(
                    RANDOM_EVENT_CLASS_BINS
                        .iter()
                        .map(|item| (*item).to_owned())
                        .collect(),
                )
            || string_set(&entry["class_stall_bins"])
                != Some(
                    RANDOM_EVENT_CLASS_BINS
                        .iter()
                        .map(|item| (*item).to_owned())
                        .collect(),
                )
            || string_set(&entry["event_bins"])
                != Some(
                    RANDOM_EVENT_BINS
                        .iter()
                        .map(|item| (*item).to_owned())
                        .collect(),
                )
            || entry["reproducer"].as_array().is_none_or(Vec::is_empty)
        {
            return Err("random-event seed evidence is incomplete".into());
        }
    }
    if report["total_decision_count"] != total_decisions {
        return Err("random-event decision total is inconsistent".into());
    }
    Ok(())
}

fn formal_required_covers(manifest: &Value) -> Result<BTreeSet<String>, String> {
    if manifest["schema"] != "arm7tdmis-formal-map-v1" {
        return Err("formal requirements manifest has wrong schema".into());
    }
    let mut required = BTreeSet::new();
    for (field, prefix) in [
        ("fsm_states", "fsm.state"),
        ("fsm_transitions", "fsm.transition"),
        ("exceptions", "exception"),
        ("debug", "debug"),
    ] {
        let rows = manifest["covers"][field]
            .as_array()
            .ok_or("formal requirements manifest has malformed covers")?;
        for row in rows {
            let value = row
                .as_str()
                .ok_or("formal requirements manifest has malformed covers")?;
            required.insert(format!("{prefix}.{value}"));
        }
    }
    Ok(required)
}

fn valid_sha256(value: &Value) -> bool {
    value.as_str().is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn valid_lower_hex(value: &Value, digits: usize) -> bool {
    value.as_str().is_some_and(|text| {
        text.len() == digits
            && text
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn valid_hashed_artifact(value: &Value) -> bool {
    value["path"].as_str().is_some_and(|path| !path.is_empty())
        && value["bytes"].as_u64().is_some_and(|count| count > 0)
        && valid_sha256(&value["sha256"])
}

fn validate_formal(report: &Value, manifest: &Value, expected_commit: &str) -> Result<(), String> {
    if report["schema"] != "arm7tdmis-formal-v1" {
        return Err("formal report has wrong schema".into());
    }
    if report["status"] != "passed" || !report["failure"].is_null() {
        return Err("formal report is not passed".into());
    }
    if report["git"]["dirty"] != false {
        return Err("formal report describes a dirty source tree".into());
    }
    if report["git"]["commit"] != expected_commit {
        return Err("formal report commit does not match regression".into());
    }
    if report["toolchain"] != manifest["toolchain"] {
        return Err("formal report does not use the pinned toolchain".into());
    }

    let proof_entries = manifest["proofs"]
        .as_array()
        .ok_or("formal proof manifest is malformed")?;
    let mut required_proofs = BTreeSet::new();
    for entry in proof_entries {
        let id = entry["id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or("formal proof IDs are missing or duplicated")?;
        if !required_proofs.insert(id.to_owned()) {
            return Err("formal proof IDs are missing or duplicated".into());
        }
    }
    let required_covers = formal_required_covers(manifest)?;
    if string_set(&report["required_proofs"]) != Some(required_proofs.clone())
        || string_set(&report["proven_proofs"]) != Some(required_proofs.clone())
        || string_set(&report["required_covers"]) != Some(required_covers.clone())
        || string_set(&report["covered_covers"]) != Some(required_covers.clone())
        || report["uncovered_covers"] != json!([])
    {
        return Err("formal proof or cover closure is incomplete".into());
    }

    let results = report["results"]
        .as_object()
        .ok_or("formal result map is incomplete")?;
    let required_results: BTreeSet<String> =
        required_proofs.union(&required_covers).cloned().collect();
    if results.keys().cloned().collect::<BTreeSet<_>>() != required_results {
        return Err("formal result map is incomplete".into());
    }
    for (obligation, entry) in results {
        if entry["status"] != "passed"
            || entry["engine"].as_str().is_none_or(str::is_empty)
            || entry["depth"].as_u64().is_none_or(|depth| depth == 0)
        {
            return Err(format!("formal result is not passing: {obligation}"));
        }
        for artifact_name in ["log", "source"] {
            if !valid_hashed_artifact(&entry[artifact_name]) {
                return Err(format!("formal {obligation} lacks hashed {artifact_name}"));
            }
        }
        if required_covers.contains(obligation) {
            let witnesses = entry["witnesses"]
                .as_array()
                .ok_or_else(|| format!("formal cover {obligation} lacks its witness pair"))?;
            let extensions: BTreeSet<_> = witnesses
                .iter()
                .filter_map(|item| Path::new(item["path"].as_str()?).extension()?.to_str())
                .collect();
            if witnesses.len() != 2 || extensions != BTreeSet::from(["vcd", "yw"]) {
                return Err(format!("formal cover {obligation} lacks its witness pair"));
            }
            if witnesses.iter().any(|item| !valid_hashed_artifact(item)) {
                return Err(format!("formal cover {obligation} has an unhashed witness"));
            }
        }
    }
    Ok(())
}

fn validate_constrained_random(
    report: &Value,
    expected_commit: &str,
    release_grade: bool,
) -> Result<(), String> {
    if report["schema"] != "arm7tdmis-constrained-random-v1" {
        return Err("constrained-random report has wrong schema".into());
    }
    if report["status"] != "passed" || (!report["failure"].is_null() && report["failure"] != false)
    {
        return Err("constrained-random report is not passed".into());
    }
    if report["git"]["dirty"] != false {
        return Err("constrained-random report describes a dirty source tree".into());
    }
    if report["git"]["commit"] != expected_commit {
        return Err("constrained-random report commit does not match regression".into());
    }

    let configuration = &report["configuration"];
    let seeds = report["seeds"]
        .as_array()
        .ok_or("constrained-random seed manifest is incomplete")?;
    let mut unique = BTreeSet::new();
    for seed in seeds {
        let id = seed["seed"]
            .as_u64()
            .ok_or("constrained-random seed manifest is incomplete")?;
        unique.insert(id);
    }
    if report["completed_seed_count"] != seeds.len() as u64
        || configuration["seed_count"] != seeds.len() as u64
        || unique.len() != seeds.len()
        || seeds.iter().any(|seed| seed["status"] != "passed")
    {
        return Err("constrained-random seed manifest is incomplete".into());
    }
    let minimum_seeds = if release_grade { 32 } else { 2 };
    let minimum_instructions = if release_grade { 256 } else { 64 };
    let instructions = configuration["instructions_per_seed"].as_u64().unwrap_or(0);
    if seeds.len() < minimum_seeds
        || instructions < minimum_instructions
        || configuration["release_grade"] != release_grade
    {
        return Err("constrained-random campaign strength is too low".into());
    }

    let mut total_events = 0_u64;
    for seed in seeds {
        total_events = total_events
            .checked_add(seed["qemu_events"].as_u64().unwrap_or(0))
            .ok_or("constrained-random event total overflows")?;
    }
    if report["total_qemu_events"] != total_events {
        return Err("constrained-random event total is inconsistent".into());
    }
    if release_grade && total_events < 8_192 {
        return Err("constrained-random release trace is too short".into());
    }

    let required_dimensions: BTreeSet<String> = [
        "instruction",
        "arm",
        "thumb",
        "register",
        "flags",
        "mode",
        "banked_register",
        "memory",
        "load",
        "store",
        "exception",
        "svc",
        "undefined",
        "alignment",
        "aligned",
        "unaligned",
        "little",
        "big",
        "dependency",
        "permitted_memory",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let required = string_set(&report["required_coverage"])
        .ok_or("constrained-random required coverage is incomplete")?;
    let covered = string_set(&report["covered"])
        .ok_or("constrained-random required coverage is incomplete")?;
    if required != required_dimensions || !required.is_subset(&covered) {
        return Err("constrained-random required coverage is incomplete".into());
    }

    for seed in seeds {
        let profiles = seed["policy_profiles"].as_object();
        if seed["qemu_events"].as_u64().unwrap_or(0) < instructions
            || profiles.is_none_or(|profiles| {
                profiles.keys().map(String::as_str).collect::<BTreeSet<_>>()
                    != BTreeSet::from(["little", "big"])
            })
            || seed["reproducer"].as_array().is_none_or(Vec::is_empty)
        {
            return Err("constrained-random seed evidence is incomplete".into());
        }
        for name in ["little", "big"] {
            let profile = &seed["policy_profiles"][name];
            if profile["status"] != "passed"
                || profile["endian"] != name
                || profile["result_words"]
                    .as_u64()
                    .is_none_or(|count| count < 20)
            {
                return Err(format!("constrained-random {name} profile is incomplete"));
            }
        }
    }
    Ok(())
}

fn validate_public_suite(
    report: &Value,
    manifest: &Value,
    expected_commit: &str,
) -> Result<(), String> {
    if manifest["schema"] != "arm7tdmis-public-suites-v1" {
        return Err("public-suite manifest has wrong schema".into());
    }
    let manifest_upstream = &manifest["upstream"];
    let manifest_suites = &manifest["suites"];
    let manifest_patches = &manifest["patches"];
    if !manifest_upstream.is_object()
        || manifest_upstream["url"] != "https://github.com/jsmolka/gba-suite.git"
        || manifest_upstream["license_spdx"] != "MIT"
        || !valid_lower_hex(&manifest_upstream["commit"], 40)
        || !valid_lower_hex(&manifest_upstream["tree"], 40)
        || !valid_sha256(&manifest_upstream["license_sha256"])
        || !manifest_patches.is_array()
        || manifest_suites.as_object().is_none_or(|suites| {
            suites.keys().map(String::as_str).collect::<BTreeSet<_>>()
                != BTreeSet::from(["arm", "thumb"])
        })
    {
        return Err("public-suite manifest is not trusted".into());
    }
    if report["schema"] != "arm7tdmis-public-suite-v1" {
        return Err("public-suite report has wrong schema".into());
    }
    if report["status"] != "passed" || (!report["failure"].is_null() && report["failure"] != false)
    {
        return Err("public-suite report is not passed".into());
    }
    if report["git"]["dirty"] != false {
        return Err("public-suite report describes a dirty source tree".into());
    }
    if report["git"]["commit"] != expected_commit {
        return Err("public-suite report commit does not match regression".into());
    }

    let upstream = &report["upstream"];
    if !upstream.is_object()
        || manifest_upstream
            .as_object()
            .unwrap()
            .iter()
            .any(|(key, value)| upstream[key] != *value)
    {
        return Err("public-suite upstream provenance does not match".into());
    }
    let license_artifact = &upstream["license_artifact"];
    if !upstream["acquisition_commands"].is_array()
        || license_artifact["sha256"] != manifest_upstream["license_sha256"]
        || license_artifact["bytes"]
            .as_u64()
            .is_none_or(|count| count == 0)
    {
        return Err("public-suite license evidence is incomplete".into());
    }
    if report["patches"] != *manifest_patches {
        return Err("public-suite policy patch manifest was altered".into());
    }
    let suites = report["suites"].as_object();
    if suites.is_none_or(|suites| {
        suites.keys().map(String::as_str).collect::<BTreeSet<_>>()
            != BTreeSet::from(["arm", "thumb"])
    }) || report["suite_count"] != 2
    {
        return Err("public-suite report does not contain both suites".into());
    }

    let mut total_retirements = 0_u64;
    for name in ["arm", "thumb"] {
        let suite_manifest = &manifest_suites[name];
        let suite = &report["suites"][name];
        if !suite_manifest.is_object() || !suite.is_object() {
            return Err(format!("public-suite {name} entry is malformed"));
        }
        for (report_key, manifest_key) in [
            ("upstream_path", "path"),
            ("upstream_sha256", "upstream_sha256"),
            ("patched_sha256", "patched_sha256"),
            ("idle_pc", "idle_pc"),
            ("result_register", "result_register"),
            ("expected_vram_signature", "expected_vram_signature"),
            ("word_patches", "word_patches"),
        ] {
            if suite[report_key] != suite_manifest[manifest_key] {
                return Err(format!(
                    "public-suite {name} result does not match its manifest"
                ));
            }
        }
        if suite["status"] != "passed" {
            return Err(format!(
                "public-suite {name} result does not match its manifest"
            ));
        }
        let signature = suite["expected_vram_signature"].as_str().unwrap_or("");
        if !signature.starts_with("0x")
            || !valid_lower_hex(&json!(&signature[2..]), 8)
            || signature == "0x00000000"
        {
            return Err(format!("public-suite {name} signature is invalid"));
        }

        let metrics = &suite["metrics"];
        let values = [
            "retirements",
            "arm_retirements",
            "thumb_retirements",
            "rom_words",
        ]
        .map(|field| metrics[field].as_u64());
        if values.iter().any(Option::is_none) {
            return Err(format!(
                "public-suite {name} retirement evidence is too weak"
            ));
        }
        let [Some(retirements), Some(arm_retirements), Some(thumb_retirements), Some(rom_words)] =
            values
        else {
            unreachable!();
        };
        let minimum = suite_manifest["minimum_retirements"].as_u64().unwrap_or(0);
        let (minimum_arm, minimum_thumb) = if name == "arm" {
            (8_000, 1)
        } else {
            (6_000, 500)
        };
        if arm_retirements.checked_add(thumb_retirements) != Some(retirements)
            || retirements < minimum
            || rom_words == 0
            || arm_retirements < minimum_arm
            || thumb_retirements < minimum_thumb
        {
            return Err(format!(
                "public-suite {name} retirement evidence is too weak"
            ));
        }
        total_retirements = total_retirements
            .checked_add(retirements)
            .ok_or("public-suite total retirement count overflows")?;

        let reproducer = suite["reproducer"].as_array();
        let artifacts = suite["artifacts"].as_object();
        if reproducer.is_none_or(|rows| {
            rows.is_empty()
                || rows
                    .iter()
                    .any(|row| row.as_str().is_none_or(str::is_empty))
        }) || artifacts.is_none_or(serde_json::Map::is_empty)
        {
            return Err(format!(
                "public-suite {name} reproduction evidence is incomplete"
            ));
        }
        if artifacts
            .unwrap()
            .values()
            .any(|artifact| !valid_hashed_artifact(artifact))
        {
            return Err(format!(
                "public-suite {name} artifact manifest is malformed"
            ));
        }
    }
    if report["total_retirements"] != total_retirements || total_retirements < 14_000 {
        return Err("public-suite total retirement evidence is inconsistent".into());
    }
    Ok(())
}

fn validate_table7(report: &Value, manifest: &Value, expected_commit: &str) -> Result<(), String> {
    let required = &manifest["required_crosses"];
    let definitions = &manifest["crosses"];
    if manifest["schema"] != "arm7tdmis-table7-cross-map-v1"
        || required != &json!(TABLE7_REQUIRED_CROSSES)
        || definitions.as_object().is_none_or(|crosses| {
            crosses.keys().map(String::as_str).collect::<BTreeSet<_>>()
                != TABLE7_REQUIRED_CROSSES.into_iter().collect()
        })
    {
        return Err("Chapter 7 cross manifest is not trusted".into());
    }
    for name in TABLE7_REQUIRED_CROSSES {
        let definition = &definitions[name];
        if definition["description"].as_str().is_none_or(str::is_empty)
            || definition["dimensions"]
                .as_object()
                .is_none_or(serde_json::Map::is_empty)
            || definition["minimum_rows"]
                .as_u64()
                .is_none_or(|count| count == 0)
            || definition["evidence"].as_array().is_none_or(Vec::is_empty)
        {
            return Err(format!("Chapter 7 cross definition is weak: {name}"));
        }
        for evidence in definition["evidence"].as_array().unwrap() {
            let phase = evidence["phase"].as_str().unwrap_or("");
            let phase_suffix = phase.strip_prefix("integration-");
            if phase_suffix.is_none_or(|suffix| {
                suffix.is_empty()
                    || !suffix.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'
                    })
            }) || !evidence["source"]
                .as_str()
                .is_some_and(|source| source.starts_with("tb/integration/"))
                || !evidence["marker"]
                    .as_str()
                    .is_some_and(|marker| marker.contains("PASS"))
                || evidence["expected_rows"]
                    .as_u64()
                    .is_none_or(|count| count == 0)
            {
                return Err(format!("Chapter 7 cross evidence is weak: {name}"));
            }
        }
    }
    if report["schema"] != "arm7tdmis-table7-cross-v1" {
        return Err("Chapter 7 cross report has wrong schema".into());
    }
    if report["status"] != "passed" || (!report["failure"].is_null() && report["failure"] != false)
    {
        return Err("Chapter 7 cross report is not passed".into());
    }
    if report["git"]["dirty"] != false {
        return Err("Chapter 7 cross report describes a dirty tree".into());
    }
    if report["git"]["commit"] != expected_commit {
        return Err("Chapter 7 cross report commit does not match".into());
    }
    if report["required_crosses"] != *required
        || report["covered_crosses"] != *required
        || report["missing_crosses"] != json!([])
        || report["cross_count"] != TABLE7_REQUIRED_CROSSES.len()
    {
        return Err("Chapter 7 required cross coverage is incomplete".into());
    }
    let report_crosses = &report["crosses"];
    if report_crosses.as_object().is_none_or(|crosses| {
        crosses.keys().map(String::as_str).collect::<BTreeSet<_>>()
            != TABLE7_REQUIRED_CROSSES.into_iter().collect()
    }) {
        return Err("Chapter 7 report cross set is incomplete".into());
    }
    let mut total_minimum_rows = 0_u64;
    let mut total_observed_rows = 0_u64;
    for name in TABLE7_REQUIRED_CROSSES {
        let definition = &definitions[name];
        let cross = &report_crosses[name];
        let expected_evidence = definition["evidence"].as_array().unwrap();
        let actual_evidence = cross["evidence"].as_array();
        if cross["description"] != definition["description"]
            || cross["dimensions"] != definition["dimensions"]
            || cross["minimum_rows"] != definition["minimum_rows"]
            || actual_evidence.is_none_or(|rows| rows.len() != expected_evidence.len())
        {
            return Err(format!("Chapter 7 cross was altered: {name}"));
        }
        let mut observed_rows = 0_u64;
        for (expected, actual) in expected_evidence.iter().zip(actual_evidence.unwrap()) {
            if !actual.is_object()
                || ["phase", "source", "marker", "expected_rows"]
                    .iter()
                    .any(|field| actual[*field] != expected[*field])
            {
                return Err(format!("Chapter 7 evidence identity was altered: {name}"));
            }
            let log = &actual["log"];
            let source = &actual["source_artifact"];
            if actual["status"] != "passed"
                || !log["path"]
                    .as_str()
                    .is_some_and(|path| path.starts_with("reports/generated/logs/"))
                || !valid_hashed_artifact(log)
                || source["path"] != expected["source"]
                || !valid_hashed_artifact(source)
            {
                return Err(format!("Chapter 7 hashed evidence is incomplete: {name}"));
            }
            observed_rows = observed_rows
                .checked_add(expected["expected_rows"].as_u64().unwrap())
                .ok_or("Chapter 7 observed row count overflows")?;
        }
        let minimum_rows = definition["minimum_rows"].as_u64().unwrap();
        if cross["observed_rows"] != observed_rows || observed_rows < minimum_rows {
            return Err(format!("Chapter 7 cross is under-covered: {name}"));
        }
        total_minimum_rows = total_minimum_rows
            .checked_add(minimum_rows)
            .ok_or("Chapter 7 minimum row count overflows")?;
        total_observed_rows = total_observed_rows
            .checked_add(observed_rows)
            .ok_or("Chapter 7 total observed row count overflows")?;
    }
    if report["total_minimum_rows"] != total_minimum_rows
        || report["total_observed_rows"] != total_observed_rows
    {
        return Err("Chapter 7 aggregate row counts are inconsistent".into());
    }
    Ok(())
}

pub(crate) fn validation_command() -> Result<String, String> {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|error| error.to_string())?;
    let request: ValidationRequest = serde_json::from_str(&input)
        .map_err(|error| format!("invalid release validation request: {error}"))?;
    match request.kind.as_str() {
        "basic" => validate_basic(
            &request.report,
            request
                .coverage
                .as_ref()
                .ok_or("basic validation needs coverage")?,
            &request.expected_commit,
        )?,
        "soak" => validate_soak(&request.report, &request.expected_commit)?,
        "random-events" => validate_random_events(&request.report, &request.expected_commit)?,
        "formal" => validate_formal(
            &request.report,
            request
                .manifest
                .as_ref()
                .ok_or("formal validation needs manifest")?,
            &request.expected_commit,
        )?,
        "constrained-random" => validate_constrained_random(
            &request.report,
            &request.expected_commit,
            request
                .release_grade
                .ok_or("constrained-random validation needs release_grade")?,
        )?,
        "public-suite" => validate_public_suite(
            &request.report,
            request
                .manifest
                .as_ref()
                .ok_or("public-suite validation needs manifest")?,
            &request.expected_commit,
        )?,
        "table7-cross" => validate_table7(
            &request.report,
            request
                .manifest
                .as_ref()
                .ok_or("Chapter 7 validation needs manifest")?,
            &request.expected_commit,
        )?,
        "functional-coverage" => functional::validate(
            &request.report,
            request
                .manifest
                .as_ref()
                .ok_or("functional-coverage validation needs manifest")?,
            &request.expected_commit,
        )?,
        _ => return Err(format!("unknown release validation kind: {}", request.kind)),
    }
    Ok("{}".into())
}

pub(crate) fn manifest_command() -> Result<String, String> {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|error| error.to_string())?;
    let request: ManifestRequest = serde_json::from_str(&input)
        .map_err(|error| format!("invalid release manifest request: {error}"))?;
    serde_json::to_string(&build_manifest(&crate::campaign::repo_root(), &request)?)
        .map_err(|error| error.to_string())
}

pub(crate) fn package_command() -> Result<String, String> {
    package::command()
}

pub(crate) fn files_prefix_command() -> Result<String, String> {
    files::prefix_command()
}

pub(crate) fn run_command(arguments: &[String]) -> Result<String, String> {
    run::command(arguments)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_release_validation_rejects_bad_or_mismatched_coverage_evidence() {
        let commit = "c".repeat(40);
        let regression = json!({
            "schema": "arm7tdmis-regression-v1",
            "status": "passed",
            "git": {"commit": commit, "dirty": false},
        });
        let coverage = json!({
            "schema": "arm7tdmis-coverage-v1",
            "git": {"commit": commit, "dirty": false},
        });
        validate_basic(&regression, &coverage, &commit).unwrap();

        let mut failed = regression.clone();
        failed["status"] = json!("failed");
        assert!(validate_basic(&failed, &coverage, &commit)
            .unwrap_err()
            .contains("not passed"));

        let mut dirty = coverage.clone();
        dirty["git"]["dirty"] = json!(true);
        assert!(validate_basic(&regression, &dirty, &commit)
            .unwrap_err()
            .contains("dirty"));

        assert!(validate_basic(&regression, &coverage, &"d".repeat(40))
            .unwrap_err()
            .contains("commit"));
    }

    #[test]
    fn release_manifest_preserves_source_tool_and_file_hashes() {
        let root = crate::campaign::repo_root();
        let request = ManifestRequest {
            candidates: vec![root.join("LICENSE"), root.join("LICENSE")],
            git_commit: "a".repeat(40),
            source_sha256: "b".repeat(64),
            tools: BTreeMap::from([("unknown-tool".into(), "version".into())]),
            created_utc: "2026-01-01T00:00:00+00:00".into(),
            output: None,
            regression_path: None,
            coverage_path: None,
        };
        let manifest = build_manifest(&root, &request).unwrap();
        assert_eq!(manifest["schema"], MANIFEST_SCHEMA);
        assert_eq!(manifest["files"].as_array().unwrap().len(), 1);
        assert_eq!(manifest["files"][0]["path"], "LICENSE");
        assert_eq!(manifest["source"]["git_commit"], request.git_commit);
        assert_eq!(manifest["source"]["source_sha256"], request.source_sha256);
        assert_eq!(
            manifest["tools"]["versions_sha256"],
            json_sha256(&json!(request.tools)).unwrap()
        );
        assert_eq!(manifest["tools"]["executables"], json!({}));
        assert_eq!(
            manifest["specifications"][0]["sha256"],
            "bb9cd0e3f2b7e2fdca4ff961cdfc5c9c85c063842e491f8997a504d3241baa14"
        );
    }

    #[test]
    fn release_manifest_rejects_missing_or_outside_files() {
        let root = crate::campaign::repo_root();
        let mut request = ManifestRequest {
            candidates: vec![root.join("missing-release-file")],
            git_commit: "a".repeat(40),
            source_sha256: "b".repeat(64),
            tools: BTreeMap::new(),
            created_utc: "2026-01-01T00:00:00+00:00".into(),
            output: None,
            regression_path: None,
            coverage_path: None,
        };
        assert!(build_manifest(&root, &request).is_err());
        request.candidates = vec![PathBuf::from("/etc/passwd")];
        assert!(build_manifest(&root, &request).is_err());
    }

    #[test]
    fn constrained_random_release_gate_rejects_weakening() {
        let required = [
            "instruction",
            "arm",
            "thumb",
            "register",
            "flags",
            "mode",
            "banked_register",
            "memory",
            "load",
            "store",
            "exception",
            "svc",
            "undefined",
            "alignment",
            "aligned",
            "unaligned",
            "little",
            "big",
            "dependency",
            "permitted_memory",
        ];
        let seeds = (1..=2)
            .map(|seed| {
                json!({
                    "seed": seed,
                    "status": "passed",
                    "qemu_events": 64,
                    "reproducer": ["qemu", format!("--seed={seed}")],
                    "policy_profiles": {
                        "little": {"status": "passed", "endian": "little", "result_words": 20},
                        "big": {"status": "passed", "endian": "big", "result_words": 20},
                    },
                })
            })
            .collect::<Vec<_>>();
        let mut report = json!({
            "schema": "arm7tdmis-constrained-random-v1",
            "status": "passed",
            "failure": null,
            "git": {"commit": "abc123", "dirty": false},
            "configuration": {"seed_count": 2, "instructions_per_seed": 64, "release_grade": false},
            "completed_seed_count": 2,
            "seeds": seeds,
            "total_qemu_events": 128,
            "required_coverage": required,
            "covered": required,
        });
        assert!(validate_constrained_random(&report, "abc123", false).is_ok());
        report["covered"].as_array_mut().unwrap().pop();
        assert!(validate_constrained_random(&report, "abc123", false).is_err());
        report["covered"] = json!(required);
        report["seeds"][0]["qemu_events"] = json!(63);
        report["total_qemu_events"] = json!(127);
        assert!(validate_constrained_random(&report, "abc123", false).is_err());
    }
}
