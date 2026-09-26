//! Compile every public synthesis top with the pinned independent Slang frontend.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use chrono::{SecondsFormat, Utc};
use regex::Regex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::campaign::{repo_root, run_command};
use crate::pinned_archive::file_sha256;

const SCHEMA: &str = "arm7tdmis-independent-lint-v1";
const EXPECTED_VERSION: &str = "slang version 11.0.0+7ddf4059f";
const TOPS: &[(&str, &[&str])] = &[
    ("arm7tdmis_top", &[]),
    ("arm7tdmi_mister", &[]),
    ("arm7tdmis_no_dft", &[]),
    (
        "arm7tdmi_mister_example_top",
        &["../fpga/example/arm7tdmi_mister_example_top.sv"],
    ),
    (
        "arm7tdmi_generic_soc",
        &["../examples/generic_soc/arm7tdmi_generic_soc.sv"],
    ),
];

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, false)
}

fn capture(command: &[String], cwd: &Path) -> Result<(i32, String), String> {
    let mut temporary = tempfile::tempfile().map_err(|error| error.to_string())?;
    let stdout = temporary.try_clone().map_err(|error| error.to_string())?;
    let stderr = temporary.try_clone().map_err(|error| error.to_string())?;
    let status = Command::new(&command[0])
        .args(&command[1..])
        .current_dir(cwd)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .status()
        .map_err(|error| error.to_string())?;
    temporary
        .seek(SeekFrom::Start(0))
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    temporary
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    let output = String::from_utf8(bytes).map_err(|error| error.to_string())?;
    Ok((status.code().unwrap_or(1), output))
}

fn find_executable(candidate: &Path) -> Result<PathBuf, String> {
    if candidate.is_file() {
        return fs::canonicalize(candidate).map_err(|error| error.to_string());
    }
    if candidate.components().count() == 1 {
        for directory in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
            let located = directory.join(candidate);
            if located.is_file() {
                return fs::canonicalize(located).map_err(|error| error.to_string());
            }
        }
    }
    Err(format!(
        "Slang executable is missing: {}",
        candidate.display()
    ))
}

fn commands(slang: &Path, output: &Path) -> Result<Vec<(String, Vec<String>, PathBuf)>, String> {
    let parent = output
        .parent()
        .ok_or("independent lint output has no parent")?;
    let mut commands = Vec::new();
    for (top, sources) in TOPS {
        let diagnostics = parent.join(format!("slang-{top}-diagnostics.json"));
        let mut command = vec![
            slang.display().to_string(),
            "--allow-use-before-declare".into(),
            "--diag-option".into(),
            "--diag-json".into(),
            diagnostics.display().to_string(),
            "--top".into(),
            (*top).into(),
            "-f".into(),
            "sim.f".into(),
        ];
        command.extend(sources.iter().map(|source| (*source).to_owned()));
        commands.push(((*top).into(), command, diagnostics));
    }
    Ok(commands)
}

fn rtl_sources(directory: &Path, sources: &mut BTreeSet<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.is_dir() {
            rtl_sources(&path, sources)?;
        } else if path.extension().is_some_and(|extension| extension == "sv") {
            sources.insert(path);
        }
    }
    Ok(())
}

fn source_inputs(root: &Path) -> Result<BTreeSet<PathBuf>, String> {
    let mut sources = BTreeSet::new();
    rtl_sources(&root.join("rtl"), &mut sources)?;
    for path in [
        "scripts/sim.f",
        "verification/constrained_random_rs/src/independent_lint.rs",
        "verification/constrained_random_rs/src/main.rs",
        "verification/constrained_random_rs/src/campaign.rs",
        "verification/constrained_random_rs/src/pinned_archive.rs",
        "verification/constrained_random_rs/Cargo.toml",
        "verification/constrained_random_rs/Cargo.lock",
        "fpga/example/arm7tdmi_mister_example_top.sv",
        "examples/generic_soc/arm7tdmi_generic_soc.sv",
    ] {
        sources.insert(root.join(path));
    }
    Ok(sources)
}

fn git_text(parts: &[&str]) -> Result<String, String> {
    let command: Vec<_> = parts.iter().map(|value| (*value).to_owned()).collect();
    run_command(&command, None)
        .map(|output| output.trim().to_owned())
        .map_err(|error| error.to_string())
}

fn write_report(path: &Path, report: &Value) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("independent lint output has no parent")?;
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

fn run_lint(slang: &Path, output: &Path) -> Result<String, String> {
    let root = repo_root();
    let scripts = root.join("scripts");
    let slang = find_executable(slang)?;
    let (version_status, version_output) =
        capture(&[slang.display().to_string(), "--version".into()], &scripts)?;
    if version_status != 0 {
        return Err(format!(
            "Slang --version exited {version_status}: {version_output}"
        ));
    }
    let version = version_output.trim();
    if version != EXPECTED_VERSION {
        return Err(format!("expected {EXPECTED_VERSION}, found {version}"));
    }
    let status_text = git_text(&["git", "status", "--porcelain=v1", "--untracked-files=all"])?;
    let mut inputs = BTreeMap::new();
    for path in source_inputs(&root)? {
        let relative = path
            .strip_prefix(&root)
            .map_err(|error| error.to_string())?;
        inputs.insert(
            relative.to_string_lossy().to_string(),
            json!({
                "bytes": fs::metadata(&path).map_err(|error| error.to_string())?.len(),
                "sha256": file_sha256(&path)?,
            }),
        );
    }
    let mut report = json!({
        "schema": SCHEMA,
        "created_utc": now(),
        "status": "running",
        "git": {
            "commit": git_text(&["git", "rev-parse", "HEAD"])?,
            "dirty": !status_text.is_empty(),
            "status": status_text.lines().collect::<Vec<_>>(),
        },
        "tool": {
            "path": slang.display().to_string(),
            "version": version,
            "bytes": fs::metadata(&slang).map_err(|error| error.to_string())?.len(),
            "sha256": file_sha256(&slang)?,
        },
        "inputs": inputs,
        "results": [],
    });
    write_report(output, &report)?;
    let summary = Regex::new(r"Build (?:succeeded|failed):\s*(\d+) errors?,\s*(\d+) warnings?")
        .map_err(|error| error.to_string())?;
    for (top, command, diagnostics) in commands(&slang, output)? {
        println!("[independent-lint] START {top}");
        let (code, text) = capture(&command, &scripts)?;
        let counts = summary.captures(&text);
        let errors: i64 = counts
            .as_ref()
            .and_then(|value| value.get(1))
            .and_then(|value| value.as_str().parse().ok())
            .unwrap_or(-1);
        let warnings: i64 = counts
            .as_ref()
            .and_then(|value| value.get(2))
            .and_then(|value| value.as_str().parse().ok())
            .unwrap_or(-1);
        let passed = code == 0 && errors == 0 && warnings == 0;
        let relative = diagnostics
            .strip_prefix(&root)
            .map_err(|error| error.to_string())?;
        let diagnostic_info = if diagnostics.is_file() {
            json!({
                "path": relative.to_string_lossy(),
                "bytes": fs::metadata(&diagnostics).map_err(|error| error.to_string())?.len(),
                "sha256": file_sha256(&diagnostics)?,
            })
        } else {
            json!({"path": relative.to_string_lossy(), "bytes": 0, "sha256": null})
        };
        report["results"]
            .as_array_mut()
            .ok_or("invalid results array")?
            .push(json!({
                "top": top,
                "command": command,
                "exit_code": code,
                "error_count": errors,
                "warning_count": warnings,
                "output_sha256": format!("{:x}", Sha256::digest(text.as_bytes())),
                "diagnostics": diagnostic_info,
                "status": if passed { "passed" } else { "failed" },
            }));
        println!(
            "[independent-lint] {} {top} (errors={errors}, warnings={warnings})",
            if passed { "PASSED" } else { "FAILED" }
        );
        if !passed {
            print!("{text}");
            report["status"] = json!("failed");
            write_report(output, &report)?;
            return Err(format!("independent lint failed for {top}"));
        }
        write_report(output, &report)?;
    }
    report["status"] = json!("passed");
    report["finished_utc"] = json!(now());
    report["result_count"] = json!(report["results"].as_array().unwrap().len());
    write_report(output, &report)?;
    Ok(format!(
        "[independent-lint] PASS ({} public tops)",
        TOPS.len()
    ))
}

pub fn command(arguments: &[String]) -> Result<String, String> {
    let mut slang = None;
    let mut output = repo_root().join("reports/generated/independent-lint.json");
    let mut args = arguments.iter();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--slang" => slang = Some(PathBuf::from(args.next().ok_or("missing --slang value")?)),
            "--output" => output = PathBuf::from(args.next().ok_or("missing --output value")?),
            other => return Err(format!("unknown independent-lint argument: {other}")),
        }
    }
    let slang = slang.ok_or("missing --slang")?;
    let output = if output.is_absolute() {
        output
    } else {
        std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(output)
    };
    run_lint(&slang, &output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_public_synthesis_top_has_a_command() {
        let output = Path::new("/tmp/independent-lint.json");
        let commands = commands(Path::new("/tools/slang"), output).unwrap();
        assert_eq!(commands.len(), 5);
        assert_eq!(commands[0].0, "arm7tdmis_top");
        assert_eq!(commands[4].0, "arm7tdmi_generic_soc");
        assert!(commands[4]
            .1
            .iter()
            .any(|arg| arg.ends_with("arm7tdmi_generic_soc.sv")));
    }
}
