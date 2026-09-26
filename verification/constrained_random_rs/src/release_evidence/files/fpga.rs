//! Independent release checks for FPGA quality, post-fit, and MiSTer evidence.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::{checked_artifact, read_json, repo_file};

fn validated_report(
    root: &Path,
    path: &Path,
    schema: &str,
    label: &str,
    commit: &str,
) -> Result<(PathBuf, Value), String> {
    let path = repo_file(root, &path.to_string_lossy())?;
    let report = read_json(&path)?;
    if report["schema"] != schema || report["status"] != "passed" {
        return Err(format!("{label} report is not passed"));
    }
    if report["git"]["dirty"] != false {
        return Err(format!("{label} report describes a dirty source tree"));
    }
    if report["git"]["commit"] != commit {
        return Err(format!("{label} report commit does not match regression"));
    }
    Ok((path, report))
}

fn check_inputs(root: &Path, inputs: &Value, label: &str) -> Result<(), String> {
    for (path_text, entry) in inputs
        .as_object()
        .ok_or_else(|| format!("{label} input manifest is malformed"))?
    {
        let path = repo_file(root, path_text)?;
        let actual = super::super::file_entry(&path, root)?;
        if actual["bytes"] != entry["bytes"] || actual["sha256"] != entry["sha256"] {
            return Err(format!("{label} input hash mismatch"));
        }
    }
    Ok(())
}

fn add_quality(
    root: &Path,
    report_root: &Path,
    commit: &str,
    files: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    let (lint_path, lint) = validated_report(
        root,
        &report_root.join("independent-lint.json"),
        "arm7tdmis-independent-lint-v1",
        "independent lint",
        commit,
    )?;
    let (cdc_path, cdc) = validated_report(
        root,
        &report_root.join("cdc-rdc.json"),
        "arm7tdmis-cdc-rdc-v1",
        "CDC/RDC",
        commit,
    )?;
    let results = lint["results"]
        .as_array()
        .ok_or("independent lint did not pass every public top")?;
    let expected_tops = BTreeSet::from([
        "arm7tdmis_top",
        "arm7tdmi_mister",
        "arm7tdmis_no_dft",
        "arm7tdmi_mister_example_top",
        "arm7tdmi_generic_soc",
    ]);
    if lint["result_count"] != results.len()
        || results
            .iter()
            .filter_map(|entry| entry["top"].as_str())
            .collect::<BTreeSet<_>>()
            != expected_tops
        || results.iter().any(|entry| {
            entry["status"] != "passed"
                || entry["exit_code"] != 0
                || entry["error_count"] != 0
                || entry["warning_count"] != 0
        })
    {
        return Err("independent lint did not pass every public top".into());
    }
    if lint["tool"]["version"] != "slang version 11.0.0+7ddf4059f" {
        return Err("independent lint used the wrong Slang version".into());
    }
    for entry in results {
        files.insert(checked_artifact(
            root,
            &entry["diagnostics"],
            "independent lint diagnostic",
        )?);
    }
    check_inputs(root, &lint["inputs"], "independent lint")?;

    if cdc["clock_domains"] != json!(["CLK"])
        || cdc["violations"]
            .as_array()
            .is_none_or(|rows| !rows.is_empty())
        || cdc["synchronizer_count"]
            .as_u64()
            .is_none_or(|count| count < 6)
        || cdc["reset_release_count"]
            .as_u64()
            .is_none_or(|count| count < 2)
        || cdc["reset_synchronizer_primitive"]["status"] != "verified"
    {
        return Err("CDC/RDC report does not prove reviewed closure".into());
    }
    let manifest = &cdc["manifest"];
    let manifest_path = repo_file(
        root,
        manifest["path"]
            .as_str()
            .ok_or("CDC/RDC manifest path is missing")?,
    )?;
    let actual = super::super::file_entry(&manifest_path, root)?;
    if actual["sha256"] != manifest["sha256"] {
        return Err("CDC/RDC manifest hash mismatch".into());
    }
    let checker_path = checked_artifact(root, &cdc["checker"], "CDC/RDC checker")?;
    check_inputs(root, &cdc["inputs"], "CDC/RDC")?;
    files.extend([lint_path, cdc_path, manifest_path, checker_path]);
    Ok(())
}

fn add_postfit(
    root: &Path,
    report_root: &Path,
    commit: &str,
    files: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    let (report_path, report) = validated_report(
        root,
        &report_root.join("postfit-report.json"),
        "arm7tdmis-postfit-v1",
        "post-fit",
        commit,
    )?;
    if report["device"] != "5CSEBA6U23I7" {
        return Err("post-fit report targets the wrong device".into());
    }
    const QUARTUS_VERSION: &str = "Version 17.0.2 Build 602 07/19/2017 SJ Lite Edition";
    for tool in ["quartus_map", "quartus_fit", "quartus_eda"] {
        if !report["tools"][tool]["version"]
            .as_str()
            .is_some_and(|version| version.contains(QUARTUS_VERSION))
        {
            return Err(format!("post-fit report used the wrong {tool} version"));
        }
    }
    let profiles = report["profiles"]
        .as_object()
        .ok_or("post-fit report lacks both endian profiles")?;
    if profiles.keys().map(String::as_str).collect::<BTreeSet<_>>()
        != BTreeSet::from(["little", "big"])
    {
        return Err("post-fit report lacks both endian profiles".into());
    }
    let expected_primitives = BTreeSet::from([
        "dffeas",
        "cyclonev_lcell_comb",
        "cyclonev_io_ibuf",
        "cyclonev_io_obuf",
        "cyclonev_clkena",
    ]);
    for (name, big_endian) in [("little", false), ("big", true)] {
        let profile = &profiles[name];
        let metrics = &profile["metrics"];
        let netlist = &profile["netlist"];
        if profile["status"] != "passed"
            || profile["big_endian"] != big_endian
            || profile["device"] != "5CSEBA6U23I7"
            || profile["synthesis_policy"]["auto_dsp_recognition"] != "off"
            || super::super::string_set(&profile["critical_warning_ids"])
                .as_ref()
                .is_none_or(|set| {
                    set.iter().map(String::as_str).collect::<BTreeSet<_>>()
                        != BTreeSet::from(["169085", "332012"])
                })
            || super::super::string_set(&netlist["primitives"])
                .as_ref()
                .is_none_or(|set| {
                    set.iter().map(String::as_str).collect::<BTreeSet<_>>() != expected_primitives
                })
            || netlist["bytes"].as_u64().is_none_or(|count| count == 0)
            || !super::super::valid_sha256(&netlist["sha256"])
            || metrics["accepted_transactions"]
                .as_u64()
                .is_none_or(|count| count < 30)
            || metrics["accepted_while_cpu_ce_low"]
                .as_u64()
                .is_none_or(|count| count < 1)
            || metrics["longest_wait_cycles"]
                .as_u64()
                .is_none_or(|count| count < 2)
        {
            return Err(format!("post-fit {name} profile is incomplete"));
        }
        for phase in ["map", "fit", "eda", "verilator", "simulation"] {
            files.insert(checked_artifact(
                root,
                &profile["logs"][phase],
                &format!("post-fit {name}/{phase} log"),
            )?);
        }
    }
    check_inputs(root, &report["inputs"], "post-fit")?;
    files.insert(report_path);
    Ok(())
}

fn add_framework(
    root: &Path,
    report_root: &Path,
    commit: &str,
    files: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    let report_path = repo_file(
        root,
        &report_root
            .join("mister-framework-report.json")
            .to_string_lossy(),
    )?;
    let report = read_json(&report_path)?;
    let candidates = crate::mister_framework_reports::validate_evidence(
        &report, root, commit, true,
    )
    .map_err(|error| format!("MiSTer framework evidence failed Rust validation: {error}"))?;
    for candidate in candidates {
        files.insert(repo_file(root, &candidate.to_string_lossy())?);
    }
    files.insert(report_path);
    Ok(())
}

pub(super) fn add_files(
    root: &Path,
    report_root: &Path,
    phases: &BTreeSet<String>,
    commit: &str,
    files: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    let lint = phases.contains("lint-independent");
    let cdc = phases.contains("cdc-rdc");
    if lint != cdc {
        return Err("regression contains incomplete FPGA-quality phases".into());
    }
    if lint {
        add_quality(root, report_root, commit, files)?;
    }
    if phases.contains("postfit-sim") {
        add_postfit(root, report_root, commit, files)?;
    }
    if phases.contains("mister-framework") {
        add_framework(root, report_root, commit, files)?;
    }
    Ok(())
}
