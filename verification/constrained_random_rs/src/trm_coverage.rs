//! Revision-locked coverage inventory for ARM DDI 0234B, including release validation.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::{json, Value};

use crate::campaign::{git_text, repo_root, sha256, utc_now, write_json, Failure};

const MAP_SCHEMA: &str = "arm7tdmis-trm-coverage-map-v1";
const REPORT_SCHEMA: &str = "arm7tdmis-trm-coverage-v1";
const SOURCE_PATH: &str = "ARM_DDI_0234B_ARM7TDMI-S_r4p3_TRM.pdf";
const SOURCE_BYTES: u64 = 1_477_711;
const SOURCE_SHA256: &str = "bb9cd0e3f2b7e2fdca4ff961cdfc5c9c85c063842e491f8997a504d3241baa14";
const DISPOSITIONS: [&str; 5] = [
    "implemented-and-tested",
    "integration-obligation",
    "erratum-corrected",
    "external-out-of-scope",
    "open-requirement",
];
const SIGNALS: [&str; 45] = [
    "ABORT",
    "ADDR[31:0]",
    "CFGBIGEND",
    "CLK",
    "CLKEN",
    "CPA",
    "CPB",
    "CPnI",
    "CPnMREQ",
    "CPnOPC",
    "CPSEQ",
    "CPTBIT",
    "CPnTRANS",
    "DBGACK",
    "DBGBREAK",
    "DBGCOMMRX",
    "DBGCOMMTX",
    "DBGEN",
    "DBGnEXEC",
    "DBGEXT[1:0]",
    "DBGINSTRVALID",
    "DBGRNG[1:0]",
    "DBGRQ",
    "DBGTCKEN",
    "DBGTDI",
    "DBGTDO",
    "DBGnTDOEN",
    "DBGTMS",
    "DBGnTRST",
    "DMORE",
    "nFIQ",
    "nIRQ",
    "LOCK",
    "PROT[1:0]",
    "RDATA[31:0]",
    "nRESET",
    "SCANENABLE",
    "SCANIN",
    "SCANOUT",
    "SIZE[1:0]",
    "TRANS[1:0]",
    "VDD",
    "VSS",
    "WDATA[31:0]",
    "WRITE",
];

fn fail(message: impl Into<String>) -> Failure {
    Failure::other(message)
}

fn source_identity() -> Value {
    json!({
        "document": "ARM DDI 0234B",
        "revision": "ARM7TDMI-S r4p3",
        "path": SOURCE_PATH,
        "pages": 242,
        "bytes": SOURCE_BYTES,
        "sha256": SOURCE_SHA256,
    })
}

fn numbered(target: &mut BTreeSet<String>, prefix: &str, count: usize, separator: &str) {
    for index in 1..=count {
        target.insert(format!("{prefix}{separator}{index}"));
    }
}

fn expected_inventory() -> BTreeMap<&'static str, BTreeSet<String>> {
    let mut sections = BTreeSet::new();
    for (chapter, count) in [
        ("1", 5),
        ("2", 11),
        ("3", 6),
        ("4", 8),
        ("5", 27),
        ("6", 5),
        ("7", 20),
        ("8", 2),
        ("A", 1),
        ("B", 4),
    ] {
        numbered(&mut sections, chapter, count, ".");
    }
    for (parent, count) in [
        ("1.1", 3),
        ("1.2", 2),
        ("1.4", 2),
        ("1.5", 5),
        ("2.2", 1),
        ("2.3", 2),
        ("2.7", 4),
        ("2.8", 3),
        ("2.9", 10),
        ("2.10", 2),
        ("3.3", 5),
        ("3.4", 6),
        ("3.5", 4),
        ("4.1", 1),
        ("4.4", 7),
        ("4.5", 2),
        ("5.1", 1),
        ("5.2", 2),
        ("5.3", 5),
        ("5.4", 1),
        ("5.9", 2),
        ("5.10", 2),
        ("5.11", 2),
        ("5.12", 1),
        ("5.13", 5),
        ("5.14", 5),
        ("5.15", 1),
        ("5.16", 2),
        ("5.18", 6),
        ("5.19", 3),
        ("5.20", 3),
        ("5.21", 2),
        ("5.24", 3),
        ("5.26", 2),
        ("8.1", 5),
        ("B.4", 6),
    ] {
        numbered(&mut sections, parent, count, ".");
    }
    let mut tables = BTreeSet::new();
    for (chapter, count) in [
        ("1", 12),
        ("2", 4),
        ("3", 9),
        ("4", 4),
        ("5", 8),
        ("6", 1),
        ("7", 23),
        ("8", 1),
        ("A", 1),
        ("B", 2),
    ] {
        numbered(&mut tables, chapter, count, "-");
    }
    let mut figures = BTreeSet::new();
    for (chapter, count) in [("1", 4), ("2", 6), ("3", 7), ("4", 5), ("5", 17), ("8", 5)] {
        numbered(&mut figures, chapter, count, "-");
    }
    BTreeMap::from([
        ("sections", sections),
        ("tables", tables),
        ("figures", figures),
        ("signals", SIGNALS.into_iter().map(str::to_owned).collect()),
    ])
}

fn strings(value: &Value, label: &str, nonempty: bool) -> Result<Vec<String>, Failure> {
    let values = value
        .as_array()
        .ok_or_else(|| fail(format!("{label} is malformed")))?;
    values
        .iter()
        .map(|value| {
            let text = value
                .as_str()
                .ok_or_else(|| fail(format!("{label} is malformed")))?;
            if nonempty && text.is_empty() {
                return Err(fail(format!("{label} is malformed")));
            }
            Ok(text.to_owned())
        })
        .collect()
}

fn inventory_sets(manifest: &Value) -> Result<BTreeMap<String, BTreeSet<String>>, Failure> {
    let expected = expected_inventory();
    let actual = manifest["inventory"]
        .as_object()
        .ok_or_else(|| fail("TRM inventory categories changed"))?;
    if actual.keys().map(String::as_str).collect::<BTreeSet<_>>()
        != expected.keys().copied().collect()
    {
        return Err(fail("TRM inventory categories changed"));
    }
    expected
        .into_iter()
        .map(|(category, expected)| {
            let values = strings(&actual[category], &format!("{category} inventory"), true)?;
            let unique: BTreeSet<_> = values.iter().cloned().collect();
            if unique.len() != values.len() {
                return Err(fail(format!("{category} inventory contains duplicates")));
            }
            if unique != expected {
                let missing: Vec<_> = expected.difference(&unique).cloned().collect();
                let extra: Vec<_> = unique.difference(&expected).cloned().collect();
                return Err(fail(format!(
                    "{category} inventory mismatch; missing={missing:?}, extra={extra:?}"
                )));
            }
            Ok((category.to_owned(), unique))
        })
        .collect()
}

fn requirements(root: &Path) -> Result<BTreeMap<String, &'static str>, Failure> {
    let text = fs::read_to_string(root.join("TASKS.md"))?;
    let pattern = Regex::new(r"(?m)^- \[([ xX])\] \*\*([A-Z]+-\d{3}):\*\*")
        .map_err(|error| fail(error.to_string()))?;
    let mut result = BTreeMap::new();
    for match_ in pattern.captures_iter(&text) {
        result.insert(
            match_[2].to_owned(),
            if match_[1].eq_ignore_ascii_case("x") {
                "checked"
            } else {
                "open"
            },
        );
    }
    Ok(result)
}

fn selector_items(
    selector: &Value,
    inventory: &BTreeMap<String, BTreeSet<String>>,
) -> Result<BTreeSet<(String, String)>, Failure> {
    let category = selector["category"]
        .as_str()
        .ok_or_else(|| fail("coverage selector has unknown category"))?;
    let values = inventory.get(category).ok_or_else(|| {
        fail(format!(
            "coverage selector has unknown category: {category}"
        ))
    })?;
    let modes = ["items", "prefix", "all"]
        .iter()
        .filter(|key| selector.get(**key).is_some())
        .count();
    if modes != 1 {
        return Err(fail(
            "coverage selector must use exactly one selection mode",
        ));
    }
    let mut selected: BTreeSet<String> = if let Some(items) = selector.get("items") {
        let requested: BTreeSet<_> = strings(items, "coverage selector items", false)?
            .into_iter()
            .collect();
        if !requested.is_subset(values) {
            let unknown: Vec<_> = requested.difference(values).cloned().collect();
            return Err(fail(format!(
                "coverage selector references unknown {category}: {unknown:?}"
            )));
        }
        requested
    } else if let Some(prefix) = selector.get("prefix") {
        let prefix = prefix
            .as_str()
            .filter(|prefix| !prefix.is_empty())
            .ok_or_else(|| fail("coverage selector prefix is malformed"))?;
        values
            .iter()
            .filter(|value| value.starts_with(prefix))
            .cloned()
            .collect()
    } else {
        if selector["all"] != true {
            return Err(fail("coverage selector all mode must be true"));
        }
        values.clone()
    };
    let excluded: BTreeSet<_> = if let Some(exclude) = selector.get("exclude") {
        strings(exclude, "coverage selector exclusion", false)?
            .into_iter()
            .collect()
    } else {
        BTreeSet::new()
    };
    if !excluded.is_subset(&selected) {
        let unknown: Vec<_> = excluded.difference(&selected).cloned().collect();
        return Err(fail(format!(
            "coverage selector excludes unmatched items: {unknown:?}"
        )));
    }
    selected.retain(|value| !excluded.contains(value));
    if selected.is_empty() {
        return Err(fail("coverage selector matched no inventory items"));
    }
    Ok(selected
        .into_iter()
        .map(|value| (category.to_owned(), value))
        .collect())
}

fn validate_manifest(manifest: &Value, root: &Path) -> Result<Value, Failure> {
    if manifest["schema"] != MAP_SCHEMA {
        return Err(fail("TRM coverage manifest has wrong schema"));
    }
    if manifest["source"] != source_identity() {
        return Err(fail("TRM source identity changed"));
    }
    let manual = root.join(SOURCE_PATH);
    if !manual.is_file()
        || fs::metadata(&manual)?.len() != SOURCE_BYTES
        || sha256(&manual)? != SOURCE_SHA256
    {
        return Err(fail("TRM PDF identity does not match the audited revision"));
    }
    let inventory = inventory_sets(manifest)?;
    let requirements = requirements(root)?;
    let groups = manifest["coverage_groups"]
        .as_array()
        .filter(|groups| !groups.is_empty())
        .ok_or_else(|| fail("TRM coverage groups are missing"))?;

    let mut assigned: BTreeMap<(String, String), String> = BTreeMap::new();
    let mut disposition_counts: BTreeMap<&str, usize> =
        DISPOSITIONS.into_iter().map(|name| (name, 0)).collect();
    let mut group_ids = BTreeSet::new();
    let mut normalized_groups = Vec::new();
    for group in groups {
        if !group.is_object() {
            return Err(fail("TRM coverage group is malformed"));
        }
        let id = group["id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| fail("TRM coverage group ID is invalid or duplicate"))?;
        if !group_ids.insert(id.to_owned()) {
            return Err(fail(format!(
                "TRM coverage group ID is invalid or duplicate: {id}"
            )));
        }
        let disposition = group["disposition"]
            .as_str()
            .filter(|value| DISPOSITIONS.contains(value))
            .ok_or_else(|| fail(format!("{id} has unknown disposition")))?;
        if group["rationale"]
            .as_str()
            .is_none_or(|value| value.trim().is_empty())
        {
            return Err(fail(format!("{id} has no rationale")));
        }
        let evidence = strings(&group["evidence"], &format!("{id} evidence"), true)?;
        if evidence.is_empty() {
            return Err(fail(format!("{id} has no evidence paths")));
        }
        for path in &evidence {
            let evidence_path = root.join(path);
            if !evidence_path.is_file()
                || !evidence_path
                    .canonicalize()?
                    .starts_with(root.canonicalize()?)
            {
                return Err(fail(format!("{id} evidence path is missing: {path}")));
            }
        }
        let requirement_ids = strings(&group["requirements"], &format!("{id} requirements"), true)?;
        if requirement_ids.is_empty() {
            return Err(fail(format!("{id} has no requirement references")));
        }
        let unknown: Vec<_> = requirement_ids
            .iter()
            .filter(|id| !requirements.contains_key(*id))
            .cloned()
            .collect();
        if !unknown.is_empty() {
            return Err(fail(format!(
                "{id} references unknown requirements: {unknown:?}"
            )));
        }
        let has_open = requirement_ids
            .iter()
            .any(|requirement| requirements[requirement] == "open");
        if disposition == "open-requirement" {
            if !has_open {
                return Err(fail(format!("{id} has no open owning requirement")));
            }
        } else if has_open {
            return Err(fail(format!(
                "{id} claims {disposition} with an unchecked requirement"
            )));
        }
        let selectors = group["selectors"]
            .as_array()
            .filter(|selectors| !selectors.is_empty())
            .ok_or_else(|| fail(format!("{id} has no inventory selectors")))?;
        let mut selected = BTreeSet::new();
        for selector in selectors {
            if !selector.is_object() {
                return Err(fail(format!("{id} has a malformed selector")));
            }
            let items = selector_items(selector, &inventory)?;
            let overlap: Vec<_> = selected.intersection(&items).cloned().collect();
            if !overlap.is_empty() {
                return Err(fail(format!(
                    "{id} selects inventory more than once: {overlap:?}"
                )));
            }
            selected.extend(items);
        }
        for item in &selected {
            if let Some(previous) = assigned.insert(item.clone(), id.to_owned()) {
                return Err(fail(format!(
                    "{}:{} assigned by both {previous} and {id}",
                    item.0, item.1
                )));
            }
        }
        *disposition_counts.get_mut(disposition).unwrap() += selected.len();
        normalized_groups.push(json!({
            "id": id,
            "disposition": disposition,
            "item_count": selected.len(),
            "requirements": requirement_ids,
            "evidence": evidence,
        }));
    }
    let expected_items: BTreeSet<_> = inventory
        .iter()
        .flat_map(|(category, values)| {
            values
                .iter()
                .map(move |value| (category.clone(), value.clone()))
        })
        .collect();
    let missing: Vec<_> = expected_items
        .difference(&assigned.keys().cloned().collect())
        .cloned()
        .collect();
    if !missing.is_empty() {
        return Err(fail(format!(
            "TRM inventory has no disposition: {}",
            missing
                .iter()
                .map(|(category, value)| format!("{category}:{value}"))
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    let inventory_counts: BTreeMap<_, _> = inventory
        .iter()
        .map(|(category, values)| (category.clone(), values.len()))
        .collect();
    Ok(json!({
        "inventory_counts": inventory_counts,
        "total_inventory_items": expected_items.len(),
        "coverage_group_count": normalized_groups.len(),
        "disposition_counts": disposition_counts,
        "groups": normalized_groups,
    }))
}

fn read_json(path: &Path) -> Result<Value, Failure> {
    serde_json::from_slice(&fs::read(path)?).map_err(|error| fail(error.to_string()))
}

fn input_entry(path: &Path, root: &Path) -> Result<Value, Failure> {
    let relative = path.strip_prefix(root).map_err(|_| {
        fail(format!(
            "TRM coverage input is outside repository: {}",
            path.display()
        ))
    })?;
    Ok(json!({
        "path": relative.to_string_lossy().replace('\\', "/"),
        "bytes": fs::metadata(path)?.len(),
        "sha256": sha256(path)?,
    }))
}

fn build_report(manifest_path: &Path, root: &Path) -> Result<Value, Failure> {
    let summary = validate_manifest(&read_json(manifest_path)?, root)?;
    let paths = [
        ("manifest", manifest_path.to_path_buf()),
        ("tasks", root.join("TASKS.md")),
        (
            "validator",
            root.join("verification/constrained_random_rs/src/trm_coverage.rs"),
        ),
        ("documentation", root.join("docs/TRM_COVERAGE.md")),
        ("manual", root.join(SOURCE_PATH)),
    ];
    let inputs: BTreeMap<_, _> = paths
        .into_iter()
        .map(|(name, path)| Ok((name, input_entry(&path, root)?)))
        .collect::<Result<_, Failure>>()?;
    let mut report = json!({
        "schema": REPORT_SCHEMA,
        "status": "passed",
        "created_utc": utc_now(),
        "git": {
            "commit": git_text(&["rev-parse", "HEAD"])?,
            "dirty": !git_text(&["status", "--porcelain=v1", "--untracked-files=all"])?.is_empty(),
        },
        "source": source_identity(),
        "inputs": inputs,
    });
    report
        .as_object_mut()
        .unwrap()
        .extend(summary.as_object().unwrap().clone());
    Ok(report)
}

pub(crate) fn validate_report(
    report: &Value,
    root: &Path,
    manifest_path: &Path,
    expected_commit: Option<&str>,
    require_clean: bool,
) -> Result<(), Failure> {
    if report["schema"] != REPORT_SCHEMA || report["status"] != "passed" {
        return Err(fail("TRM coverage report is not a passing known schema"));
    }
    if report["source"] != source_identity() {
        return Err(fail("TRM coverage report source identity changed"));
    }
    let expected = expected_inventory();
    let counts: BTreeMap<_, _> = expected
        .iter()
        .map(|(key, values)| (*key, values.len()))
        .collect();
    if report["inventory_counts"] != json!(counts) {
        return Err(fail("TRM coverage report inventory counts changed"));
    }
    if report["total_inventory_items"] != expected.values().map(BTreeSet::len).sum::<usize>() {
        return Err(fail("TRM coverage report total changed"));
    }
    if require_clean && report["git"]["dirty"] != false {
        return Err(fail("TRM coverage report describes a dirty source tree"));
    }
    if expected_commit.is_some_and(|commit| report["git"]["commit"] != commit) {
        return Err(fail("TRM coverage report commit does not match regression"));
    }
    let inputs = report["inputs"]
        .as_object()
        .ok_or_else(|| fail("TRM coverage report input manifest changed"))?;
    let expected_names: BTreeSet<_> = ["manifest", "tasks", "validator", "documentation", "manual"]
        .into_iter()
        .collect();
    if inputs.keys().map(String::as_str).collect::<BTreeSet<_>>() != expected_names {
        return Err(fail("TRM coverage report input manifest changed"));
    }
    let expected_paths = BTreeMap::from([
        ("manifest", manifest_path.to_path_buf()),
        ("tasks", root.join("TASKS.md")),
        (
            "validator",
            root.join("verification/constrained_random_rs/src/trm_coverage.rs"),
        ),
        ("documentation", root.join("docs/TRM_COVERAGE.md")),
        ("manual", root.join(SOURCE_PATH)),
    ]);
    for (name, entry) in inputs {
        let path = entry["path"]
            .as_str()
            .ok_or_else(|| fail("TRM coverage report input entry is malformed"))?;
        let full = root.join(path);
        if !full.is_file()
            || full.canonicalize()? != expected_paths[name.as_str()].canonicalize()?
        {
            return Err(fail(format!(
                "TRM coverage report input path changed: {name}: {}",
                full.display()
            )));
        }
        if entry["bytes"].as_u64() != Some(fs::metadata(&full)?.len())
            || entry["sha256"].as_str() != Some(sha256(&full)?.as_str())
        {
            return Err(fail(format!(
                "TRM coverage report input hash mismatch: {}",
                full.display()
            )));
        }
    }
    Ok(())
}

fn absolute(path: &Path) -> Result<PathBuf, Failure> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

pub fn command(arguments: &[String]) -> Result<(), String> {
    run(arguments).map_err(|error| error.to_string())
}

fn run(arguments: &[String]) -> Result<(), Failure> {
    let root = repo_root();
    let mut manifest = root.join("verification/trm_coverage.json");
    let mut output = root.join("reports/generated/trm-coverage-report.json");
    let mut report_to_validate = None;
    let mut expected_commit = None;
    let mut require_clean = false;
    let mut check = false;
    let mut position = 0;
    while position < arguments.len() {
        let name = &arguments[position];
        if name == "--check" || name == "--require-clean" {
            if name == "--check" {
                check = true;
            } else {
                require_clean = true;
            }
            position += 1;
            continue;
        }
        let value = arguments
            .get(position + 1)
            .ok_or_else(|| fail(format!("missing value for {name}")))?;
        match name.as_str() {
            "--manifest" => manifest = PathBuf::from(value),
            "--output" => output = PathBuf::from(value),
            "--validate-report" => report_to_validate = Some(PathBuf::from(value)),
            "--expected-commit" => expected_commit = Some(value.clone()),
            _ => return Err(fail(format!("unknown argument: {name}"))),
        }
        position += 2;
    }
    let manifest = absolute(&manifest)?;
    if let Some(path) = report_to_validate {
        let report = read_json(&absolute(&path)?)?;
        validate_report(
            &report,
            &root,
            &manifest,
            expected_commit.as_deref(),
            require_clean,
        )?;
        let summary = validate_manifest(&read_json(&manifest)?, &root)?;
        for field in [
            "inventory_counts",
            "total_inventory_items",
            "coverage_group_count",
            "disposition_counts",
            "groups",
        ] {
            if report[field] != summary[field] {
                return Err(fail(format!(
                    "TRM coverage report {field} does not match manifest"
                )));
            }
        }
        return Ok(());
    }
    if require_clean || expected_commit.is_some() {
        return Err(fail(
            "--require-clean and --expected-commit require --validate-report",
        ));
    }
    let report = build_report(&manifest, &root)?;
    if check {
        validate_report(&report, &root, &manifest, None, false)?;
    }
    write_json(&absolute(&output)?, &report)?;
    let counts = &report["inventory_counts"];
    println!(
        "[trm-coverage] PASS ({} sections/subsections, {} tables, {} figures, {} signals; {} disposition groups)",
        counts["sections"], counts["tables"], counts["figures"], counts["signals"],
        report["coverage_group_count"]
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checked_manifest() -> Value {
        read_json(&repo_root().join("verification/trm_coverage.json")).unwrap()
    }

    #[test]
    fn checked_inventory_and_dispositions_cover_every_manual_item() {
        let summary = validate_manifest(&checked_manifest(), &repo_root()).unwrap();
        assert_eq!(
            summary["inventory_counts"],
            json!({
                "sections": 207, "tables": 65, "figures": 44, "signals": 45
            })
        );
        assert_eq!(summary["total_inventory_items"], 361);
        assert_eq!(
            summary["disposition_counts"],
            json!({
                "implemented-and-tested": 343,
                "external-out-of-scope": 8,
                "integration-obligation": 10,
                "erratum-corrected": 0,
                "open-requirement": 0,
            })
        );
    }

    #[test]
    fn rejects_missing_duplicate_unknown_unchecked_and_missing_evidence() {
        let root = repo_root();
        let mut manifest = checked_manifest();
        manifest["inventory"]["sections"]
            .as_array_mut()
            .unwrap()
            .retain(|value| value != "5.18.5");
        assert!(validate_manifest(&manifest, &root)
            .unwrap_err()
            .to_string()
            .contains("sections inventory mismatch"));

        let mut manifest = checked_manifest();
        manifest["coverage_groups"][0]["selectors"]
            .as_array_mut()
            .unwrap()
            .push(json!({"category":"sections", "items":["1.1"]}));
        assert!(validate_manifest(&manifest, &root)
            .unwrap_err()
            .to_string()
            .contains("selects inventory more than once"));

        let mut manifest = checked_manifest();
        manifest["coverage_groups"][0]["requirements"] = json!(["NOPE-999"]);
        assert!(validate_manifest(&manifest, &root)
            .unwrap_err()
            .to_string()
            .contains("unknown requirements"));

        let mut manifest = checked_manifest();
        manifest["coverage_groups"][0]["requirements"] = json!(["MIST-008"]);
        assert!(validate_manifest(&manifest, &root)
            .unwrap_err()
            .to_string()
            .contains("unchecked requirement"));

        let mut manifest = checked_manifest();
        manifest["source"]["sha256"] = json!("0".repeat(64));
        assert!(validate_manifest(&manifest, &root)
            .unwrap_err()
            .to_string()
            .contains("source identity changed"));

        let mut manifest = checked_manifest();
        manifest["coverage_groups"][0]["evidence"] = json!(["docs/THIS_FILE_DOES_NOT_EXIST.md"]);
        assert!(validate_manifest(&manifest, &root)
            .unwrap_err()
            .to_string()
            .contains("evidence path is missing"));
    }

    #[test]
    fn report_inputs_are_hashed_and_release_identity_is_fail_closed() {
        let root = repo_root();
        let manifest = root.join("verification/trm_coverage.json");
        let report = build_report(&manifest, &root).unwrap();
        validate_report(
            &report,
            &root,
            &manifest,
            Some(report["git"]["commit"].as_str().unwrap()),
            false,
        )
        .unwrap();
        let mut changed = report.clone();
        changed["inputs"]["validator"]["sha256"] = json!("0".repeat(64));
        assert!(validate_report(&changed, &root, &manifest, None, false)
            .unwrap_err()
            .to_string()
            .contains("input hash mismatch"));
        let mut changed = report.clone();
        changed["inputs"]["validator"]["path"] = json!("TASKS.md");
        changed["inputs"]["validator"]["bytes"] =
            json!(fs::metadata(root.join("TASKS.md")).unwrap().len());
        changed["inputs"]["validator"]["sha256"] = json!(sha256(&root.join("TASKS.md")).unwrap());
        assert!(validate_report(&changed, &root, &manifest, None, false)
            .unwrap_err()
            .to_string()
            .contains("input path changed"));
        let mut changed = report.clone();
        changed["git"]["dirty"] = Value::Null;
        assert!(validate_report(&changed, &root, &manifest, None, true)
            .unwrap_err()
            .to_string()
            .contains("dirty source tree"));
    }
}
