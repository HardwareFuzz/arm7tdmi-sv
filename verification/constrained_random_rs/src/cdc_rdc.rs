//! Fail-hard structural clock-domain and reset-domain audit.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;

use chrono::{SecondsFormat, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::campaign::repo_root;
use crate::pinned_archive::file_sha256;

const REPORT_SCHEMA: &str = "arm7tdmis-cdc-rdc-v1";
const MANIFEST_SCHEMA: &str = "arm7tdmis-cdc-rdc-manifest-v1";

static BLOCK_COMMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)/\*.*?\*/").unwrap());
static LINE_COMMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"//[^\n]*").unwrap());
static ALWAYS_FF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\balways_ff\s*@\s*\((?P<sensitivity>[^)]*)\)").unwrap());
static EDGE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(posedge|negedge)\s+([A-Za-z_]\w*)").unwrap());
static NONBLOCKING_ASSIGN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)\b(?P<lhs>[A-Za-z_]\w*)(?:\s*\[[^]]+\])?\s*<=\s*(?P<rhs>[^;]+);").unwrap()
});
static CONTINUOUS_ASSIGN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)\bassign\s+(?P<lhs>[A-Za-z_]\w*)(?:\s*\[[^]]+\])?\s*=\s*(?P<rhs>[^;]+);")
        .unwrap()
});
static ALWAYS_LATCH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\balways_latch\b").unwrap());
static UNTYPED_ALWAYS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\balways\s*@").unwrap());

#[derive(Deserialize)]
struct Manifest {
    schema: Option<String>,
    #[serde(default)]
    clock_domains: Vec<String>,
    #[serde(default)]
    allowed_async_resets: Vec<String>,
    #[serde(default)]
    synchronizers: Vec<Synchronizer>,
    reset_synchronizer_primitive: Option<ResetPrimitive>,
    #[serde(default)]
    reset_release_instances: Vec<ResetRelease>,
    expected_sequential_block_count: Option<usize>,
    expected_async_reset_block_count: Option<usize>,
    #[serde(default)]
    reviewed_reset_contracts: Vec<Value>,
}

#[derive(Deserialize)]
struct Synchronizer {
    file: String,
    source: String,
    first_stage: String,
    second_stage: String,
    #[serde(default)]
    require_attribute: bool,
}

#[derive(Deserialize)]
struct ResetPrimitive {
    file: String,
    async_source: String,
    first_stage: String,
    second_stage: String,
    output: String,
    #[serde(default)]
    require_attribute: bool,
}

#[derive(Deserialize)]
struct ResetRelease {
    file: String,
    instance: String,
    source: String,
    released_reset: String,
}

#[derive(Serialize)]
struct Violation {
    rule: String,
    path: String,
    detail: String,
}

#[derive(Serialize)]
struct SequentialBlock {
    path: String,
    line: usize,
    sensitivity: String,
    clocks: Vec<String>,
    async_resets: Vec<String>,
}

#[derive(Serialize)]
struct SynchronizerResult {
    file: String,
    source: String,
    first_stage: String,
    second_stage: String,
    source_users: Vec<String>,
    first_stage_users: Vec<String>,
    marked: bool,
}

#[derive(Serialize)]
struct ResetPrimitiveResult {
    file: String,
    async_source: String,
    first_stage: String,
    second_stage: String,
    output: String,
    first_stage_values: Vec<String>,
    second_stage_values: Vec<String>,
    first_stage_users: Vec<String>,
    second_stage_users: Vec<String>,
    marked: bool,
    sensitivity_verified: bool,
    status: &'static str,
}

#[derive(Serialize)]
struct ResetReleaseResult {
    file: String,
    instance: String,
    source: String,
    released_reset: String,
    status: &'static str,
}

#[derive(Serialize)]
struct Analysis {
    clock_domains: Vec<String>,
    sequential_block_count: usize,
    async_reset_block_count: usize,
    sequential_blocks: Vec<SequentialBlock>,
    synchronizer_count: usize,
    synchronizers: Vec<SynchronizerResult>,
    reset_synchronizer_primitive: Value,
    reset_release_count: usize,
    reset_release_instances: Vec<ResetReleaseResult>,
    violations: Vec<Violation>,
}

fn strip_comments(source: &str) -> String {
    let without_blocks = BLOCK_COMMENT.replace_all(source, " ");
    LINE_COMMENT.replace_all(&without_blocks, " ").into_owned()
}

fn token_present(text: &str, token: &str) -> bool {
    let pattern = format!(r"\b{}\b", regex::escape(token));
    Regex::new(&pattern).unwrap().is_match(text)
}

fn assignments(source: &str) -> Vec<(String, String)> {
    let mut found = Vec::<(usize, String, String)>::new();
    for pattern in [&*NONBLOCKING_ASSIGN, &*CONTINUOUS_ASSIGN] {
        for capture in pattern.captures_iter(source) {
            found.push((
                capture.get(0).unwrap().start(),
                capture["lhs"].to_owned(),
                capture["rhs"].to_owned(),
            ));
        }
    }
    found.sort_by_key(|item| item.0);
    found.into_iter().map(|(_, lhs, rhs)| (lhs, rhs)).collect()
}

fn attribute_marks(source: &str, signal: &str) -> bool {
    let pattern = format!(
        r"(?s)SYNCHRONIZER_IDENTIFICATION\s+FORCED_IF_ASYNCHRONOUS[^;]{{0,240}}\blogic\b[^;]*\b{}\b",
        regex::escape(signal)
    );
    Regex::new(&pattern).unwrap().is_match(source)
}

fn edges(sensitivity: &str) -> (Vec<String>, Vec<String>) {
    let mut positive = Vec::new();
    let mut negative = Vec::new();
    for capture in EDGE.captures_iter(sensitivity) {
        if &capture[1] == "posedge" {
            positive.push(capture[2].to_owned());
        } else {
            negative.push(capture[2].to_owned());
        }
    }
    (positive, negative)
}

fn violation(violations: &mut Vec<Violation>, rule: &str, path: &str, detail: impl Into<String>) {
    violations.push(Violation {
        rule: rule.to_owned(),
        path: path.to_owned(),
        detail: detail.into(),
    });
}

fn python_list(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| format!("'{value}'"))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn analyze_sources(sources: &BTreeMap<String, String>, manifest: &Manifest) -> Analysis {
    let clocks: BTreeSet<_> = manifest.clock_domains.iter().cloned().collect();
    let resets: BTreeSet<_> = manifest.allowed_async_resets.iter().cloned().collect();
    let mut violations = Vec::new();
    let mut sequential_blocks = Vec::new();

    for (path, original_source) in sources {
        let source = strip_comments(original_source);
        if ALWAYS_LATCH.is_match(&source) {
            violation(
                &mut violations,
                "inferred-latch-process",
                path,
                "always_latch is forbidden in production RTL",
            );
        }
        if UNTYPED_ALWAYS.is_match(&source) {
            violation(
                &mut violations,
                "untyped-sequential-process",
                path,
                "use always_ff/always_comb so driver intent is checked",
            );
        }
        for capture in ALWAYS_FF.captures_iter(&source) {
            let sensitivity = &capture["sensitivity"];
            let (positive, negative) = edges(sensitivity);
            let line = source[..capture.get(0).unwrap().start()]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count()
                + 1;
            sequential_blocks.push(SequentialBlock {
                path: path.clone(),
                line,
                sensitivity: sensitivity.split_whitespace().collect::<Vec<_>>().join(" "),
                clocks: positive.clone(),
                async_resets: negative.clone(),
            });
            let position = format!("{path}:{line}");
            if positive.len() != 1 || !clocks.contains(&positive[0]) {
                violation(
                    &mut violations,
                    "unknown-clock-domain",
                    &position,
                    format!(
                        "always_ff clocks {}, expected one of {}",
                        python_list(&positive),
                        python_list(&clocks.iter().cloned().collect::<Vec<_>>())
                    ),
                );
            }
            for reset in &negative {
                if !resets.contains(reset) {
                    violation(
                        &mut violations,
                        "unauthorized-async-reset",
                        &position,
                        format!("{reset} is not an approved reset-release path"),
                    );
                }
            }
            if negative.len() > 1 {
                violation(
                    &mut violations,
                    "multiple-async-resets",
                    &position,
                    format!("always_ff has async resets {}", python_list(&negative)),
                );
            }
        }
    }

    let mut synchronizers = Vec::new();
    for entry in &manifest.synchronizers {
        let original = sources.get(&entry.file).map(String::as_str).unwrap_or("");
        let source = strip_comments(original);
        let assignments = assignments(&source);
        let source_users: Vec<_> = assignments
            .iter()
            .filter(|(_, rhs)| token_present(rhs, &entry.source))
            .map(|(lhs, _)| lhs.clone())
            .collect();
        let first_stage_users: Vec<_> = assignments
            .iter()
            .filter(|(_, rhs)| token_present(rhs, &entry.first_stage))
            .map(|(lhs, _)| lhs.clone())
            .collect();
        let marked = attribute_marks(original, &entry.first_stage)
            && attribute_marks(original, &entry.second_stage);
        synchronizers.push(SynchronizerResult {
            file: entry.file.clone(),
            source: entry.source.clone(),
            first_stage: entry.first_stage.clone(),
            second_stage: entry.second_stage.clone(),
            source_users: source_users.clone(),
            first_stage_users: first_stage_users.clone(),
            marked,
        });
        if source.is_empty() {
            violation(
                &mut violations,
                "missing-synchronizer-source",
                &entry.file,
                format!("source file for {} is missing", entry.source),
            );
            continue;
        }
        if source_users != [entry.first_stage.clone()] {
            violation(
                &mut violations,
                "async-source-fanout",
                &entry.file,
                format!(
                    "{} assignment users are {}",
                    entry.source,
                    python_list(&source_users)
                ),
            );
        }
        if first_stage_users != [entry.second_stage.clone()] {
            violation(
                &mut violations,
                "first-stage-fanout",
                &entry.file,
                format!(
                    "{} assignment users are {}",
                    entry.first_stage,
                    python_list(&first_stage_users)
                ),
            );
        }
        let control = format!(
            r"\b(?:if|case|while)\s*\([^)]*\b{}\b",
            regex::escape(&entry.source)
        );
        if Regex::new(&control).unwrap().is_match(&source) {
            violation(
                &mut violations,
                "async-source-control-use",
                &entry.file,
                format!("{} is used directly in control logic", entry.source),
            );
        }
        if entry.require_attribute && !marked {
            violation(
                &mut violations,
                "missing-synchronizer-attribute",
                &entry.file,
                format!(
                    "{}/{} are not both marked",
                    entry.first_stage, entry.second_stage
                ),
            );
        }
    }

    let mut reset_primitive_result = json!({"status": "not-reviewed"});
    if let Some(entry) = &manifest.reset_synchronizer_primitive {
        let original = sources.get(&entry.file).map(String::as_str).unwrap_or("");
        let source = strip_comments(original);
        let assignments = assignments(&source);
        let values = |signal: &str| -> Vec<String> {
            assignments
                .iter()
                .filter(|(lhs, _)| lhs == signal)
                .map(|(_, rhs)| rhs.split_whitespace().collect::<Vec<_>>().join(" "))
                .collect()
        };
        let users = |signal: &str| -> Vec<String> {
            assignments
                .iter()
                .filter(|(_, rhs)| token_present(rhs, signal))
                .map(|(lhs, _)| lhs.clone())
                .collect()
        };
        let first_stage_values = values(&entry.first_stage);
        let second_stage_values = values(&entry.second_stage);
        let first_stage_users = users(&entry.first_stage);
        let second_stage_users = users(&entry.second_stage);
        let marked = attribute_marks(original, &entry.first_stage)
            && attribute_marks(original, &entry.second_stage);
        let sensitivity_verified = ALWAYS_FF.captures_iter(&source).any(|capture| {
            let (positive, negative) = edges(&capture["sensitivity"]);
            positive == ["CLK"] && negative == [entry.async_source.as_str()]
        });
        let valid = first_stage_values == ["1'b0", "1'b1"]
            && second_stage_values == ["1'b0", entry.first_stage.as_str()]
            && first_stage_users == [entry.second_stage.as_str()]
            && second_stage_users == [entry.output.as_str()]
            && sensitivity_verified
            && (!entry.require_attribute || marked);
        reset_primitive_result = serde_json::to_value(ResetPrimitiveResult {
            file: entry.file.clone(),
            async_source: entry.async_source.clone(),
            first_stage: entry.first_stage.clone(),
            second_stage: entry.second_stage.clone(),
            output: entry.output.clone(),
            first_stage_values,
            second_stage_values,
            first_stage_users,
            second_stage_users,
            marked,
            sensitivity_verified,
            status: if valid { "verified" } else { "invalid" },
        })
        .unwrap();
        if !valid {
            violation(
                &mut violations,
                "invalid-reset-synchronizer-primitive",
                &entry.file,
                "async-assert/sync-release topology or attributes changed",
            );
        }
    }

    let mut reset_release_instances = Vec::new();
    for entry in &manifest.reset_release_instances {
        let source = strip_comments(sources.get(&entry.file).map(String::as_str).unwrap_or(""));
        let instance_pattern = format!(
            r"(?s)\barm7tdmis_reset_sync\s+{}\s*\((?P<ports>.*?)\)\s*;",
            regex::escape(&entry.instance)
        );
        let valid = Regex::new(&instance_pattern)
            .unwrap()
            .captures(&source)
            .is_some_and(|capture| {
                let ports = &capture["ports"];
                let input_pattern =
                    format!(r"\.nRESET\s*\(\s*{}\s*\)", regex::escape(&entry.source));
                let output_pattern = format!(
                    r"\.core_nreset\s*\(\s*{}\s*\)",
                    regex::escape(&entry.released_reset)
                );
                Regex::new(&input_pattern).unwrap().is_match(ports)
                    && Regex::new(&output_pattern).unwrap().is_match(ports)
            });
        reset_release_instances.push(ResetReleaseResult {
            file: entry.file.clone(),
            instance: entry.instance.clone(),
            source: entry.source.clone(),
            released_reset: entry.released_reset.clone(),
            status: if valid { "verified" } else { "invalid" },
        });
        if !valid {
            violation(
                &mut violations,
                "invalid-reset-release-instance",
                &entry.file,
                format!(
                    "{} does not map {} to {}",
                    entry.instance, entry.source, entry.released_reset
                ),
            );
        }
    }

    let async_reset_count = sequential_blocks
        .iter()
        .filter(|block| !block.async_resets.is_empty())
        .count();
    if let Some(expected) = manifest.expected_sequential_block_count {
        if sequential_blocks.len() != expected {
            violation(
                &mut violations,
                "sequential-inventory-change",
                "rtl/",
                format!(
                    "found {} always_ff blocks, expected {expected}",
                    sequential_blocks.len()
                ),
            );
        }
    }
    if let Some(expected) = manifest.expected_async_reset_block_count {
        if async_reset_count != expected {
            violation(
                &mut violations,
                "async-reset-inventory-change",
                "rtl/",
                format!("found {async_reset_count} async-reset blocks, expected {expected}"),
            );
        }
    }

    Analysis {
        clock_domains: clocks.into_iter().collect(),
        sequential_block_count: sequential_blocks.len(),
        async_reset_block_count: async_reset_count,
        sequential_blocks,
        synchronizer_count: synchronizers.len(),
        synchronizers,
        reset_synchronizer_primitive: reset_primitive_result,
        reset_release_count: reset_release_instances.len(),
        reset_release_instances,
        violations,
    }
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn rtl_sources(
    directory: &Path,
    root: &Path,
    sources: &mut BTreeMap<String, String>,
) -> Result<(), String> {
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.is_dir() {
            rtl_sources(&path, root, sources)?;
        } else if path.extension().is_some_and(|extension| extension == "sv") {
            let relative = path.strip_prefix(root).map_err(|error| error.to_string())?;
            let contents = fs::read_to_string(&path).map_err(|error| error.to_string())?;
            sources.insert(relative.to_string_lossy().replace('\\', "/"), contents);
        }
    }
    Ok(())
}

fn build_report(root: &Path, manifest_path: &Path) -> Result<Value, String> {
    let manifest_bytes = fs::read(manifest_path).map_err(|error| error.to_string())?;
    let manifest: Manifest =
        serde_json::from_slice(&manifest_bytes).map_err(|error| error.to_string())?;
    if manifest.schema.as_deref() != Some(MANIFEST_SCHEMA) {
        return Err("CDC/RDC manifest has wrong schema".into());
    }
    let mut sources = BTreeMap::new();
    rtl_sources(&root.join("rtl"), root, &mut sources)?;
    let analysis = analyze_sources(&sources, &manifest);
    let status_text = git(root, &["status", "--porcelain=v1", "--untracked-files=all"])?;
    let checker = root.join("verification/constrained_random_rs/src/cdc_rdc.rs");
    let mut inputs = BTreeMap::new();
    for (path, contents) in &sources {
        inputs.insert(
            path.clone(),
            json!({
                "bytes": contents.len(),
                "sha256": format!("{:x}", Sha256::digest(contents.as_bytes())),
            }),
        );
    }
    let manifest_relative = manifest_path
        .strip_prefix(root)
        .map_err(|error| error.to_string())?;
    let report = json!({
        "schema": REPORT_SCHEMA,
        "created_utc": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, false),
        "status": if analysis.violations.is_empty() { "passed" } else { "failed" },
        "git": {
            "commit": git(root, &["rev-parse", "HEAD"])?,
            "dirty": !status_text.is_empty(),
            "status": status_text.lines().collect::<Vec<_>>(),
        },
        "manifest": {
            "path": manifest_relative.to_string_lossy().replace('\\', "/"),
            "sha256": format!("{:x}", Sha256::digest(&manifest_bytes)),
            "reviewed_reset_contracts": manifest.reviewed_reset_contracts,
        },
        "checker": {
            "path": "verification/constrained_random_rs/src/cdc_rdc.rs",
            "bytes": fs::metadata(&checker).map_err(|error| error.to_string())?.len(),
            "sha256": file_sha256(&checker)?,
        },
        "inputs": inputs,
        "clock_domains": analysis.clock_domains,
        "sequential_block_count": analysis.sequential_block_count,
        "async_reset_block_count": analysis.async_reset_block_count,
        "sequential_blocks": analysis.sequential_blocks,
        "synchronizer_count": analysis.synchronizer_count,
        "synchronizers": analysis.synchronizers,
        "reset_synchronizer_primitive": analysis.reset_synchronizer_primitive,
        "reset_release_count": analysis.reset_release_count,
        "reset_release_instances": analysis.reset_release_instances,
        "violations": analysis.violations,
    });
    Ok(report)
}

fn validate_report(report: &Value) -> Result<(), String> {
    if report["schema"] != REPORT_SCHEMA {
        return Err("CDC/RDC report has wrong schema".into());
    }
    if report["status"] != "passed"
        || report["violations"]
            .as_array()
            .is_none_or(|items| !items.is_empty())
    {
        return Err("CDC/RDC report contains structural violations".into());
    }
    if report["clock_domains"] != json!(["CLK"]) {
        return Err("CDC/RDC report does not prove the single-clock design".into());
    }
    if report["synchronizer_count"].as_u64().unwrap_or(0) < 6 {
        return Err("CDC/RDC report lost an event synchronizer".into());
    }
    if report["reset_release_count"].as_u64().unwrap_or(0) < 2 {
        return Err("CDC/RDC report lost a reset-release synchronizer".into());
    }
    if report["reset_synchronizer_primitive"]["status"] != "verified" {
        return Err("CDC/RDC report lost reset synchronizer topology".into());
    }
    Ok(())
}

fn write_report(path: &Path, report: &Value) -> Result<(), String> {
    let parent = path.parent().ok_or("CDC/RDC output path has no parent")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    serde_json::to_writer_pretty(&mut temporary, report).map_err(|error| error.to_string())?;
    temporary
        .write_all(b"\n")
        .map_err(|error| error.to_string())?;
    temporary.persist(path).map_err(|error| error.to_string())?;
    Ok(())
}

pub fn command(arguments: &[String]) -> Result<String, String> {
    let root = repo_root();
    let mut manifest = root.join("verification/cdc_rdc_manifest.json");
    let mut output = root.join("reports/generated/cdc-rdc.json");
    let mut check = false;
    let mut args = arguments.iter();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--manifest" => {
                manifest = PathBuf::from(args.next().ok_or("missing --manifest value")?)
            }
            "--output" => output = PathBuf::from(args.next().ok_or("missing --output value")?),
            "--check" => check = true,
            other => return Err(format!("unknown cdc-rdc argument: {other}")),
        }
    }
    let current = std::env::current_dir().map_err(|error| error.to_string())?;
    if !manifest.is_absolute() {
        manifest = current.join(manifest);
    }
    if !output.is_absolute() {
        output = current.join(output);
    }
    let report = build_report(&root, &manifest)?;
    write_report(&output, &report)?;
    if check {
        validate_report(&report)?;
    }
    let status = report["status"]
        .as_str()
        .unwrap_or("failed")
        .to_ascii_uppercase();
    let violations = report["violations"].as_array().map_or(0, Vec::len);
    let line = format!(
        "[cdc-rdc] {status} ({} sequential blocks, {} event synchronizers, {violations} violations)",
        report["sequential_block_count"], report["synchronizer_count"]
    );
    if report["status"] != "passed" {
        return Err(line);
    }
    Ok(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reviewed_tree_has_one_clock_and_verified_reset_topology() {
        let root = repo_root();
        let report = build_report(&root, &root.join("verification/cdc_rdc_manifest.json")).unwrap();
        validate_report(&report).unwrap();
        assert_eq!(report["sequential_block_count"], 37);
        assert_eq!(report["async_reset_block_count"], 15);
        assert_eq!(report["synchronizer_count"], 6);
    }

    #[test]
    fn rejects_extra_clock_reset_and_meta_fanout() {
        let safe = "module crossing(input logic CLK, RESET_N, EVENT_ASYNC, output logic EVENT_SYNC);\nlogic event_meta_q;\nalways_ff @(posedge CLK or negedge RESET_N) begin\nif (!RESET_N) begin event_meta_q <= 1'b0; EVENT_SYNC <= 1'b0; end\nelse begin event_meta_q <= EVENT_ASYNC; EVENT_SYNC <= event_meta_q; end\nend\nendmodule\n";
        let manifest: Manifest = serde_json::from_value(json!({
            "clock_domains": ["CLK"],
            "allowed_async_resets": ["RESET_N"],
            "synchronizers": [{"file": "rtl/crossing.sv", "source": "EVENT_ASYNC", "first_stage": "event_meta_q", "second_stage": "EVENT_SYNC"}],
        })).unwrap();
        let sources = |text: &str| BTreeMap::from([("rtl/crossing.sv".into(), text.into())]);
        assert!(analyze_sources(&sources(safe), &manifest)
            .violations
            .is_empty());
        let variants = [
            safe.replace("posedge CLK", "posedge AUX_CLK"),
            safe.replace("negedge RESET_N", "negedge reset_comb"),
            safe.replace("endmodule", "assign unsafe = event_meta_q;\nendmodule"),
        ];
        for variant in variants {
            assert!(!analyze_sources(&sources(&variant), &manifest)
                .violations
                .is_empty());
        }
    }
}
