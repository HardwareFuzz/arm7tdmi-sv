//! Quartus fit/STA/power report contracts and four-profile option summary.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::campaign::{repo_root, write_json, Failure};

const DEVICE: &str = "5CSEBA6U23I7";
const DEFAULT_PROJECT: &str = "arm7tdmi_mister";
const DEFAULT_TOP: &str = "arm7tdmi_mister_example_top";
const REPORT_SCHEMA: &str = "arm7tdmi-quartus-report-v1";
const OPTIONS_SCHEMA: &str = "arm7tdmis-quartus-options-v1";
const PROFILES: [&str; 4] = ["none", "debug", "coprocessor", "both"];

fn fail(message: impl Into<String>) -> Failure {
    Failure::other(message)
}

fn defaults(option_profile: bool) -> BTreeMap<String, u64> {
    BTreeMap::from([
        ("alm".into(), if option_profile { 7_500 } else { 5_000 }),
        (
            "register".into(),
            if option_profile { 6_000 } else { 4_096 },
        ),
        ("dsp".into(), 8),
        ("memory_bit".into(), 0),
    ])
}

#[derive(Debug, Clone)]
struct Config {
    project: String,
    top: String,
    device: String,
    resource_limits: BTreeMap<String, u64>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            project: DEFAULT_PROJECT.into(),
            top: DEFAULT_TOP.into(),
            device: DEVICE.into(),
            resource_limits: defaults(false),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TimingEntry {
    #[serde(rename = "type")]
    path_type: String,
    slack_ns: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Fmax {
    corners: Vec<f64>,
    minimum: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Report {
    schema: String,
    project: String,
    top: Option<String>,
    device: Option<String>,
    resources: BTreeMap<String, Option<u64>>,
    resource_limits: BTreeMap<String, u64>,
    clock_enable_registers: Option<u64>,
    fmax_mhz: Fmax,
    power_mw: BTreeMap<String, Option<f64>>,
    power_confidence: Option<String>,
    power_warning_waivers: Vec<String>,
    timing: Vec<TimingEntry>,
    status: String,
}

fn capture(pattern: &str, contents: &str, group: usize) -> Option<String> {
    Regex::new(pattern)
        .expect("valid Quartus report regex")
        .captures(contents)
        .and_then(|fields| fields.get(group).map(|value| value.as_str().to_owned()))
}

fn integer_field(contents: &str, label: &str) -> Option<u64> {
    let pattern = format!(r"(?m)^{}\s*:\s*([\d,]+)", regex::escape(label));
    capture(&pattern, contents, 1)?
        .replace(',', "")
        .parse()
        .ok()
}

fn table_integer_field(contents: &str, label: &str) -> Option<u64> {
    let pattern = format!(r"(?m)^\s*;\s*{}\s*;\s*([\d,]+)\s*;", regex::escape(label));
    capture(&pattern, contents, 1)?
        .replace(',', "")
        .parse()
        .ok()
}

fn float_field(contents: &str, label: &str, unit: &str) -> Option<f64> {
    let pattern = format!(
        r"(?m)^{}\s*:\s*(-?\d+(?:\.\d+)?)\s*{}",
        regex::escape(label),
        regex::escape(unit),
    );
    capture(&pattern, contents, 1)?.parse().ok()
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

fn python_option_repr(value: Option<&str>) -> String {
    value.map_or("None".into(), |text| format!("'{text}'"))
}

fn validate_reports(output_dir: &Path, config: &Config) -> (Report, Vec<String>) {
    let mut errors = Vec::new();
    let read = |suffix: &str, errors: &mut Vec<String>| {
        read_report(
            &output_dir.join(format!("{}{suffix}", config.project)),
            errors,
        )
    };
    let flow = read(".flow.rpt", &mut errors);
    let map_summary = read(".map.summary", &mut errors);
    let map_report = read(".map.rpt", &mut errors);
    let fit_summary = read(".fit.summary", &mut errors);
    let fit_report = read(".fit.rpt", &mut errors);
    let asm_report = read(".asm.rpt", &mut errors);
    let sta_summary = read(".sta.summary", &mut errors);
    let sta_report = read(".sta.rpt", &mut errors);
    let power_summary = read(".pow.summary", &mut errors);
    let power_report = read(".pow.rpt", &mut errors);

    for (contents, marker, success, phase) in [
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
        (
            &power_summary,
            "PowerPlay Power Analyzer Status",
            "Successful",
            "PowerPlay",
        ),
        (
            &power_report,
            "PowerPlay Power Analyzer was successful",
            "",
            "PowerPlay report",
        ),
    ] {
        if !contents.contains(marker) || (!success.is_empty() && !contents.contains(success)) {
            errors.push(format!("{phase} success marker missing"));
        }
    }
    let top = capture(r"(?m)^Top-level Entity Name\s*:\s*(\S+)", &fit_summary, 1);
    let device = capture(r"(?m)^Device\s*:\s*(\S+)", &fit_summary, 1);
    if top.as_deref() != Some(config.top.as_str()) {
        errors.push(format!(
            "unexpected top: {}",
            python_option_repr(top.as_deref())
        ));
    }
    if device.as_deref() != Some(config.device.as_str()) {
        errors.push(format!(
            "unexpected device: {}",
            python_option_repr(device.as_deref())
        ));
    }
    if !sta_report.contains("fully constrained for setup requirements") {
        errors.push("setup paths are not fully constrained".into());
    }
    if !sta_report.contains("fully constrained for hold requirements") {
        errors.push("hold paths are not fully constrained".into());
    }
    let critical_code = Regex::new(r"Critical Warning \((\d+)\)").unwrap();
    let mut power_warning_waivers = Vec::new();
    for (name, contents) in [
        ("fit", &fit_report),
        ("sta", &sta_report),
        ("power", &power_report),
    ] {
        for line in contents.lines() {
            if line.contains("Critical Warning") {
                let code = critical_code
                    .captures(line)
                    .and_then(|fields| fields.get(1))
                    .map(|value| value.as_str())
                    .unwrap_or("");
                if name == "power" && code == "215050" {
                    power_warning_waivers.push(line.trim().to_owned());
                } else {
                    errors.push(format!("{name}: {}", line.trim()));
                }
            }
            if line.contains("Warning (332174): Ignored filter") {
                errors.push(format!("{name}: {}", line.trim()));
            }
        }
    }
    let timing_re = Regex::new(r"Type\s*:\s*(.+?)\nSlack\s*:\s*(-?\d+(?:\.\d+)?)").unwrap();
    let mut timing = Vec::new();
    for fields in timing_re.captures_iter(&sta_summary) {
        let path_type = fields[1].trim().to_owned();
        let slack_ns: f64 = fields[2].parse().expect("matched numeric slack");
        if slack_ns < 0.0 {
            errors.push(format!("negative slack {slack_ns:.3} ns: {path_type}"));
        }
        timing.push(TimingEntry {
            path_type,
            slack_ns,
        });
    }
    if timing.is_empty() {
        errors.push("no timing slack entries found".into());
    }
    let resources = BTreeMap::from([
        (
            "alm".into(),
            integer_field(&fit_summary, "Logic utilization (in ALMs)"),
        ),
        (
            "register".into(),
            integer_field(&fit_summary, "Total registers"),
        ),
        (
            "memory_bit".into(),
            integer_field(&fit_summary, "Total block memory bits"),
        ),
        (
            "dsp".into(),
            integer_field(&fit_summary, "Total DSP Blocks"),
        ),
    ]);
    // Preserve the established budget-check order in diagnostics.
    for resource in ["alm", "register", "dsp", "memory_bit"] {
        let Some(limit) = config.resource_limits.get(resource) else {
            continue;
        };
        match resources.get(resource).copied().flatten() {
            None => errors.push(format!("missing resource field: {resource}")),
            Some(value) if value > *limit => {
                errors.push(format!("{resource} budget exceeded: {value} > {limit}"))
            }
            _ => {}
        }
    }
    let clock_enable_registers =
        table_integer_field(&map_report, "Number of registers using Clock Enable");
    match clock_enable_registers {
        None => errors.push("missing clock-enable register inference field".into()),
        Some(0) => errors.push("no registers inferred with clock enable".into()),
        _ => {}
    }
    let fmax_re =
        Regex::new(r"(?m)^\s*;\s*(\d+(?:\.\d+)?)\s+MHz\s*;\s*\d+(?:\.\d+)?\s+MHz\s*;\s*CLK\s*;")
            .unwrap();
    let fmax_values: Vec<f64> = fmax_re
        .captures_iter(&sta_report)
        .map(|fields| fields[1].parse().expect("matched numeric Fmax"))
        .collect();
    if fmax_values.is_empty() {
        errors.push("no CLK Fmax entries found".into());
    }
    let fmax_minimum = fmax_values.iter().copied().reduce(f64::min);
    let power_mw = BTreeMap::from([
        (
            "total".into(),
            float_field(&power_summary, "Total Thermal Power Dissipation", "mW"),
        ),
        (
            "core_dynamic".into(),
            float_field(
                &power_summary,
                "Core Dynamic Thermal Power Dissipation",
                "mW",
            ),
        ),
        (
            "core_static".into(),
            float_field(
                &power_summary,
                "Core Static Thermal Power Dissipation",
                "mW",
            ),
        ),
        (
            "io".into(),
            float_field(&power_summary, "I/O Thermal Power Dissipation", "mW"),
        ),
    ]);
    for field in ["total", "core_dynamic", "core_static", "io"] {
        if power_mw[field].is_none() {
            errors.push(format!("missing power field: {field}"));
        }
    }
    let power_confidence = capture(
        r"(?m)^Power Estimation Confidence\s*:\s*(.+)$",
        &power_summary,
        1,
    )
    .map(|text| text.trim().to_owned());
    if power_confidence.is_none() {
        errors.push("missing power estimation confidence".into());
    }
    let image = output_dir.join(format!("{}.sof", config.project));
    if fs::metadata(&image).map_or(true, |metadata| metadata.len() == 0) {
        errors.push(format!(
            "missing or empty programming image: {}.sof",
            config.project
        ));
    }
    let report = Report {
        schema: REPORT_SCHEMA.into(),
        project: config.project.clone(),
        top,
        device,
        resources,
        resource_limits: config.resource_limits.clone(),
        clock_enable_registers,
        fmax_mhz: Fmax {
            corners: fmax_values,
            minimum: fmax_minimum,
        },
        power_mw,
        power_confidence,
        power_warning_waivers,
        timing,
        status: if errors.is_empty() {
            "passed"
        } else {
            "failed"
        }
        .into(),
    };
    (report, errors)
}

fn required_u64(report: &Report, key: &str) -> Result<u64, Failure> {
    report
        .resources
        .get(key)
        .copied()
        .flatten()
        .ok_or_else(|| fail(format!("{} option report lacks {key}", report.project)))
}

fn required_power(report: &Report) -> Result<f64, Failure> {
    report
        .power_mw
        .get("core_dynamic")
        .copied()
        .flatten()
        .ok_or_else(|| {
            fail(format!(
                "{} option report lacks core dynamic power",
                report.project
            ))
        })
}

fn round3(value: f64) -> f64 {
    (value * 1000.0).round_ties_even() / 1000.0
}

fn summarize_profiles(profiles: &BTreeMap<String, Report>) -> Result<Value, Failure> {
    if profiles.len() != PROFILES.len() || PROFILES.iter().any(|name| !profiles.contains_key(*name))
    {
        return Err(fail(format!(
            "option profiles must be exactly: {}",
            PROFILES.join(", ")
        )));
    }
    for (name, report) in profiles {
        if report.status != "passed"
            || report.top.as_deref() != Some("arm7tdmi_mister")
            || report.device.as_deref() != Some(DEVICE)
        {
            return Err(fail(format!(
                "{name} option report did not pass or has wrong target"
            )));
        }
    }
    let baseline = &profiles["none"];
    let mut deltas = BTreeMap::new();
    for name in ["debug", "coprocessor", "both"] {
        let report = &profiles[name];
        let mut resource_deltas = BTreeMap::new();
        for field in ["alm", "register", "memory_bit", "dsp"] {
            resource_deltas.insert(
                field,
                required_u64(report, field)? as i64 - required_u64(baseline, field)? as i64,
            );
        }
        let fmax = round3(
            report
                .fmax_mhz
                .minimum
                .ok_or_else(|| fail("option Fmax missing"))?
                - baseline
                    .fmax_mhz
                    .minimum
                    .ok_or_else(|| fail("baseline Fmax missing"))?,
        );
        let power = round3(required_power(report)? - required_power(baseline)?);
        deltas.insert(
            name,
            json!({
                "alm": resource_deltas["alm"],
                "register": resource_deltas["register"],
                "memory_bit": resource_deltas["memory_bit"],
                "dsp": resource_deltas["dsp"],
                "fmax_mhz": fmax,
                "core_dynamic_power_mw": power,
            }),
        );
    }
    Ok(json!({
        "schema": OPTIONS_SCHEMA, "status": "passed", "device": DEVICE,
        "top": "arm7tdmi_mister", "profiles": profiles, "deltas": deltas,
    }))
}

fn collect_profiles(root: &Path) -> Result<BTreeMap<String, Report>, Failure> {
    let mut profiles = BTreeMap::new();
    let mut errors = Vec::new();
    for name in PROFILES {
        let config = Config {
            project: format!("arm7tdmi_option_{name}"),
            top: "arm7tdmi_mister".into(),
            device: DEVICE.into(),
            resource_limits: defaults(true),
        };
        let output = root.join("fpga/option_output").join(name);
        let (report, report_errors) = validate_reports(&output, &config);
        errors.extend(
            report_errors
                .into_iter()
                .map(|error| format!("{name}: {error}")),
        );
        profiles.insert(name.to_owned(), report);
    }
    if !errors.is_empty() {
        return Err(fail(errors.join("\n")));
    }
    Ok(profiles)
}

pub fn report_command(arguments: &[String]) -> Result<(), String> {
    let mut config = Config::default();
    let mut directory: Option<PathBuf> = None;
    let mut index = 0;
    while index < arguments.len() {
        let value = &arguments[index];
        if value.starts_with("--") {
            let next = arguments
                .get(index + 1)
                .ok_or_else(|| format!("missing value for {value}"))?;
            match value.as_str() {
                "--project" => config.project = next.clone(),
                "--top" => config.top = next.clone(),
                "--device" => config.device = next.clone(),
                "--max-alm" | "--max-register" | "--max-dsp" | "--max-memory-bit" => {
                    let key = value.trim_start_matches("--max-").replace('-', "_");
                    config.resource_limits.insert(
                        key,
                        next.parse::<u64>()
                            .map_err(|_| format!("invalid {value}"))?,
                    );
                }
                _ => return Err(format!("unknown option: {value}")),
            }
            index += 2;
        } else {
            if directory.replace(PathBuf::from(value)).is_some() {
                return Err("expected one Quartus output directory".into());
            }
            index += 1;
        }
    }
    let directory = directory.ok_or_else(|| "missing Quartus output directory".to_owned())?;
    let (report, errors) = validate_reports(&directory, &config);
    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?
    );
    for error in &errors {
        eprintln!("[quartus-report] FAIL: {error}");
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!("{} Quartus report errors", errors.len()))
    }
}

pub fn options_command(arguments: &[String]) -> Result<(), String> {
    let root = repo_root();
    let output = match arguments {
        [] => root.join("reports/generated/quartus-options.json"),
        [flag, path] if flag == "--output" => PathBuf::from(path),
        _ => return Err("expected optional --output PATH".into()),
    };
    let profiles = collect_profiles(&root).map_err(|error| error.to_string())?;
    let summary = summarize_profiles(&profiles).map_err(|error| error.to_string())?;
    write_json(&output, &summary).map_err(|error| error.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&summary).map_err(|error| error.to_string())?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(directory: &Path, project: &str, top: &str) {
        fs::create_dir_all(directory).unwrap();
        let write = |suffix: &str, contents: &str| {
            fs::write(directory.join(format!("{project}{suffix}")), contents).unwrap();
        };
        write(".flow.rpt", "Flow Status ; Successful\n");
        write(".map.summary", "Analysis & Synthesis Status : Successful\n");
        write(
            ".map.rpt",
            "; Number of registers using Clock Enable ; 2600 ;\n",
        );
        write(".fit.summary", &format!("Fitter Status : Successful\nTop-level Entity Name : {top}\nDevice : {DEVICE}\nLogic utilization (in ALMs) : 3283 / 41910\nTotal registers : 2706\nTotal block memory bits : 0 / 5662720\nTotal DSP Blocks : 6 / 112\n"));
        write(".fit.rpt", "Fitter was successful\n");
        write(".asm.rpt", "Assembler was successful\n");
        write(
            ".sta.summary",
            "Type  : Slow Setup 'CLK'\nSlack : 1.250\nType  : Fast Hold 'CLK'\nSlack : 0.100\n",
        );
        write(".sta.rpt", "Design is fully constrained for setup requirements\nDesign is fully constrained for hold requirements\n; 42.50 MHz ; 42.50 MHz ; CLK ; ;\n; 45.00 MHz ; 45.00 MHz ; CLK ; ;\nTimeQuest Timing Analyzer was successful\n");
        write(".pow.summary", "PowerPlay Power Analyzer Status : Successful\nTotal Thermal Power Dissipation : 450.07 mW\nCore Dynamic Thermal Power Dissipation : 28.96 mW\nCore Static Thermal Power Dissipation : 412.43 mW\nI/O Thermal Power Dissipation : 8.68 mW\nPower Estimation Confidence : Low: vectorless fixture\n");
        write(".pow.rpt", "PowerPlay Power Analyzer was successful\n");
        fs::write(directory.join(format!("{project}.sof")), b"non-empty").unwrap();
    }

    #[test]
    fn complete_report_and_each_required_failure() {
        let temporary = tempfile::tempdir().unwrap();
        fixture(temporary.path(), DEFAULT_PROJECT, DEFAULT_TOP);
        let (report, errors) = validate_reports(temporary.path(), &Config::default());
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(report.status, "passed");
        assert_eq!(report.resources["alm"], Some(3283));
        assert_eq!(report.fmax_mhz.minimum, Some(42.5));
        assert_eq!(report.power_mw["core_dynamic"], Some(28.96));
        for (suffix, original, changed, expected) in [
            (
                ".sta.summary",
                "Slack : 1.250",
                "Slack : -0.250",
                "negative slack",
            ),
            (
                ".fit.rpt",
                "Fitter was successful",
                "Critical Warning (169085): pins are not assigned",
                "Critical Warning",
            ),
            (
                ".sta.rpt",
                "; 42.50 MHz ; 42.50 MHz ; CLK ; ;\n; 45.00 MHz ; 45.00 MHz ; CLK ; ;\n",
                "",
                "Fmax",
            ),
            (
                ".pow.summary",
                "Total Thermal Power Dissipation : 450.07 mW",
                "",
                "power field",
            ),
        ] {
            let temporary = tempfile::tempdir().unwrap();
            fixture(temporary.path(), DEFAULT_PROJECT, DEFAULT_TOP);
            let path = temporary.path().join(format!("{DEFAULT_PROJECT}{suffix}"));
            fs::write(
                &path,
                fs::read_to_string(&path)
                    .unwrap()
                    .replace(original, changed),
            )
            .unwrap();
            let (report, errors) = validate_reports(temporary.path(), &Config::default());
            assert_eq!(report.status, "failed");
            assert!(
                errors.iter().any(|error| error.contains(expected)),
                "{errors:?}"
            );
        }
        let missing_image = tempfile::tempdir().unwrap();
        fixture(missing_image.path(), DEFAULT_PROJECT, DEFAULT_TOP);
        fs::remove_file(missing_image.path().join(format!("{DEFAULT_PROJECT}.sof"))).unwrap();
        let (report, errors) = validate_reports(missing_image.path(), &Config::default());
        assert_eq!(report.status, "failed");
        assert!(errors
            .iter()
            .any(|error| error.contains("programming image")));

        let narrow_budget = Config {
            resource_limits: BTreeMap::from([
                ("alm".into(), 3_000),
                ("register".into(), 4_096),
                ("dsp".into(), 8),
                ("memory_bit".into(), 0),
            ]),
            ..Config::default()
        };
        let (_, errors) = validate_reports(temporary.path(), &narrow_budget);
        assert!(errors
            .iter()
            .any(|error| error.contains("alm budget exceeded")));
    }

    #[test]
    fn option_deltas_use_shared_baseline() {
        let temporary = tempfile::tempdir().unwrap();
        let mut profiles = BTreeMap::new();
        for (name, alm, registers, fmax, power) in [
            ("none", 3000, 2400, 30.0, 25.0),
            ("debug", 3600, 3300, 29.0, 29.0),
            ("coprocessor", 3200, 2500, 28.5, 27.0),
            ("both", 3800, 3400, 28.0, 31.0),
        ] {
            fixture(
                temporary.path(),
                &format!("arm7tdmi_option_{name}"),
                "arm7tdmi_mister",
            );
            let config = Config {
                project: format!("arm7tdmi_option_{name}"),
                top: "arm7tdmi_mister".into(),
                device: DEVICE.into(),
                resource_limits: defaults(true),
            };
            let (mut report, errors) = validate_reports(temporary.path(), &config);
            assert!(errors.is_empty());
            report.resources.insert("alm".into(), Some(alm));
            report.resources.insert("register".into(), Some(registers));
            report.fmax_mhz.minimum = Some(fmax);
            report.power_mw.insert("core_dynamic".into(), Some(power));
            profiles.insert(name.into(), report);
        }
        let summary = summarize_profiles(&profiles).unwrap();
        assert_eq!(summary["deltas"]["debug"]["alm"], 600);
        assert_eq!(summary["deltas"]["debug"]["register"], 900);
        assert_eq!(summary["deltas"]["coprocessor"]["alm"], 200);
        assert_eq!(summary["deltas"]["both"]["register"], 1000);
        assert_eq!(summary["deltas"]["both"]["fmax_mhz"], -2.0);
        assert_eq!(
            summary["deltas"]["coprocessor"]["core_dynamic_power_mw"],
            2.0
        );
        profiles.remove("both");
        assert!(summarize_profiles(&profiles).is_err());
    }

    #[test]
    fn option_projects_share_target_constraints_and_auto_fit_policy() {
        let root = repo_root();
        for (profile, debug, coprocessor) in [
            ("none", "0", "0"),
            ("debug", "1", "0"),
            ("coprocessor", "0", "1"),
            ("both", "1", "1"),
        ] {
            let qsf = fs::read_to_string(root.join(format!("fpga/arm7tdmi_option_{profile}.qsf")))
                .unwrap();
            for required in [
                "TOP_LEVEL_ENTITY arm7tdmi_mister".to_owned(),
                format!("DEVICE {DEVICE}"),
                "QIP_FILE arm7tdmi_mister.qip".to_owned(),
                "SDC_FILE arm7tdmi_options.sdc".to_owned(),
                "FITTER_EFFORT \"AUTO FIT\"".to_owned(),
                format!("set_parameter -name ENABLE_DEBUG {debug}"),
                format!("set_parameter -name ENABLE_COPROCESSOR {coprocessor}"),
            ] {
                assert!(qsf.contains(&required), "{profile}: missing {required}");
            }
        }
        assert!(root.join("fpga/arm7tdmi_options.sdc").is_file());
    }
}
