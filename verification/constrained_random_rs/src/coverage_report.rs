//! Measured Verilator/LCOV coverage summary. This is a structural measurement
//! for named tests, not an architectural or functional-coverage closure claim.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::campaign::{git_text, repo_root, run_command, sha256, utc_now, write_json, Failure};

const SCHEMA: &str = "arm7tdmis-coverage-v1";
const SOURCE_DIRECTORIES: [&str; 6] = ["rtl", "tb", "verification", "scripts", "fpga", "examples"];
const SOURCE_EXTENSIONS: [&str; 14] = [
    "sv", "svh", "f", "py", "sdc", "qsf", "qip", "tcl", "json", "S", "c", "ld", "rs", "toml",
];

fn failure(message: impl Into<String>) -> Failure {
    Failure::other(message)
}

fn include_source(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "Makefile")
        || path.extension().is_some_and(|extension| {
            SOURCE_EXTENSIONS
                .iter()
                .any(|candidate| extension == *candidate)
                || extension == "lock"
        })
}

fn collect_sources(directory: &Path, candidates: &mut Vec<PathBuf>) -> Result<(), Failure> {
    if !directory.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        if ["obj_dir", "__pycache__", "target"]
            .iter()
            .any(|ignored| name == *ignored)
        {
            continue;
        }
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect_sources(&path, candidates)?;
        } else if path.is_file() && include_source(&path) {
            candidates.push(path);
        }
    }
    Ok(())
}

pub(crate) fn source_digest(root: &Path) -> Result<String, Failure> {
    let mut candidates = Vec::new();
    for directory in SOURCE_DIRECTORIES {
        collect_sources(&root.join(directory), &mut candidates)?;
    }
    candidates.sort();
    let mut digest = Sha256::new();
    for path in candidates {
        let relative = path
            .strip_prefix(root)
            .map_err(|error| failure(error.to_string()))?;
        let relative = relative.to_string_lossy().replace('\\', "/");
        let bytes = relative.as_bytes();
        let length = u32::try_from(bytes.len()).map_err(|_| failure("source path is too long"))?;
        digest.update(length.to_be_bytes());
        digest.update(bytes);
        let contents = fs::read(path)?;
        digest.update((contents.len() as u64).to_be_bytes());
        digest.update(contents);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn field<'a>(body: &'a str, name: &str) -> &'a str {
    let marker = format!("\x01{name}\x02");
    body.find(&marker)
        .map(|position| {
            let tail = &body[position + marker.len()..];
            &tail[..tail.find('\x01').unwrap_or(tail.len())]
        })
        .unwrap_or("")
}

fn is_rtl_source(source: &str) -> bool {
    source
        .replace('\\', "/")
        .split('/')
        .any(|part| part == "rtl")
}

fn point_totals(raw_text: &str) -> Result<Value, Failure> {
    let pattern = Regex::new(r"^C '(.*)' ([0-9]+)$").unwrap();
    let mut all: BTreeMap<String, [u64; 2]> = BTreeMap::new();
    let mut rtl: BTreeMap<String, [u64; 2]> = BTreeMap::new();
    let mut parsed = 0_u64;
    for line in raw_text.lines().map(|line| line.trim_end_matches('\r')) {
        let Some(marker) = pattern.captures(line) else {
            continue;
        };
        let body = &marker[1];
        let point_type = match field(body, "t") {
            "" => "unknown",
            kind => kind,
        };
        let source = field(body, "f");
        let count: u64 = marker[2]
            .parse()
            .map_err(|_| failure("coverage count exceeds u64"))?;
        parsed += 1;
        let values = all.entry(point_type.to_owned()).or_default();
        values[1] += 1;
        if count > 0 {
            values[0] += 1;
        }
        if is_rtl_source(source) {
            let values = rtl.entry(point_type.to_owned()).or_default();
            values[1] += 1;
            if count > 0 {
                values[0] += 1;
            }
        }
    }
    if parsed == 0 {
        return Err(failure("raw coverage contains no Verilator points"));
    }
    let summary = |values: BTreeMap<String, [u64; 2]>| -> BTreeMap<String, Value> {
        values
            .into_iter()
            .map(|(kind, [covered, total])| (kind, json!({"covered": covered, "total": total})))
            .collect()
    };
    Ok(json!({"all": summary(all), "rtl": summary(rtl)}))
}

#[derive(Default)]
struct LcovRecord {
    line_total: u64,
    line_hit: u64,
    branch_total: u64,
    branch_hit: u64,
}

fn finish_record(
    source: &mut String,
    record: &mut LcovRecord,
    records: &mut BTreeMap<String, Value>,
) {
    if !source.is_empty() {
        records.insert(
            source.clone(),
            json!({
                "lines": {"covered": record.line_hit, "total": record.line_total},
                "branches": {"covered": record.branch_hit, "total": record.branch_total},
            }),
        );
    }
    source.clear();
    *record = LcovRecord::default();
}

fn lcov_totals(text: &str) -> Result<BTreeMap<String, Value>, Failure> {
    let mut records = BTreeMap::new();
    let mut source = String::new();
    let mut record = LcovRecord::default();
    for line in text.lines().map(|line| line.trim_end_matches('\r')) {
        if let Some(value) = line.strip_prefix("SF:") {
            finish_record(&mut source, &mut record, &mut records);
            source = value.to_owned();
        } else if let Some(value) = line.strip_prefix("LF:") {
            record.line_total = value
                .parse()
                .map_err(|_| failure("invalid LCOV LF count"))?;
        } else if let Some(value) = line.strip_prefix("LH:") {
            record.line_hit = value
                .parse()
                .map_err(|_| failure("invalid LCOV LH count"))?;
        } else if let Some(value) = line.strip_prefix("BRF:") {
            record.branch_total = value
                .parse()
                .map_err(|_| failure("invalid LCOV BRF count"))?;
        } else if let Some(value) = line.strip_prefix("BRH:") {
            record.branch_hit = value
                .parse()
                .map_err(|_| failure("invalid LCOV BRH count"))?;
        } else if line == "end_of_record" {
            finish_record(&mut source, &mut record, &mut records);
        }
    }
    finish_record(&mut source, &mut record, &mut records);
    if records.is_empty() {
        return Err(failure("LCOV input contains no source records"));
    }
    Ok(records)
}

fn report_path(path: &Path) -> Result<String, Failure> {
    let path = fs::canonicalize(path)?;
    Ok(path
        .strip_prefix(repo_root())
        .unwrap_or(&path)
        .to_string_lossy()
        .into_owned())
}

fn artifact(path: &Path) -> Result<Value, Failure> {
    Ok(json!({
        "path": report_path(path)?,
        "bytes": fs::metadata(path)?.len(),
        "sha256": sha256(path)?,
    }))
}

fn build_report(
    raw: &Path,
    lcov: &Path,
    tests: &[String],
    git_commit: &str,
    source_sha256: &str,
    tool_version: &str,
    git_dirty: bool,
) -> Result<Value, Failure> {
    let raw_text = fs::read_to_string(raw)?;
    let lcov_text = fs::read_to_string(lcov)?;
    let points = point_totals(&raw_text)?;
    let point_types: Vec<_> = points["all"].as_object().unwrap().keys().cloned().collect();
    Ok(json!({
        "schema": SCHEMA,
        "generated_utc": utc_now(),
        "git": {"commit": git_commit, "dirty": git_dirty, "source_sha256": source_sha256},
        "tool": tool_version,
        "instrumentation": ["--coverage"],
        "point_types": point_types,
        "tests": tests,
        "points": points,
        "lcov": lcov_totals(&lcov_text)?,
        "artifacts": {"raw": artifact(raw)?, "lcov": artifact(lcov)?},
        "claim": "Measured coverage for the listed tests only; this is not architectural or functional coverage closure.",
    }))
}

pub fn source_digest_command(arguments: &[String]) -> Result<String, String> {
    if !arguments.is_empty() {
        return Err("source-digest takes no arguments".into());
    }
    source_digest(&repo_root()).map_err(|error| error.to_string())
}

pub fn coverage_report(arguments: &[String]) -> Result<(), String> {
    run(arguments).map_err(|error| error.to_string())
}

fn run(arguments: &[String]) -> Result<(), Failure> {
    let raw = PathBuf::from(crate::option(arguments, "--raw").map_err(failure)?);
    let lcov = PathBuf::from(crate::option(arguments, "--lcov").map_err(failure)?);
    let output = PathBuf::from(crate::option(arguments, "--output").map_err(failure)?);
    let tests: Vec<String> = crate::option(arguments, "--tests")
        .map_err(failure)?
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    let status = git_text(&["status", "--porcelain=v1", "--untracked-files=all"])?;
    let tool = run_command(&["verilator_coverage".into(), "--version".into()], None)?;
    let report = build_report(
        &raw,
        &lcov,
        &tests,
        &git_text(&["rev-parse", "HEAD"])?,
        &source_digest(&repo_root())?,
        tool.trim(),
        !status.is_empty(),
    )?;
    write_json(&output, &report)?;
    let covered = report["points"]["rtl"]["line"]["covered"]
        .as_u64()
        .unwrap_or(0);
    let total = report["points"]["rtl"]["line"]["total"]
        .as_u64()
        .unwrap_or(0);
    println!(
        "[coverage] wrote {} ({covered}/{total} RTL line points for listed tests)",
        output.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(source: &str, point_type: &str, count: u32) -> String {
        format!("C '\x01f\x02{source}\x01l\x0210\x01t\x02{point_type}\x01o\x02hit' {count}\n")
    }

    #[test]
    fn raw_and_lcov_counts_have_separate_all_and_rtl_scopes() {
        let raw = format!(
            "{}{}{}{}",
            point("rtl/core/example.sv", "line", 4),
            point("rtl/core/example.sv", "line", 0),
            point("rtl/core/example.sv", "toggle", 2),
            point("tb/example_tb.sv", "branch", 0)
        );
        let points = point_totals(&raw).unwrap();
        assert_eq!(points["all"]["line"], json!({"covered": 1, "total": 2}));
        assert_eq!(points["rtl"]["toggle"], json!({"covered": 1, "total": 1}));
        assert_eq!(points["all"]["branch"], json!({"covered": 0, "total": 1}));
        assert!(points["rtl"]["branch"].is_null());
        let lcov = "TN:verilator_coverage\nSF:rtl/core/example.sv\nLF:2\nLH:1\nBRF:1\nBRH:0\nend_of_record\n";
        assert_eq!(
            lcov_totals(lcov).unwrap()["rtl/core/example.sv"]["lines"],
            json!({"covered": 1, "total": 2})
        );
        assert!(point_totals("no points\n").is_err());
        assert!(lcov_totals("end_of_record\n").is_err());
    }

    #[test]
    fn source_digest_ignores_build_output_and_frames_paths_and_contents() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        fs::create_dir_all(root.join("scripts/obj_dir")).unwrap();
        fs::create_dir_all(root.join("verification/constrained_random_rs/src")).unwrap();
        fs::write(root.join("scripts/Makefile"), b"a").unwrap();
        fs::write(root.join("scripts/obj_dir/ignored.rs"), b"x").unwrap();
        fs::write(
            root.join("verification/constrained_random_rs/src/main.rs"),
            b"b",
        )
        .unwrap();
        let first = source_digest(root).unwrap();
        fs::write(root.join("scripts/obj_dir/ignored.rs"), b"changed").unwrap();
        assert_eq!(first, source_digest(root).unwrap());
        fs::write(
            root.join("verification/constrained_random_rs/src/main.rs"),
            b"changed",
        )
        .unwrap();
        assert_ne!(first, source_digest(root).unwrap());
    }

    #[test]
    fn report_preserves_measurement_scope_and_artifact_hashes() {
        let temporary = tempfile::tempdir().unwrap();
        let raw = temporary.path().join("coverage.dat");
        let lcov = temporary.path().join("coverage.info");
        fs::write(&raw, point("rtl/core/example.sv", "line", 1)).unwrap();
        fs::write(
            &lcov,
            "SF:rtl/core/example.sv\nLF:1\nLH:1\nBRF:0\nBRH:0\nend_of_record\n",
        )
        .unwrap();
        let report = build_report(
            &raw,
            &lcov,
            &["alpha".into(), "beta".into()],
            &"a".repeat(40),
            &"b".repeat(64),
            "Verilator unit",
            true,
        )
        .unwrap();
        assert_eq!(report["schema"], SCHEMA);
        assert_eq!(report["tests"], json!(["alpha", "beta"]));
        assert_eq!(report["git"]["dirty"], true);
        assert_eq!(report["artifacts"]["raw"]["sha256"], sha256(&raw).unwrap());
        assert_eq!(
            report["points"]["rtl"]["line"],
            json!({"covered": 1, "total": 1})
        );
        assert!(report["claim"]
            .as_str()
            .unwrap()
            .contains("not architectural or functional"));
    }
}
