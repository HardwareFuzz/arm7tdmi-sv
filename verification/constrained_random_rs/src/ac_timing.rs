//! Revision-locked Chapter 8/Table 8-1 timing disposition and active SDC audit.
//! The checked-in reference is independent of the mutable disposition manifest.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use chrono::{SecondsFormat, Utc};
use regex::Regex;
use serde_json::{json, Value};

use crate::campaign::{git_text, repo_root, sha256, write_json, Failure};

const MAP_SCHEMA: &str = "arm7tdmis-ac-timing-map-v1";
const REPORT_SCHEMA: &str = "arm7tdmis-ac-timing-v1";
const SOURCE_PATH: &str = "ARM_DDI_0234B_ARM7TDMI-S_r4p3_TRM.pdf";
const SOURCE_BYTES: u64 = 1_477_711;
const SOURCE_SHA256: &str = "bb9cd0e3f2b7e2fdca4ff961cdfc5c9c85c063842e491f8997a504d3241baa14";
const SDC_PATH: &str = "fpga/arm7tdmis_conformance.sdc";

static REFERENCE: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("ac_timing_reference.json"))
        .expect("checked-in Table 8 reference must be valid JSON")
});
static DELAY_COMMAND: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^set_(input|output)_delay -clock CLK -(min|max) (-?[0-9]+(?:\.[0-9]+)?) \[get_ports \{([^}]*)\}\]$",
    )
    .unwrap()
});

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
        "section": "8.2",
        "table": "8-1",
        "printed_pages": ["8-8", "8-9"],
        "title": "Provisional AC parameters",
    })
}

fn target_identity() -> Value {
    json!({
        "profile": "raw-feature-complete",
        "top": "arm7tdmis_top",
        "device": "5CSEBA6U23I7",
        "tool": "Quartus Lite 17.0.2",
        "sdc": SDC_PATH,
        "clock_port": "CLK",
        "clock_period_ns": 62.5,
        "clock_mhz": 16.0,
        "input_translation": "max input delay = negative source setup fraction relative to the rising capture edge; min input delay = 0.25 ns target clock-skew margin for a source 0% hold",
        "output_translation": "max output delay = period - source clock-to-valid fraction; min output delay = 0 ns and routed hold slack must remain positive for source >0% holds",
    })
}

fn strings(value: &Value, label: &str) -> Result<Vec<String>, Failure> {
    value
        .as_array()
        .ok_or_else(|| fail(format!("{label} is malformed")))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| fail(format!("{label} is malformed")))
        })
        .collect()
}

fn keys(value: &Value) -> Option<BTreeSet<&str>> {
    Some(value.as_object()?.keys().map(String::as_str).collect())
}

fn commands(text: &str) -> Result<Vec<String>, Failure> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut braces = 0i32;
    let mut brackets = 0i32;
    for raw_line in text.lines() {
        let line = raw_line.split('#').next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(line);
        braces += line.matches('{').count() as i32 - line.matches('}').count() as i32;
        brackets += line.matches('[').count() as i32 - line.matches(']').count() as i32;
        if braces < 0 || brackets < 0 {
            return Err(fail("SDC has an unmatched Tcl delimiter"));
        }
        if braces == 0 && brackets == 0 {
            result.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        return Err(fail("SDC ends with an incomplete Tcl command"));
    }
    Ok(result)
}

type DelayKey = (String, String, Vec<String>);

fn delay_constraints(text: &str) -> Result<BTreeMap<DelayKey, f64>, Failure> {
    let mut result = BTreeMap::new();
    for command in commands(text)? {
        let Some(captures) = DELAY_COMMAND.captures(&command) else {
            continue;
        };
        let key = (
            captures[1].to_owned(),
            captures[2].to_owned(),
            captures[4].split_whitespace().map(str::to_owned).collect(),
        );
        if result.contains_key(&key) {
            return Err(fail(format!("duplicate SDC delay constraint: {key:?}")));
        }
        let delay = captures[3]
            .parse::<f64>()
            .map_err(|error| fail(error.to_string()))?;
        result.insert(key, delay);
    }
    Ok(result)
}

fn validate_scope(contract: &Value) -> Result<(), Failure> {
    let scope = &contract["scope"];
    let expected: BTreeSet<_> = [
        "source_units",
        "supplier_dependency",
        "implementation_scope",
        "fpga_claim",
    ]
    .into_iter()
    .collect();
    if keys(scope) != Some(expected) {
        return Err(fail("AC timing physical scope is incomplete"));
    }
    let joined = scope
        .as_object()
        .unwrap()
        .values()
        .map(|value| {
            value
                .as_str()
                .map_or_else(|| value.to_string(), str::to_owned)
        })
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    for term in [
        "percent",
        "supplier",
        "provisional",
        "synthesized",
        "not a portable",
    ] {
        if !joined.contains(term) {
            return Err(fail(format!(
                "AC timing physical scope lost required caveat: {term}"
            )));
        }
    }
    Ok(())
}

fn validate_figures(contract: &Value) -> Result<(), Failure> {
    let figures = contract["figure_groups"]
        .as_array()
        .filter(|figures| figures.len() == 5)
        .ok_or_else(|| fail("Chapter 8 figure disposition is incomplete"))?;
    let mut actual = BTreeMap::<String, (String, Vec<String>)>::new();
    for figure in figures {
        let id = figure["figure"]
            .as_str()
            .ok_or_else(|| fail("Chapter 8 figure disposition is malformed"))?;
        let title = figure["title"]
            .as_str()
            .ok_or_else(|| fail("Chapter 8 figure disposition is malformed"))?;
        let rows = strings(&figure["rows"], "Chapter 8 figure rows")?;
        if actual
            .insert(id.to_owned(), (title.to_owned(), rows))
            .is_some()
        {
            return Err(fail(format!(
                "duplicate Chapter 8 figure disposition: {id}"
            )));
        }
    }
    let expected: BTreeMap<String, (String, Vec<String>)> = REFERENCE["figure_groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|figure| {
            (
                figure["figure"].as_str().unwrap().to_owned(),
                (
                    figure["title"].as_str().unwrap().to_owned(),
                    strings(&figure["rows"], "reference figure rows").unwrap(),
                ),
            )
        })
        .collect();
    if actual != expected {
        return Err(fail("Chapter 8 figure-to-row mapping changed"));
    }
    Ok(())
}

fn validate_ownership(contract: &Value) -> Result<(), Failure> {
    let entries = contract["integration_owned"]
        .as_array()
        .ok_or_else(|| fail("integration-owned timing categories are missing"))?;
    let mut actual = BTreeMap::<String, Vec<String>>::new();
    for entry in entries {
        let id = entry["id"]
            .as_str()
            .ok_or_else(|| fail("integration-owned timing category is malformed"))?;
        if entry["disposition"]
            .as_str()
            .is_none_or(|value| value.trim().is_empty())
        {
            return Err(fail("integration-owned timing category is malformed"));
        }
        let signals = strings(&entry["signals"], "integration-owned signals")?;
        if actual.insert(id.to_owned(), signals).is_some() {
            return Err(fail(format!("duplicate integration-owned category: {id}")));
        }
    }
    let expected: BTreeMap<String, Vec<String>> = REFERENCE["integration_owned"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, value)| {
            (
                key.clone(),
                strings(value, "reference owner signals").unwrap(),
            )
        })
        .collect();
    if actual != expected {
        return Err(fail("clock/reset/debug/test/power ownership changed"));
    }
    let non_table = &contract["non_table_raw_ports"];
    let expected_non_table = REFERENCE["non_table_raw_ports"].as_object().unwrap();
    if keys(non_table) != Some(expected_non_table.keys().map(String::as_str).collect()) {
        return Err(fail("non-Table-8 raw-port disposition is incomplete"));
    }
    for (category, ports) in expected_non_table {
        let entry = &non_table[category];
        if entry["disposition"]
            .as_str()
            .is_none_or(|value| value.trim().is_empty())
            || entry["ports"] != *ports
        {
            return Err(fail(format!("non-Table-8 {category} disposition changed")));
        }
    }
    Ok(())
}

fn validate_contract(
    contract: &Value,
    root: &Path,
    sdc_text: Option<&str>,
    verify_source: bool,
) -> Result<Value, Failure> {
    if contract["schema"] != MAP_SCHEMA {
        return Err(fail("AC timing manifest has wrong schema"));
    }
    if contract["source"] != source_identity() {
        return Err(fail("AC timing source identity changed"));
    }
    if contract["target"] != target_identity() {
        return Err(fail("AC timing FPGA target/translation changed"));
    }
    validate_scope(contract)?;
    validate_figures(contract)?;
    validate_ownership(contract)?;

    if verify_source {
        let manual = root.join(SOURCE_PATH);
        if !manual.is_file()
            || fs::metadata(&manual)?.len() != SOURCE_BYTES
            || sha256(&manual)? != SOURCE_SHA256
        {
            return Err(fail("TRM PDF identity does not match the audited revision"));
        }
    }
    let expected_rows = REFERENCE["rows"].as_array().unwrap();
    let rows = contract["rows"]
        .as_array()
        .filter(|rows| rows.len() == expected_rows.len())
        .ok_or_else(|| {
            fail(format!(
                "Table 8-1 must contain exactly {} rows",
                expected_rows.len()
            ))
        })?;
    let actual_symbols = rows
        .iter()
        .map(|row| row["symbol"].as_str().unwrap_or_default().to_owned())
        .collect::<Vec<_>>();
    let expected_symbols = expected_rows
        .iter()
        .map(|row| row["symbol"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    if actual_symbols != expected_symbols
        || actual_symbols.iter().collect::<BTreeSet<_>>().len() != actual_symbols.len()
    {
        return Err(fail("Table 8-1 row order/set changed"));
    }
    let mut positive_hold_rows = 0;
    let mut replacement_count = 0;
    for (actual, expected) in rows.iter().zip(expected_rows) {
        let symbol = expected["symbol"].as_str().unwrap();
        for field in ["symbol", "parameter", "source_min", "source_max", "figure"] {
            if actual[field] != expected[field] {
                return Err(fail(format!("Table 8-1 {symbol} {field} changed")));
            }
        }
        if actual["signals"] != expected["signals"] {
            return Err(fail(format!("Table 8-1 {symbol} signal mapping changed")));
        }
        if !actual["target_sdc"].is_object() {
            return Err(fail(format!(
                "Table 8-1 {symbol} has no target SDC mapping"
            )));
        }
        if actual["target_sdc"] != expected["target_sdc"] {
            return Err(fail(format!(
                "Table 8-1 {symbol} target SDC mapping changed"
            )));
        }
        if expected["target_sdc"]["kind"] == "output-min" {
            positive_hold_rows += 1;
        }
        if actual["figure_symbol"] != expected["figure_symbol"] {
            return Err(fail(format!(
                "Figure 8-4 symbol conflict mapping changed: {symbol}"
            )));
        }
        if actual["replaced_signals"] != expected["replaced_signals"] {
            // The old audit treats a missing and an empty list alike.
            if !(expected.get("replaced_signals").is_none()
                && actual["replaced_signals"] == json!([]))
            {
                return Err(fail(format!(
                    "Table 8-1 {symbol} reset replacement changed"
                )));
            }
        }
        if let Some(replacements) = expected["replaced_signals"].as_array() {
            replacement_count += replacements.len();
        }
    }
    let notes = &contract["source_notes"];
    let expected_note_keys: BTreeSet<_> = ["zero_hold", "control_group", "debug_label_conflict"]
        .into_iter()
        .collect();
    if keys(notes) != Some(expected_note_keys) {
        return Err(fail("Table 8-1 source notes are incomplete"));
    }
    if !notes["debug_label_conflict"]
        .as_str()
        .is_some_and(|value| value.contains("tohdbctl"))
    {
        return Err(fail("Table/Figure 8-4 symbol conflict caveat disappeared"));
    }

    let owned_sdc;
    let sdc = if let Some(sdc) = sdc_text {
        sdc
    } else {
        owned_sdc = fs::read_to_string(root.join(SDC_PATH))?;
        &owned_sdc
    };
    let constraints = delay_constraints(sdc)?;
    for expected in expected_rows {
        let symbol = expected["symbol"].as_str().unwrap();
        let marker = format!("AC-T8:{symbol}");
        if sdc.matches(&marker).count() != 1 {
            return Err(fail(format!(
                "SDC must contain exactly one {marker} marker"
            )));
        }
        let target = &expected["target_sdc"];
        let kind = target["kind"].as_str().unwrap();
        if kind == "clock" {
            if !sdc.contains("create_clock -name CLK -period 62.500 [get_ports {CLK}]") {
                return Err(fail("tcyc target clock constraint changed"));
            }
            continue;
        }
        let (direction, bound) = kind
            .split_once('-')
            .ok_or_else(|| fail(format!("invalid reference SDC kind: {kind}")))?;
        let ports = strings(&target["ports"], "reference SDC ports")?;
        let key = (direction.to_owned(), bound.to_owned(), ports);
        let expected_delay = target["delay_ns"].as_f64().unwrap();
        if constraints.get(&key) != Some(&expected_delay) {
            return Err(fail(format!(
                "SDC constraint for {symbol} changed: expected {key:?}={expected_delay}, got {:?}",
                constraints.get(&key)
            )));
        }
    }
    if !sdc.contains("AC-OWN:reset") {
        return Err(fail("reset replacement SDC marker is missing"));
    }
    let false_paths = commands(sdc)?
        .into_iter()
        .filter(|command| command.starts_with("set_false_path"))
        .collect::<Vec<_>>()
        .join(" ");
    for reset in ["nRESET", "DBGnTRST"] {
        if !false_paths.contains(reset) {
            return Err(fail(format!(
                "asynchronous reset is not false-pathed: {reset}"
            )));
        }
    }
    if !false_paths.contains("DBGEN") {
        return Err(fail("DBGEN static integration strap is not false-pathed"));
    }
    for marker in ["AC-NONTABLE:static", "AC-NONTABLE:output"] {
        if sdc.matches(marker).count() != 1 {
            return Err(fail(format!("non-Table-8 target marker changed: {marker}")));
        }
    }
    Ok(json!({
        "table_row_count": rows.len(),
        "figure_count": REFERENCE["figure_groups"].as_array().unwrap().len(),
        "sdc_mapped_row_count": rows.len(),
        "positive_hold_row_count": positive_hold_rows,
        "replacement_signal_count": replacement_count,
        "integration_owner_count": REFERENCE["integration_owned"].as_object().unwrap().len(),
        "symbols": expected_symbols,
    }))
}

fn read_json(path: &Path) -> Result<Value, Failure> {
    serde_json::from_slice(&fs::read(path)?).map_err(|error| fail(error.to_string()))
}

fn build_report(manifest_path: &Path, root: &Path) -> Result<Value, Failure> {
    let contract = read_json(manifest_path)?;
    let summary = validate_contract(&contract, root, None, true)?;
    let manifest_path = manifest_path.canonicalize()?;
    let root = root.canonicalize()?;
    let relative = manifest_path
        .strip_prefix(&root)
        .map_err(|_| fail("AC timing manifest is outside repository"))?;
    let sdc = root.join(SDC_PATH);
    let mut report = json!({
        "schema": REPORT_SCHEMA,
        "generated_utc": Utc::now().to_rfc3339_opts(SecondsFormat::Micros, false),
        "git": {
            "commit": git_text(&["rev-parse", "HEAD"])?,
            "dirty": !git_text(&["status", "--porcelain", "--untracked-files=all"])?.is_empty(),
        },
        "source": source_identity(),
        "target": target_identity(),
        "manifest": {
            "path": relative.to_string_lossy().replace('\\', "/"),
            "sha256": sha256(&manifest_path)?,
        },
        "sdc": {"path": SDC_PATH, "sha256": sha256(&sdc)?},
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
    if report["schema"] != REPORT_SCHEMA {
        return Err(fail("AC timing report has wrong schema"));
    }
    if report["source"] != source_identity() {
        return Err(fail("AC timing report source identity changed"));
    }
    if report["target"] != target_identity() {
        return Err(fail("AC timing report target identity changed"));
    }
    if report["table_row_count"] != REFERENCE["rows"].as_array().unwrap().len() {
        return Err(fail("AC timing report row count changed"));
    }
    if expected_commit.is_some_and(|commit| report["git"]["commit"] != commit) {
        return Err(fail("AC timing report commit does not match regression"));
    }
    if require_clean && report["git"]["dirty"] != false {
        return Err(fail("AC timing report describes a dirty source tree"));
    }
    let canonical_root = root.canonicalize()?;
    let canonical_manifest = manifest_path.canonicalize()?;
    let manifest_relative = canonical_manifest
        .strip_prefix(&canonical_root)
        .map_err(|_| fail("AC timing manifest is outside repository"))?
        .to_string_lossy()
        .replace('\\', "/");
    if report["manifest"]["path"] != manifest_relative
        || report["manifest"]["sha256"] != sha256(manifest_path)?
        || report["sdc"]["path"] != SDC_PATH
        || report["sdc"]["sha256"] != sha256(&root.join(SDC_PATH))?
    {
        return Err(fail("AC timing report manifest/SDC identity changed"));
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
    let mut manifest = root.join("verification/ac_timing.json");
    let mut output = root.join("reports/generated/ac-timing-report.json");
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
        let summary = validate_contract(&read_json(&manifest)?, &root, None, true)?;
        for field in [
            "table_row_count",
            "figure_count",
            "sdc_mapped_row_count",
            "positive_hold_row_count",
            "replacement_signal_count",
            "integration_owner_count",
            "symbols",
        ] {
            if report[field] != summary[field] {
                return Err(fail(format!(
                    "AC timing disposition report {field} does not match manifest"
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
    println!(
        "[ac-timing] PASS ({} Table 8-1 rows, {} figures, {} integration-owner categories)",
        report["table_row_count"], report["figure_count"], report["integration_owner_count"]
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checked_contract() -> Value {
        read_json(&repo_root().join("verification/ac_timing.json")).unwrap()
    }

    fn checked_sdc() -> String {
        fs::read_to_string(repo_root().join(SDC_PATH)).unwrap()
    }

    fn rejects(contract: &Value, sdc: &str, expected: &str) {
        let error = validate_contract(contract, &repo_root(), Some(sdc), false)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(expected),
            "{error:?} did not contain {expected:?}"
        );
    }

    #[test]
    fn checked_table_and_sdc_cover_all_37_rows() {
        let summary = validate_contract(&checked_contract(), &repo_root(), None, true).unwrap();
        assert_eq!(summary["table_row_count"], 37);
        assert_eq!(summary["figure_count"], 5);
        assert_eq!(summary["sdc_mapped_row_count"], 37);
        assert_eq!(summary["positive_hold_row_count"], 9);
        assert_eq!(summary["replacement_signal_count"], 2);
        assert_eq!(summary["integration_owner_count"], 5);
    }

    #[test]
    fn mutated_rows_scope_figures_and_ownership_fail() {
        let sdc = checked_sdc();
        let mut contract = checked_contract();
        contract["rows"].as_array_mut().unwrap().pop();
        rejects(&contract, &sdc, "exactly 37 rows");

        let mut contract = checked_contract();
        contract["rows"].as_array_mut().unwrap().swap(0, 1);
        rejects(&contract, &sdc, "row order/set changed");

        let mut contract = checked_contract();
        contract["rows"][1]["source_min"] = json!("39%");
        rejects(&contract, &sdc, "tisclken source_min changed");

        let mut contract = checked_contract();
        contract["scope"]["fpga_claim"] = json!("portable guarantee");
        rejects(&contract, &sdc, "physical scope lost");

        let mut contract = checked_contract();
        contract["figure_groups"][3]["rows"]
            .as_array_mut()
            .unwrap()
            .retain(|row| row != "tohdbgstat");
        rejects(&contract, &sdc, "figure-to-row mapping changed");

        let mut contract = checked_contract();
        contract["integration_owned"][3]["signals"]
            .as_array_mut()
            .unwrap()
            .retain(|signal| signal != "SCANOUT");
        rejects(&contract, &sdc, "ownership changed");
    }

    #[test]
    fn weakened_active_sdc_constraints_fail() {
        let contract = checked_contract();
        let sdc = checked_sdc();
        rejects(
            &contract,
            &sdc.replacen("AC-T8:tovtrans", "AC-T8:deleted", 1),
            "AC-T8:tovtrans marker",
        );
        rejects(
            &contract,
            &sdc.replacen(
                "set_output_delay -clock CLK -max 31.250 [get_ports {TRANS[*]}]",
                "set_output_delay -clock CLK -max 32.250 [get_ports {TRANS[*]}]",
                1,
            ),
            "constraint for tovtrans changed",
        );
        rejects(
            &contract,
            &sdc.replacen("nRESET DBGnTRST", "nRESET", 1),
            "DBGnTRST",
        );
        rejects(
            &contract,
            &sdc.replacen("set_false_path -from [get_ports {DBGEN}]", "", 1),
            "DBGEN static",
        );
    }

    #[test]
    fn report_hashes_commit_and_cleanliness_are_checked() {
        let root = repo_root();
        let manifest = root.join("verification/ac_timing.json");
        let report = build_report(&manifest, &root).unwrap();
        validate_report(
            &report,
            &root,
            &manifest,
            report["git"]["commit"].as_str(),
            false,
        )
        .unwrap();
        let mut changed = report.clone();
        changed["sdc"]["sha256"] = json!("0".repeat(64));
        assert!(validate_report(&changed, &root, &manifest, None, false).is_err());
        let mut changed = report.clone();
        changed["git"]["dirty"] = Value::Null;
        assert!(validate_report(&changed, &root, &manifest, None, true).is_err());
    }
}
