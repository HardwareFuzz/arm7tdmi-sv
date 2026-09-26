//! Fail-closed parsing of the pinned MiSTer framework's Quartus reports.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::{json, Value};

use crate::campaign::{sha256, Failure};

pub(super) const SCHEMA: &str = "arm7tdmis-mister-framework-v1";
pub(super) const URL: &str = "https://github.com/MiSTer-devel/Template_MiSTer.git";
pub(super) const COMMIT: &str = "69b8a2acc6d84dd313b5abcba6a17155287ed3d8";
pub(super) const TREE: &str = "b8b642eae80716da93754a3e778a5daa3313350b";
pub(super) const PROJECT: &str = "Template";
pub(super) const TOP: &str = "sys_top";
pub(super) const DEVICE: &str = "5CSEBA6U23I7";
pub(super) const CLOCK_MHZ: f64 = 12.5;
pub(super) const CLOCK_PERIOD_NS: f64 = 80.0;
pub(super) const FITTER_SEED: u64 = 4;
pub(super) const CLOCK_TARGET: &str =
    "emu|pll|pll_inst|altera_pll_i|general[0].gpll~PLL_OUTPUT_COUNTER|divclk";
pub(super) const UNCONSTRAINED_LABELS: [&str; 5] = [
    "Unconstrained Clocks",
    "Unconstrained Input Ports",
    "Unconstrained Input Port Paths",
    "Unconstrained Output Ports",
    "Unconstrained Output Port Paths",
];
pub(super) const REPORT_SUFFIXES: [&str; 9] = [
    ".flow.rpt",
    ".map.summary",
    ".map.rpt",
    ".fit.summary",
    ".fit.rpt",
    ".asm.rpt",
    ".sta.summary",
    ".sta.rpt",
    ".rbf",
];
const REVIEWED_WAIVERS: [&str; 4] = [
    "sys_top.sdc:60:332174:arc*",
    "sys_top.sdc:61:332174:arx*",
    "sys_top.sdc:61:332174:ary*",
    "sys_top.sdc:61:332049:set_false_path",
];

fn fail(message: impl Into<String>) -> Failure {
    Failure::other(message)
}

fn capture(pattern: &str, text: &str, group: usize) -> Option<String> {
    Regex::new(pattern)
        .expect("static Quartus report regex")
        .captures(text)
        .and_then(|match_| match_.get(group).map(|value| value.as_str().to_owned()))
}

fn integer_field(text: &str, label: &str) -> Option<u64> {
    let pattern = format!(r"(?m)^{}\s*:\s*([\d,]+)", regex::escape(label));
    capture(&pattern, text, 1)?.replace(',', "").parse().ok()
}

fn read_report(path: &Path, errors: &mut Vec<String>) -> String {
    match fs::read(path) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            errors.push(format!(
                "missing report: {}",
                path.file_name().unwrap_or_default().to_string_lossy()
            ));
            String::new()
        }
        Err(error) => {
            errors.push(format!("cannot read {}: {error}", path.display()));
            String::new()
        }
    }
}

/// Return the same result fields as the historical report, plus explicit errors.
pub(super) fn parse_reports(output_dir: &Path) -> (Value, Vec<String>) {
    let mut errors = Vec::new();
    let report = |suffix: &str, errors: &mut Vec<String>| {
        read_report(&output_dir.join(format!("{PROJECT}{suffix}")), errors)
    };
    let flow = report(".flow.rpt", &mut errors);
    let map_summary = report(".map.summary", &mut errors);
    let map_report = report(".map.rpt", &mut errors);
    let fit_summary = report(".fit.summary", &mut errors);
    let fit_report = report(".fit.rpt", &mut errors);
    let asm_report = report(".asm.rpt", &mut errors);
    let sta_summary = report(".sta.summary", &mut errors);
    let sta_report = report(".sta.rpt", &mut errors);

    for (contents, marker, success, label) in [
        (&flow, "Flow Status", "Successful", "full flow"),
        (
            &map_summary,
            "Analysis & Synthesis Status",
            "Successful",
            "synthesis",
        ),
        (&fit_summary, "Fitter Status", "Successful", "fit"),
        (&asm_report, "Assembler was successful", "", "assembly"),
        (
            &sta_report,
            "TimeQuest Timing Analyzer was successful",
            "",
            "TimeQuest",
        ),
    ] {
        if !contents.contains(marker) || (!success.is_empty() && !contents.contains(success)) {
            errors.push(format!("{label} success marker missing"));
        }
    }

    let top = capture(r"(?m)^Top-level Entity Name\s*:\s*(\S+)", &fit_summary, 1);
    let device = capture(r"(?m)^Device\s*:\s*(\S+)", &fit_summary, 1);
    if top.as_deref() != Some(TOP) {
        errors.push(format!(
            "unexpected top: {}",
            top.as_ref()
                .map_or("None".to_owned(), |value| format!("'{value}'"))
        ));
    }
    if device.as_deref() != Some(DEVICE) {
        errors.push(format!(
            "unexpected device: {}",
            device
                .as_ref()
                .map_or("None".to_owned(), |value| format!("'{value}'"))
        ));
    }
    let fitter_seed = capture(
        r"(?m)^\s*;\s*Fitter Initial Placement Seed\s*;\s*(\d+)\s*;",
        &fit_report,
        1,
    )
    .and_then(|value| value.parse::<u64>().ok());
    if fitter_seed != Some(FITTER_SEED) {
        errors.push(format!(
            "unexpected fitter seed: {}",
            fitter_seed.map_or("None".to_owned(), |value| value.to_string())
        ));
    }
    for entity in ["arm7tdmi_generic_soc", "arm7tdmi_mister"] {
        if !map_report.contains(entity) {
            errors.push(format!("mapped hierarchy lacks {entity}"));
        }
    }

    let critical_warning = Regex::new(r"^\s*Critical Warning \(\d+\):").unwrap();
    let ignored_warning = Regex::new(r"^Warning \(\d+\): Ignored ").unwrap();
    let filter = Regex::new(
        r"^Warning \(332174\): Ignored filter at (?P<file>[^\s(]+)\((?P<line>\d+)\): (?P<filter>.+?) could not be matched",
    ).unwrap();
    let command = Regex::new(
        r"^Warning \(332049\): Ignored (?P<command>\S+) at (?P<file>[^\s(]+)\((?P<line>\d+)\): Argument <from> is not an object ID",
    ).unwrap();
    let mut reviewed = BTreeSet::new();
    for (name, contents) in [
        ("flow", &flow),
        ("map", &map_report),
        ("fit", &fit_report),
        ("assembly", &asm_report),
        ("sta", &sta_report),
    ] {
        for line in contents.lines() {
            if critical_warning.is_match(line) {
                errors.push(format!("{name}: {}", line.trim()));
            }
            if !ignored_warning.is_match(line) {
                continue;
            }
            let waiver = if let Some(fields) = filter.captures(line) {
                format!(
                    "{}:{}:332174:{}",
                    Path::new(&fields["file"])
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy(),
                    &fields["line"],
                    &fields["filter"]
                )
            } else if let Some(fields) = command.captures(line) {
                format!(
                    "{}:{}:332049:{}",
                    Path::new(&fields["file"])
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy(),
                    &fields["line"],
                    &fields["command"]
                )
            } else {
                String::new()
            };
            if REVIEWED_WAIVERS.contains(&waiver.as_str()) {
                reviewed.insert(waiver);
            } else {
                errors.push(format!("{name}: {}", line.trim()));
            }
        }
    }

    let timing_re = Regex::new(r"Type\s*:\s*(.+?)\nSlack\s*:\s*(-?\d+(?:\.\d+)?)").unwrap();
    let mut timing = Vec::new();
    let mut setup_slacks = Vec::new();
    let mut hold_slacks = Vec::new();
    for fields in timing_re.captures_iter(&sta_summary) {
        let name = fields[1].trim();
        let slack: f64 = fields[2].parse().expect("regex matched numeric slack");
        timing.push(json!({"type": name, "slack_ns": slack}));
        if slack < 0.0 {
            errors.push(format!("negative slack {slack:.3} ns: {name}"));
        }
        if name.contains("Setup") {
            setup_slacks.push(slack);
        }
        if name.contains("Hold") {
            hold_slacks.push(slack);
        }
    }
    if timing.is_empty() {
        errors.push("no timing slack entries found".to_owned());
    }
    let mut unconstrained: BTreeMap<&str, Option<Value>> = BTreeMap::new();
    for label in UNCONSTRAINED_LABELS {
        let pattern = format!(
            r"(?m)^\s*;\s*{}\s*;\s*(\d+)\s*;\s*(\d+)\s*;",
            regex::escape(label)
        );
        let row = Regex::new(&pattern)
            .unwrap()
            .captures(&sta_report)
            .and_then(|fields| {
                Some(json!({
                    "setup": fields.get(1)?.as_str().parse::<u64>().ok()?,
                    "hold": fields.get(2)?.as_str().parse::<u64>().ok()?,
                }))
            });
        match &row {
            None => errors.push(format!("missing unconstrained-path row: {label}")),
            Some(counts) if counts != &json!({"setup": 0, "hold": 0}) => {
                errors.push(format!(
                    "{label} is not zero: {{'setup': {}, 'hold': {}}}",
                    counts["setup"], counts["hold"]
                ));
            }
            _ => {}
        }
        unconstrained.insert(label, row);
    }
    let clock_pattern = format!(
        r"(?m)^\s*;\s*{}\s*;\s*Generated\s*;\s*(\d+(?:\.\d+)?)\s*;\s*(\d+(?:\.\d+)?)\s+MHz\s*;",
        regex::escape(CLOCK_TARGET)
    );
    let core_clock = Regex::new(&clock_pattern).unwrap().captures(&sta_report).map(|fields| {
        let period_ns: f64 = fields[1].parse().expect("regex matched numeric period");
        let frequency_mhz: f64 = fields[2].parse().expect("regex matched numeric frequency");
        if (period_ns - CLOCK_PERIOD_NS).abs() > 0.001 ||
            (frequency_mhz - CLOCK_MHZ).abs() > 0.001 {
            errors.push(format!(
                "generated core clock has wrong period/frequency: {period_ns:.3} ns/{frequency_mhz:.3} MHz"
            ));
        }
        json!({"target": CLOCK_TARGET, "period_ns": period_ns, "frequency_mhz": frequency_mhz})
    });
    if core_clock.is_none() {
        errors.push(format!("generated {CLOCK_MHZ:.3} MHz core clock is absent"));
    }
    let resources = json!({
        "alm": integer_field(&fit_summary, "Logic utilization (in ALMs)"),
        "register": integer_field(&fit_summary, "Total registers"),
        "memory_bit": integer_field(&fit_summary, "Total block memory bits"),
        "dsp": integer_field(&fit_summary, "Total DSP Blocks"),
        "pll": integer_field(&fit_summary, "Total PLLs"),
    });
    for (name, value) in resources.as_object().unwrap() {
        if value.is_null() {
            errors.push(format!("missing resource field: {name}"));
        }
    }
    let bitstream = output_dir.join(format!("{PROJECT}.rbf"));
    if fs::metadata(&bitstream).map_or(true, |metadata| metadata.len() == 0) {
        errors.push(format!("missing or empty bitstream: {PROJECT}.rbf"));
    }
    let minimum_setup = setup_slacks.into_iter().reduce(f64::min);
    let minimum_hold = hold_slacks.into_iter().reduce(f64::min);
    let result = json!({
        "project": PROJECT,
        "top": top,
        "device": device,
        "fitter_seed": fitter_seed,
        "core_clock_mhz": core_clock.as_ref().map(|clock| clock["frequency_mhz"].clone()),
        "core_clock": core_clock,
        "reviewed_upstream_waivers": reviewed,
        "resources": resources,
        "timing": {
            "entries": timing,
            "minimum_setup_slack_ns": minimum_setup,
            "minimum_hold_slack_ns": minimum_hold,
        },
        "unconstrained": unconstrained,
        "status": if errors.is_empty() {"passed"} else {"failed"},
    });
    (result, errors)
}

fn verified_entry(root: &Path, path_text: &str, entry: &Value) -> Result<PathBuf, Failure> {
    let root = root.canonicalize()?;
    let path = root.join(path_text).canonicalize().map_err(|error| {
        fail(format!(
            "missing MiSTer framework file {path_text}: {error}"
        ))
    })?;
    if !path.starts_with(&root) || !path.is_file() {
        return Err(fail(format!(
            "MiSTer framework file escapes repository: {path_text}"
        )));
    }
    if fs::metadata(&path)?.len() != entry["bytes"].as_u64().unwrap_or(u64::MAX)
        || sha256(&path)? != entry["sha256"].as_str().unwrap_or("")
    {
        return Err(fail(format!(
            "MiSTer framework file hash mismatch: {path_text}"
        )));
    }
    Ok(path)
}

pub(super) fn validate_evidence(
    report: &Value,
    root: &Path,
    expected_commit: &str,
    require_clean: bool,
) -> Result<Vec<PathBuf>, Failure> {
    if report["schema"] != SCHEMA || report["status"] != "passed" {
        return Err(fail("MiSTer framework report schema/status is invalid"));
    }
    if report["framework"] != json!({"url": URL, "commit": COMMIT, "tree": TREE}) {
        return Err(fail("MiSTer framework identity is not pinned"));
    }
    if report["git"]["commit"] != expected_commit
        || (require_clean && report["git"]["dirty"] != false)
    {
        return Err(fail(
            "MiSTer framework Git identity or cleanliness is invalid",
        ));
    }
    let result = &report["result"];
    if result["status"] != "passed"
        || result["top"] != TOP
        || result["device"] != DEVICE
        || result["fitter_seed"] != FITTER_SEED
        || result["core_clock_mhz"] != CLOCK_MHZ
        || result["core_clock"]
            != json!({
                "target": CLOCK_TARGET, "period_ns": CLOCK_PERIOD_NS,
                "frequency_mhz": CLOCK_MHZ,
            })
    {
        return Err(fail("MiSTer framework result has wrong target identity"));
    }
    let waivers: BTreeSet<&str> = result["reviewed_upstream_waivers"]
        .as_array()
        .ok_or_else(|| fail("MiSTer framework waivers are missing"))?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    if waivers != REVIEWED_WAIVERS.into_iter().collect()
        || result["reviewed_upstream_waivers"]
            .as_array()
            .unwrap()
            .len()
            != REVIEWED_WAIVERS.len()
    {
        return Err(fail("MiSTer framework upstream waiver inventory changed"));
    }
    for field in ["alm", "register", "memory_bit", "dsp", "pll"] {
        if result["resources"][field].as_u64().is_none() {
            return Err(fail(format!(
                "MiSTer framework resource {field} is missing"
            )));
        }
    }
    let timing = &result["timing"];
    for key in ["minimum_setup_slack_ns", "minimum_hold_slack_ns"] {
        if !timing[key].as_f64().is_some_and(|slack| slack >= 0.0) {
            return Err(fail("MiSTer framework timing is not closed"));
        }
    }
    let unconstrained = result["unconstrained"]
        .as_object()
        .ok_or_else(|| fail("MiSTer framework unconstrained summary is missing"))?;
    if unconstrained.len() != UNCONSTRAINED_LABELS.len()
        || UNCONSTRAINED_LABELS
            .iter()
            .any(|name| unconstrained.get(*name) != Some(&json!({"setup": 0, "hold": 0})))
    {
        return Err(fail("MiSTer framework has unconstrained paths"));
    }
    let inputs = report["inputs"]
        .as_object()
        .ok_or_else(|| fail("MiSTer framework inputs are missing"))?;
    let canonical_root = root.canonicalize()?;
    let expected_inputs: BTreeSet<String> = crate::mister_framework::local_inputs(root)?
        .into_iter()
        .map(|path| -> Result<String, Failure> {
            Ok(path
                .canonicalize()?
                .strip_prefix(&canonical_root)
                .map_err(|_| fail("MiSTer framework input escaped repository"))?
                .to_string_lossy()
                .replace('\\', "/"))
        })
        .collect::<Result<_, _>>()?;
    if inputs.keys().cloned().collect::<BTreeSet<_>>() != expected_inputs {
        return Err(fail("MiSTer framework input inventory is incomplete"));
    }
    let mut candidates = Vec::new();
    for (name, entry) in inputs {
        candidates.push(verified_entry(root, name, entry)?);
    }
    let artifacts = report["artifacts"]
        .as_object()
        .ok_or_else(|| fail("MiSTer framework artifacts are missing"))?;
    let expected: BTreeSet<String> = REPORT_SUFFIXES
        .iter()
        .map(|suffix| format!("{PROJECT}{suffix}"))
        .chain(std::iter::once("build.log".to_owned()))
        .collect();
    if artifacts.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(fail("MiSTer framework artifact inventory is incomplete"));
    }
    for entry in artifacts.values() {
        let name = entry["path"].as_str().unwrap_or("");
        candidates.push(verified_entry(root, name, entry)?);
    }
    Ok(candidates)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Map;

    fn fixture(root: &Path) -> PathBuf {
        let output = root.join("output_files");
        fs::create_dir_all(&output).unwrap();
        let write = |suffix: &str, contents: &str| {
            fs::write(output.join(format!("{PROJECT}{suffix}")), contents).unwrap();
        };
        write(".flow.rpt", "Flow Status : Successful\n");
        write(".map.summary", "Analysis & Synthesis Status : Successful\n");
        write(".map.rpt", "arm7tdmi_generic_soc\narm7tdmi_mister\n");
        write(
            ".fit.summary",
            "Fitter Status : Successful\nTop-level Entity Name : sys_top\nDevice : 5CSEBA6U23I7\nLogic utilization (in ALMs) : 12,345 / 41,910\nTotal registers : 6789\nTotal block memory bits : 0 / 5,662,720\nTotal DSP Blocks : 6 / 112\nTotal PLLs : 4 / 6\n",
        );
        write(
            ".fit.rpt",
            "Fitter was successful\n; Fitter Initial Placement Seed ; 4 ; 1 ;\n",
        );
        write(".asm.rpt", "Assembler was successful\n");
        write(".sta.summary", "Type  : Slow Model Setup 'clk_sys'\nSlack : 1.250\nType  : Fast Model Hold 'clk_sys'\nSlack : 0.125\n");
        let mut sta = format!(
            "TimeQuest Timing Analyzer was successful\n; {CLOCK_TARGET} ; Generated ; 80.000 ; 12.5 MHz ;\n"
        );
        for label in UNCONSTRAINED_LABELS {
            sta.push_str(&format!("; {label} ; 0 ; 0 ;\n"));
        }
        write(".sta.rpt", &sta);
        fs::write(
            output.join(format!("{PROJECT}.rbf")),
            b"real-bitstream-fixture",
        )
        .unwrap();
        output
    }

    fn add_reviewed_waivers(output: &Path) -> PathBuf {
        let path = output.join(format!("{PROJECT}.sta.rpt"));
        let mut contents = fs::read_to_string(&path).unwrap();
        contents.push_str("Warning (332174): Ignored filter at sys_top.sdc(60): arc* could not be matched with a clock or keeper\n");
        contents.push_str("Warning (332174): Ignored filter at sys_top.sdc(61): arx* could not be matched with a keeper\n");
        contents.push_str("Warning (332174): Ignored filter at sys_top.sdc(61): ary* could not be matched with a keeper\n");
        contents.push_str("Warning (332049): Ignored set_false_path at sys_top.sdc(61): Argument <from> is not an object ID\n");
        fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn parser_accepts_exact_fit_timing_constraints_and_bitstream() {
        let temporary = tempfile::tempdir().unwrap();
        let output = fixture(temporary.path());
        let (result, errors) = parse_reports(&output);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(result["status"], "passed");
        assert_eq!(result["top"], TOP);
        assert_eq!(result["device"], DEVICE);
        assert_eq!(result["fitter_seed"], FITTER_SEED);
        assert_eq!(result["resources"]["alm"], 12_345);
        assert_eq!(result["timing"]["minimum_setup_slack_ns"], 1.25);
        assert_eq!(result["timing"]["minimum_hold_slack_ns"], 0.125);
        assert_eq!(result["core_clock"]["target"], CLOCK_TARGET);
        assert_eq!(result["core_clock"]["period_ns"], CLOCK_PERIOD_NS);
        assert_eq!(result["core_clock_mhz"], CLOCK_MHZ);
    }

    #[test]
    fn parser_rejects_each_signoff_weakening() {
        let mutations = [
            (".sta.summary", "Slack : 1.250", "Slack : -0.001"),
            (
                ".sta.rpt",
                "; Unconstrained Clocks ; 0 ; 0 ;",
                "; Unconstrained Clocks ; 1 ; 0 ;",
            ),
            (".map.rpt", "arm7tdmi_mister", "removed_core"),
            (
                ".fit.rpt",
                "Fitter was successful",
                "Critical Warning (999999): unsafe\nFitter was successful",
            ),
            (".sta.rpt", CLOCK_TARGET, "unrelated|pll|divclk"),
            (
                ".fit.rpt",
                "Fitter Initial Placement Seed ; 4",
                "Fitter Initial Placement Seed ; 3",
            ),
        ];
        for (suffix, old, new) in mutations {
            let temporary = tempfile::tempdir().unwrap();
            let output = fixture(temporary.path());
            let path = output.join(format!("{PROJECT}{suffix}"));
            let contents = fs::read_to_string(&path).unwrap();
            fs::write(&path, contents.replace(old, new)).unwrap();
            let (result, errors) = parse_reports(&output);
            assert_eq!(result["status"], "failed", "{suffix}");
            assert!(!errors.is_empty(), "{suffix}");
        }
    }

    #[test]
    fn only_exact_reviewed_upstream_waivers_are_accepted() {
        let temporary = tempfile::tempdir().unwrap();
        let output = fixture(temporary.path());
        let path = add_reviewed_waivers(&output);
        let contents = fs::read_to_string(&path).unwrap();
        let (result, errors) = parse_reports(&output);
        assert!(errors.is_empty(), "{errors:?}");
        let actual: BTreeSet<&str> = result["reviewed_upstream_waivers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect();
        assert_eq!(actual, REVIEWED_WAIVERS.into_iter().collect());
        fs::write(&path, contents.replace("arc*", "unsafe*")).unwrap();
        let (result, errors) = parse_reports(&output);
        assert_eq!(result["status"], "failed");
        assert!(errors.iter().any(|error| error.contains("unsafe*")));
    }

    #[test]
    fn report_validation_checks_file_hashes_and_paths() {
        let root = crate::campaign::repo_root();
        fs::create_dir_all(root.join(".tools")).unwrap();
        let temporary = tempfile::Builder::new()
            .prefix("mister-report-contract-")
            .tempdir_in(root.join(".tools"))
            .unwrap();
        let output = fixture(temporary.path());
        add_reviewed_waivers(&output);
        let (result, errors) = parse_reports(&output);
        assert!(errors.is_empty());
        let build_log = temporary.path().join("build.log");
        fs::write(&build_log, "build").unwrap();
        let mut inputs = Map::new();
        for path in crate::mister_framework::local_inputs(&root).unwrap() {
            let relative = path.strip_prefix(&root).unwrap().to_string_lossy();
            inputs.insert(
                relative.to_string(),
                json!({
                    "bytes": fs::metadata(&path).unwrap().len(),
                    "sha256": sha256(&path).unwrap(),
                }),
            );
        }
        let mut artifacts = Map::new();
        for suffix in REPORT_SUFFIXES {
            let name = format!("{PROJECT}{suffix}");
            let path = output.join(&name);
            artifacts.insert(
                name,
                json!({
                    "path": path.strip_prefix(&root).unwrap().to_string_lossy(),
                    "bytes": fs::metadata(&path).unwrap().len(),
                    "sha256": sha256(&path).unwrap(),
                }),
            );
        }
        artifacts.insert(
            "build.log".to_owned(),
            json!({
                "path": build_log.strip_prefix(&root).unwrap().to_string_lossy(),
                "bytes": 5, "sha256": sha256(&build_log).unwrap(),
            }),
        );
        let report = json!({
            "schema": SCHEMA, "status": "passed",
            "framework": {"url": URL, "commit": COMMIT, "tree": TREE},
            "git": {"commit": "abc", "dirty": false},
            "result": result,
            "inputs": inputs,
            "artifacts": artifacts,
        });
        assert_eq!(
            validate_evidence(&report, &root, "abc", true)
                .unwrap()
                .len(),
            crate::mister_framework::local_inputs(&root).unwrap().len() + 10
        );
        let report_path = temporary.path().join("framework-report.json");
        fs::write(&report_path, serde_json::to_vec(&report).unwrap()).unwrap();
        assert!(crate::mister_framework::command(&[
            "--validate-report".into(),
            report_path.to_string_lossy().into_owned(),
            "--expected-commit".into(),
            "abc".into(),
        ])
        .is_ok());
        fs::write(&build_log, "changed").unwrap();
        assert!(validate_evidence(&report, &root, "abc", true).is_err());
        assert!(validate_evidence(&report, &root, "other", true).is_err());
    }
}
