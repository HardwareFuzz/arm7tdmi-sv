//! Bidirectional release traceability for the canonical TASKS.md ledger.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::campaign::{git_text, repo_root, sha256, utc_now, write_json, Failure};

const REPORT_SCHEMA: &str = "arm7tdmis-traceability-v1";
const MAPPING_SCHEMA: &str = "arm7tdmis-traceability-map-v1";

#[derive(Deserialize)]
struct Mapping {
    schema: String,
    prefix_defaults: BTreeMap<String, Evidence>,
    #[serde(default)]
    requirement_overrides: BTreeMap<String, Evidence>,
    #[serde(default)]
    rtl_rules: Vec<Rule>,
    #[serde(default)]
    verification_rules: Vec<Rule>,
}

#[derive(Default, Deserialize)]
struct Evidence {
    #[serde(default)]
    rtl: Vec<String>,
    #[serde(default)]
    tests: Vec<String>,
    #[serde(default)]
    source_sections: Vec<String>,
}

#[derive(Deserialize)]
struct Rule {
    glob: String,
    requirements: Vec<String>,
}

struct Requirement {
    ledger_status: &'static str,
    task_section: String,
    body: String,
}

fn failure(message: impl Into<String>) -> Failure {
    Failure::other(message)
}

fn load_json(path: &Path) -> Result<Value, Failure> {
    serde_json::from_slice(&fs::read(path)?).map_err(|error| failure(error.to_string()))
}

fn tracked_files() -> Result<Vec<String>, Failure> {
    let root = repo_root();
    // Include candidate source files before they are staged, and never credit a
    // deleted file as verification evidence.  `git ls-files` by itself does
    // the opposite for both cases while a migration is in progress.
    let listing = git_text(&["ls-files", "--cached", "--others", "--exclude-standard"])?;
    Ok(existing_source_files(&root, &listing))
}

fn existing_source_files(root: &Path, listing: &str) -> Vec<String> {
    let mut files = listing
        .lines()
        .filter(|line| !line.is_empty() && root.join(line).is_file())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    files.sort();
    files.dedup();
    files
}

fn requirement_entries(tasks: &str) -> BTreeMap<String, Requirement> {
    let section_pattern = Regex::new(r"^## (31(?:\.\d+)? .+)$").unwrap();
    let requirement_pattern = Regex::new(r"^- \[([ xX])\] \*\*([A-Z]+-\d{3}):\*\*").unwrap();
    let mut requirements = BTreeMap::new();
    let mut section = String::new();
    let mut active: Option<(String, Vec<String>)> = None;

    let finish = |active: &mut Option<(String, Vec<String>)>,
                  requirements: &mut BTreeMap<String, Requirement>| {
        if let Some((id, body)) = active.take() {
            if let Some(entry) = requirements.get_mut(&id) {
                entry.body = body.join("\n").trim().to_owned();
            }
        }
    };

    for line in tasks.lines() {
        if let Some(capture) = section_pattern.captures(line) {
            finish(&mut active, &mut requirements);
            section = capture[1].to_owned();
            continue;
        }
        if let Some(capture) = requirement_pattern.captures(line) {
            finish(&mut active, &mut requirements);
            let id = capture[2].to_owned();
            requirements.insert(
                id.clone(),
                Requirement {
                    ledger_status: if capture[1].eq_ignore_ascii_case("x") {
                        "checked"
                    } else {
                        "open"
                    },
                    task_section: section.clone(),
                    body: String::new(),
                },
            );
            active = Some((id, vec![line.to_owned()]));
            continue;
        }
        if line.starts_with("## ") {
            finish(&mut active, &mut requirements);
        } else if let Some((_, body)) = active.as_mut() {
            body.push(line.to_owned());
        }
    }
    finish(&mut active, &mut requirements);
    requirements
}

// Python fnmatchcase's '*' spans directory separators. The reviewed map uses
// literals and '*'; support '?' and character classes for future rules too.
fn glob_regex(glob: &str) -> Result<Regex, Failure> {
    let chars: Vec<char> = glob.chars().collect();
    let mut expression = String::from("(?s)\\A");
    let mut position = 0;
    while position < chars.len() {
        match chars[position] {
            '*' => expression.push_str(".*"),
            '?' => expression.push('.'),
            '[' => {
                let mut end = position + 1;
                if end < chars.len() && (chars[end] == '!' || chars[end] == '^') {
                    end += 1;
                }
                if end < chars.len() && chars[end] == ']' {
                    end += 1;
                }
                while end < chars.len() && chars[end] != ']' {
                    end += 1;
                }
                if end == chars.len() {
                    expression.push_str(r"\[");
                } else {
                    expression.push('[');
                    let mut inner = position + 1;
                    if chars[inner] == '!' {
                        expression.push('^');
                        inner += 1;
                    } else if chars[inner] == '^' {
                        expression.push_str(r"\^");
                        inner += 1;
                    }
                    for character in &chars[inner..end] {
                        if *character == '\\' {
                            expression.push_str(r"\\");
                        } else {
                            expression.push(*character);
                        }
                    }
                    expression.push(']');
                    position = end;
                }
            }
            character => expression.push_str(&regex::escape(&character.to_string())),
        }
        position += 1;
    }
    expression.push_str(r"\z");
    Regex::new(&expression).map_err(|error| failure(error.to_string()))
}

fn matching_requirements(path: &str, rules: &[(Regex, Vec<String>)]) -> Vec<String> {
    let mut mapped = BTreeSet::new();
    for (pattern, requirements) in rules {
        if pattern.is_match(path) {
            mapped.extend(requirements.iter().cloned());
        }
    }
    mapped.into_iter().collect()
}

fn unique_strings(values: impl IntoIterator<Item = String>) -> Vec<String> {
    values
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn explicit_evidence_paths(
    body: &str,
    tracked: &BTreeSet<String>,
    roots: &[&str],
    suffixes: &[&str],
) -> Vec<String> {
    let pattern =
        Regex::new(r"`((?:rtl|tb|verification|scripts|fpga|examples|docs)/[^` ]+)`").unwrap();
    unique_strings(pattern.captures_iter(body).filter_map(|capture| {
        let path = capture[1].to_owned();
        (tracked.contains(&path)
            && roots.iter().any(|root| path.starts_with(root))
            && suffixes.iter().any(|suffix| path.ends_with(suffix)))
        .then_some(path)
    }))
}

fn phase_names_from_paths<'a>(paths: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut phases = BTreeSet::new();
    for path in paths {
        let name = path.rsplit('/').next().unwrap_or(path);
        if path.starts_with("tb/unit/") {
            if let Some(test) = name.strip_suffix("_tb.sv") {
                phases.insert(format!("unit-{test}"));
            }
        } else if path.starts_with("tb/integration/") {
            if let Some(test) = name.strip_suffix("_tb.sv") {
                if name == "arm7tdmis_public_suite_tb.sv" {
                    phases.insert("public-suite".to_owned());
                } else {
                    phases.insert(format!(
                        "integration-{}",
                        test.strip_prefix("arm7tdmis_").unwrap_or(test)
                    ));
                }
            }
        }
    }
    phases.into_iter().collect()
}

fn regression_context(path: &Path) -> Option<Value> {
    if !path.is_file() {
        return None;
    }
    let report = load_json(path).ok()?;
    (report["schema"] == "arm7tdmis-regression-v1").then_some(report)
}

fn python_string(value: Option<&Value>, default: &str) -> String {
    match value {
        None => default.to_owned(),
        Some(Value::Null) => "None".to_owned(),
        Some(Value::Bool(true)) => "True".to_owned(),
        Some(Value::Bool(false)) => "False".to_owned(),
        Some(Value::String(text)) => text.clone(),
        Some(value) => value.to_string(),
    }
}

fn latest_result(
    ledger_status: &str,
    expected_phases: &[String],
    regression: Option<&Value>,
) -> String {
    if ledger_status == "open" {
        return "OPEN - no accepted sign-off result".to_owned();
    }
    let Some(report) = regression else {
        return "ledger checked; no local regression report supplied".to_owned();
    };
    let mut results = BTreeMap::new();
    if let Some(rows) = report["results"].as_array() {
        for row in rows {
            if row.is_object() {
                results.insert(
                    python_string(row.get("name"), "None"),
                    python_string(row.get("status"), "None"),
                );
            }
        }
    }
    let matched: Vec<String> = expected_phases
        .iter()
        .filter_map(|phase| results.get(phase).map(|status| format!("{phase}={status}")))
        .collect();
    let commit = python_string(report["git"].get("commit"), "unknown");
    let commit: String = commit.chars().take(12).collect();
    let aggregate = python_string(report.get("status"), "unknown");
    if matched.is_empty() {
        format!("ledger checked; regression {commit} {aggregate} has no cited phase")
    } else {
        format!("regression {commit} {aggregate}; {}", matched.join(", "))
    }
}

fn extension_is(path: &str, suffixes: &[&str]) -> bool {
    Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| suffixes.contains(&extension))
}

fn verification_files(tracked: &[String]) -> Vec<String> {
    tracked
        .iter()
        .filter(|path| {
            (path.starts_with("tb/") && extension_is(path, &["sv", "svh", "hex"]))
                || (path.starts_with("verification/")
                    && extension_is(path, &["sv", "py", "S", "c", "ld", "rs", "toml", "lock"]))
                || (path.starts_with("scripts/tests/test_") && extension_is(path, &["py", "rs"]))
        })
        .cloned()
        .collect()
}

fn compile_rules(rules: &[Rule]) -> Result<Vec<(Regex, Vec<String>)>, Failure> {
    rules
        .iter()
        .map(|rule| Ok((glob_regex(&rule.glob)?, rule.requirements.clone())))
        .collect()
}

fn build_report(mapping_path: &Path, regression_path: &Path) -> Result<Value, Failure> {
    let root = repo_root();
    let tasks_path = root.join("TASKS.md");
    let mapping: Mapping = serde_json::from_value(load_json(mapping_path)?)
        .map_err(|error| failure(error.to_string()))?;
    if mapping.schema != MAPPING_SCHEMA {
        return Err(failure("traceability mapping has wrong schema"));
    }
    let tasks = fs::read_to_string(&tasks_path)?;
    let requirements = requirement_entries(&tasks);
    let tracked = tracked_files()?;
    let tracked_set: BTreeSet<String> = tracked.iter().cloned().collect();
    let regression = regression_context(regression_path);
    let rtl_rules = compile_rules(&mapping.rtl_rules)?;
    let verification_rules = compile_rules(&mapping.verification_rules)?;

    let known_ids: BTreeSet<String> = requirements.keys().cloned().collect();
    let mut referenced_ids: BTreeSet<String> =
        mapping.requirement_overrides.keys().cloned().collect();
    for rule in mapping.rtl_rules.iter().chain(&mapping.verification_rules) {
        referenced_ids.extend(rule.requirements.iter().cloned());
    }

    let rtl_files: Vec<String> = tracked
        .iter()
        .filter(|path| path.starts_with("rtl/") && extension_is(path, &["sv", "svh"]))
        .cloned()
        .collect();
    let verification_files = verification_files(&tracked);
    let rtl_reverse: BTreeMap<String, Vec<String>> = rtl_files
        .iter()
        .map(|path| (path.clone(), matching_requirements(path, &rtl_rules)))
        .collect();
    let verification_reverse: BTreeMap<String, Vec<String>> = verification_files
        .iter()
        .map(|path| {
            (
                path.clone(),
                matching_requirements(path, &verification_rules),
            )
        })
        .collect();

    let mut rows = BTreeMap::new();
    let mut checked_count = 0_usize;
    for (id, entry) in &requirements {
        let prefix = id.split_once('-').map_or(id.as_str(), |(prefix, _)| prefix);
        let defaults = mapping
            .prefix_defaults
            .get(prefix)
            .ok_or_else(|| failure(format!("no prefix defaults for {id}")))?;
        let override_values = mapping.requirement_overrides.get(id);
        let explicit_rtl = explicit_evidence_paths(
            &entry.body,
            &tracked_set,
            &["rtl/", "fpga/", "examples/"],
            &[".sv", ".svh", ".qsf", ".sdc", ".qip", ".f"],
        );
        let explicit_tests = explicit_evidence_paths(
            &entry.body,
            &tracked_set,
            &["tb/", "verification/", "scripts/"],
            &[".sv", ".py", ".S", ".c", ".ld", ".rs"],
        );
        let rtl = unique_strings(
            defaults
                .rtl
                .iter()
                .chain(&explicit_rtl)
                .chain(override_values.into_iter().flat_map(|entry| &entry.rtl))
                .cloned(),
        );
        let tests = unique_strings(
            defaults
                .tests
                .iter()
                .chain(&explicit_tests)
                .chain(override_values.into_iter().flat_map(|entry| &entry.tests))
                .cloned(),
        );
        let phases = phase_names_from_paths(tests.iter().map(String::as_str));
        let source_sections = unique_strings(
            std::iter::once(&entry.task_section)
                .chain(&defaults.source_sections)
                .chain(
                    override_values
                        .into_iter()
                        .flat_map(|entry| &entry.source_sections),
                )
                .cloned(),
        );
        if entry.ledger_status == "checked" {
            checked_count += 1;
        }
        rows.insert(
            id.clone(),
            json!({
                "source_sections": source_sections,
                "rtl": rtl,
                "tests": tests,
                "coverage_bins": [format!("requirement:{id}:acceptance")],
                "expected_regression_phases": phases,
                "latest_result": latest_result(entry.ledger_status, &phases, regression.as_ref()),
                "ledger_status": entry.ledger_status,
            }),
        );
    }

    let mapping_relative = mapping_path
        .strip_prefix(&root)
        .map_err(|error| failure(error.to_string()))?;
    let regression_relative = if regression.is_some() {
        regression_path
            .strip_prefix(&root)
            .ok()
            .map(|path| path.to_string_lossy().into_owned())
    } else {
        None
    };
    let status = git_text(&["status", "--porcelain=v1", "--untracked-files=all"])?;
    Ok(json!({
        "schema": REPORT_SCHEMA,
        "created_utc": utc_now(),
        "git": {
            "commit": git_text(&["rev-parse", "HEAD"])?,
            "dirty": !status.is_empty(),
        },
        "inputs": {
            "tasks": {"path": "TASKS.md", "sha256": sha256(&tasks_path)?},
            "mapping": {
                "path": mapping_relative.to_string_lossy(),
                "sha256": sha256(mapping_path)?,
            },
            "regression": regression_relative,
        },
        "requirements": rows,
        "rtl_to_requirements": rtl_reverse,
        "verification_to_requirements": verification_reverse,
        "unmapped_rtl": rtl_reverse.iter().filter_map(|(path, ids)| ids.is_empty().then_some(path)).collect::<Vec<_>>(),
        "unmapped_verification": verification_reverse.iter().filter_map(|(path, ids)| ids.is_empty().then_some(path)).collect::<Vec<_>>(),
        "unknown_requirement_references": referenced_ids.difference(&known_ids).collect::<Vec<_>>(),
        "summary": {
            "requirement_count": rows.len(),
            "checked_requirement_count": checked_count,
            "open_requirement_count": rows.len() - checked_count,
            "rtl_file_count": rtl_reverse.len(),
            "verification_file_count": verification_reverse.len(),
        },
    }))
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_i64() != Some(0),
        Value::String(value) => !value.is_empty(),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
    }
}

fn validate_report(report: &Value) -> Result<(), Failure> {
    let mut errors = Vec::new();
    if report["schema"] != REPORT_SCHEMA {
        errors.push("wrong report schema".to_owned());
    }
    for (name, label) in [
        ("unmapped_rtl", "unmapped RTL"),
        ("unmapped_verification", "unmapped verification"),
        ("unknown_requirement_references", "unknown requirements"),
    ] {
        if let Some(items) = report[name].as_array() {
            if !items.is_empty() {
                errors.push(format!(
                    "{label}: {}",
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
    }
    if let Some(rows) = report["requirements"].as_object() {
        for (id, row) in rows {
            for field in [
                "source_sections",
                "rtl",
                "tests",
                "coverage_bins",
                "latest_result",
                "ledger_status",
            ] {
                if !truthy(&row[field]) {
                    errors.push(format!("{id} has empty {field}"));
                }
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(failure(errors.join("; ")))
    }
}

fn absolute(path: &Path) -> Result<PathBuf, Failure> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    if let Ok(canonical) = fs::canonicalize(&path) {
        return Ok(canonical);
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    Ok(normalized)
}

pub fn traceability_report(arguments: &[String]) -> Result<(), String> {
    run(arguments).map_err(|error| error.to_string())
}

fn run(arguments: &[String]) -> Result<(), Failure> {
    let root = repo_root();
    let mut mapping = root.join("verification/traceability.json");
    let mut regression = root.join("reports/generated/regression.json");
    let mut output = root.join("reports/generated/traceability-report.json");
    let mut check = false;
    let mut position = 0;
    while position < arguments.len() {
        let name = &arguments[position];
        if name == "--check" {
            check = true;
            position += 1;
            continue;
        }
        let value = arguments
            .get(position + 1)
            .ok_or_else(|| failure(format!("missing value for {name}")))?;
        match name.as_str() {
            "--mapping" => mapping = PathBuf::from(value),
            "--regression" => regression = PathBuf::from(value),
            "--output" => output = PathBuf::from(value),
            _ => return Err(failure(format!("unknown argument: {name}"))),
        }
        position += 2;
    }
    let report = build_report(&absolute(&mapping)?, &absolute(&regression)?)?;
    if check {
        validate_report(&report)?;
    }
    write_json(&absolute(&output)?, &report)?;
    println!(
        "[traceability] PASS ({} requirements, {} RTL files, {} verification files)",
        report["summary"]["requirement_count"],
        report["summary"]["rtl_file_count"],
        report["summary"]["verification_file_count"],
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deleted_tracked_files_cannot_supply_evidence() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("new.rs"), "fn main() {}\n").unwrap();
        assert_eq!(
            existing_source_files(directory.path(), "removed.py\nnew.rs\nnew.rs\n"),
            ["new.rs"]
        );
    }

    #[test]
    fn checked_report_covers_every_ledger_requirement_and_tracked_source() {
        let root = repo_root();
        assert!(root
            .join("verification/constrained_random_rs/Cargo.toml")
            .is_file());
        assert!(root
            .join("verification/constrained_random_rs/src/traceability_report.rs")
            .is_file());
        assert!(root.join("verification/traceability.json").is_file());

        let temporary = tempfile::tempdir().unwrap();
        let output = temporary.path().join("traceability-report.json");
        run(&[
            "--check".to_owned(),
            "--output".to_owned(),
            output.to_string_lossy().into_owned(),
        ])
        .unwrap();
        let report = load_json(&output).unwrap();
        let tasks = fs::read_to_string(root.join("TASKS.md")).unwrap();
        let pattern = Regex::new(r"\*\*([A-Z]+-\d{3}):\*\*").unwrap();
        let expected: BTreeSet<String> = pattern
            .captures_iter(&tasks)
            .map(|capture| capture[1].to_owned())
            .collect();
        let actual: BTreeSet<String> = report["requirements"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(report["schema"], REPORT_SCHEMA);
        assert_eq!(actual, expected);
        assert_eq!(report["unmapped_rtl"], json!([]));
        assert_eq!(report["unmapped_verification"], json!([]));
        assert_eq!(report["unknown_requirement_references"], json!([]));
        for (id, row) in report["requirements"].as_object().unwrap() {
            for field in [
                "source_sections",
                "rtl",
                "tests",
                "coverage_bins",
                "latest_result",
                "ledger_status",
            ] {
                assert!(truthy(&row[field]), "{id} has empty {field}");
            }
        }
    }

    #[test]
    fn traceability_remains_mandatory_in_make_regression_release_and_documentation() {
        let root = repo_root();
        let makefile = fs::read_to_string(root.join("scripts/Makefile")).unwrap();
        let harness =
            fs::read_to_string(root.join("verification/constrained_random_rs/src/regression.rs"))
                .unwrap();
        let release = fs::read_to_string(
            root.join("verification/constrained_random_rs/src/release_evidence/files.rs"),
        )
        .unwrap();
        let documentation = fs::read_to_string(root.join("docs/TRACEABILITY.md")).unwrap();
        let tasks = fs::read_to_string(root.join("TASKS.md")).unwrap();
        assert!(makefile.contains("traceability:"));
        assert!(makefile.contains("$(RANDOM_RUST_BIN)) traceability-report"));
        assert!(harness.contains("\"traceability\""));
        assert!(release.contains("traceability-report.json"));
        assert!(documentation.contains("arm7tdmis-traceability-v1"));
        assert!(documentation.to_lowercase().contains("bidirectional"));
        assert!(documentation.contains("make -C scripts traceability"));
        assert!(tasks.contains("- [x] **DOC-003:**"));
    }

    #[test]
    fn parses_requirement_blocks_and_stops_at_other_sections() {
        let entries = requirement_entries(
            "## 31.2 Release\n- [x] **VER-001:** checked\n  `rtl/top/a.sv`\n\n## 32 Next\n- [ ] **DOC-001:** open\n",
        );
        assert_eq!(entries["VER-001"].task_section, "31.2 Release");
        assert!(entries["VER-001"].body.contains("`rtl/top/a.sv`"));
        assert!(!entries["VER-001"].body.contains("## 32"));
        assert_eq!(entries["DOC-001"].ledger_status, "open");
    }

    #[test]
    fn reverse_rules_match_across_directories_and_public_suite_has_its_own_phase() {
        let rule = glob_regex("tb/integration/*.sv").unwrap();
        assert!(rule.is_match("tb/integration/nested/example.sv"));
        assert!(!rule.is_match("tb/unit/example.sv"));
        assert!(glob_regex("tb/[ui]*/*.sv")
            .unwrap()
            .is_match("tb/unit/example.sv"));
        assert_eq!(
            phase_names_from_paths(["tb/integration/arm7tdmis_public_suite_tb.sv"]),
            ["public-suite"]
        );
    }

    #[test]
    fn validation_rejects_unmapped_and_unknown_requirements() {
        let report = json!({
            "schema": REPORT_SCHEMA,
            "unmapped_rtl": ["rtl/core/example.sv"],
            "unmapped_verification": [],
            "unknown_requirement_references": ["VAL-999"],
            "requirements": {
                "VAL-001": {
                    "source_sections": ["section"], "rtl": [], "tests": ["test"],
                    "coverage_bins": ["bin"], "latest_result": "result", "ledger_status": "checked",
                }
            },
        });
        let error = validate_report(&report).unwrap_err().to_string();
        assert!(error.contains("unmapped RTL: rtl/core/example.sv"));
        assert!(error.contains("unknown requirements: VAL-999"));
        assert!(error.contains("VAL-001 has empty rtl"));
    }

    #[test]
    fn regression_result_keeps_open_rows_open_and_names_matching_phases() {
        let report = json!({
            "schema": "arm7tdmis-regression-v1",
            "status": "passed",
            "git": {"commit": "1234567890abcdef"},
            "results": [
                {"name": "public-suite", "status": "passed"},
                {"name": "unit-regfile", "status": "failed"},
            ],
        });
        let phases = vec!["public-suite".to_owned(), "unit-regfile".to_owned()];
        assert_eq!(
            latest_result("checked", &phases, Some(&report)),
            "regression 1234567890ab passed; public-suite=passed, unit-regfile=failed"
        );
        assert_eq!(
            latest_result("open", &phases, Some(&report)),
            "OPEN - no accepted sign-off result"
        );
    }

    #[test]
    fn rust_verification_sources_are_in_reverse_scope() {
        let paths = vec![
            "verification/constrained_random_rs/src/traceability_report.rs".to_owned(),
            "verification/constrained_random_rs/Cargo.toml".to_owned(),
            "verification/constrained_random_rs/Cargo.lock".to_owned(),
            "verification/traceability.json".to_owned(),
        ];
        assert_eq!(verification_files(&paths), paths[..3]);
    }
}
