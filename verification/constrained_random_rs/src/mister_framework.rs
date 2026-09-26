//! Build the ARM7 overlay inside the pinned official MiSTer framework.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Write};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Map, Value};

use crate::campaign::{repo_root, sha256, utc_now, write_json, Failure};
use crate::coverage_report::source_digest;
use crate::mister_framework_reports::{
    self as reports, CLOCK_MHZ, COMMIT, FITTER_SEED, PROJECT, REPORT_SUFFIXES, SCHEMA, TREE, URL,
};

fn fail(message: impl Into<String>) -> Failure {
    Failure::other(message)
}

fn repo_file(root: &Path, relative: &str) -> PathBuf {
    root.join(relative)
}

fn file_entry(path: &Path, root: &Path) -> Result<Value, Failure> {
    let resolved = path.canonicalize()?;
    let root = root.canonicalize()?;
    let relative = resolved.strip_prefix(&root).map_err(|_| {
        fail(format!(
            "MiSTer framework artifact is outside repository: {}",
            path.display()
        ))
    })?;
    Ok(json!({
        "path": relative.to_string_lossy().replace('\\', "/"),
        "bytes": fs::metadata(&resolved)?.len(),
        "sha256": sha256(&resolved)?,
    }))
}

fn run_text(command: &[String], cwd: &Path) -> Result<String, Failure> {
    let (program, arguments) = command.split_first().ok_or_else(|| fail("empty command"))?;
    let output = Command::new(program)
        .args(arguments)
        .current_dir(cwd)
        .output()?;
    if !output.status.success() {
        return Err(fail(format!(
            "{} exited {}: {}{}",
            command.join(" "),
            output.status.code().unwrap_or(1),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn git(cwd: &Path, arguments: &[&str]) -> Result<String, Failure> {
    let mut command = vec!["git".to_owned()];
    command.extend(arguments.iter().map(|value| (*value).to_owned()));
    run_text(&command, cwd)
}

fn run_logged(command: &[String], cwd: &Path, log_path: &Path) -> Result<i32, Failure> {
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let (receiver, sender) = UnixStream::pair()?;
    let stdout: OwnedFd = sender.try_clone()?.into();
    let stderr: OwnedFd = sender.into();
    let (program, arguments) = command.split_first().ok_or_else(|| fail("empty command"))?;
    let mut child = Command::new(program)
        .args(arguments)
        .current_dir(cwd)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()?;
    let mut log = File::create(log_path)?;
    let mut reader = BufReader::new(receiver);
    let mut line = Vec::new();
    let mut terminal = io::stdout().lock();
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        terminal.write_all(&line)?;
        terminal.flush()?;
        log.write_all(&line)?;
    }
    log.sync_all()?;
    let status = child.wait()?;
    Ok(status.code().unwrap_or(1))
}

fn manifest_sources(root: &Path) -> Result<Vec<PathBuf>, Failure> {
    let root = root.canonicalize()?;
    let manifest = repo_file(&root, "fpga/arm7tdmi_mister.f");
    let parent = manifest
        .parent()
        .ok_or_else(|| fail("manifest has no parent"))?;
    let mut sources = Vec::new();
    for raw in fs::read_to_string(&manifest)?.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('+') {
            continue;
        }
        let path = parent
            .join(line)
            .canonicalize()
            .map_err(|error| fail(format!("manifest source is missing: {line}: {error}")))?;
        if !path.starts_with(&root) || !path.is_file() {
            return Err(fail(format!("manifest source escapes repository: {line}")));
        }
        sources.push(path);
    }
    if sources.iter().collect::<BTreeSet<_>>().len() != sources.len() {
        return Err(fail("FPGA source manifest contains duplicate files"));
    }
    Ok(sources)
}

pub(crate) fn local_inputs(root: &Path) -> Result<Vec<PathBuf>, Failure> {
    let mut files: BTreeSet<PathBuf> = manifest_sources(root)?.into_iter().collect();
    for relative in [
        "fpga/arm7tdmi_mister.f",
        "examples/generic_soc/arm7tdmi_generic_soc.sv",
        "examples/generic_soc/program.S",
        "examples/generic_soc/linker.ld",
        "verification/constrained_random_rs/src/generic_soc_rom.rs",
        "examples/mister_framework/arm7tdmi_mister_emu.sv",
        "examples/mister_framework/Template.sdc",
        "verification/constrained_random_rs/src/mister_framework.rs",
        "verification/constrained_random_rs/src/mister_framework_reports.rs",
    ] {
        files.insert(repo_file(root, relative));
    }
    Ok(files.into_iter().collect())
}

fn replace_once(path: &Path, old: &str, new: &str, label: &str) -> Result<(), Failure> {
    let contents = fs::read_to_string(path)?;
    if contents.matches(old).count() != 1 {
        return Err(fail(format!("official template {label} marker changed")));
    }
    fs::write(path, contents.replace(old, new))?;
    Ok(())
}

fn qip_line(relative: &Path) -> String {
    format!(
        "set_global_assignment -name SYSTEMVERILOG_FILE \"{}\"",
        relative.to_string_lossy().replace('\\', "/")
    )
}

fn install_overlay(
    framework_root: &Path,
    root: &Path,
) -> Result<BTreeMap<String, String>, Failure> {
    let mut ordered = manifest_sources(root)?;
    ordered.push(repo_file(
        root,
        "examples/generic_soc/arm7tdmi_generic_soc.sv",
    ));
    let overlay = framework_root.join("arm7tdmi");
    let mut copied = BTreeMap::new();
    for source in &ordered {
        let relative = source
            .strip_prefix(root)
            .map_err(|_| fail("overlay source escaped repository"))?;
        let destination = overlay.join(relative);
        fs::create_dir_all(
            destination
                .parent()
                .ok_or_else(|| fail("overlay path has no parent"))?,
        )?;
        fs::copy(source, &destination)?;
        copied.insert(
            relative.to_string_lossy().replace('\\', "/"),
            sha256(&destination)?,
        );
    }
    fs::copy(
        repo_file(root, "examples/mister_framework/arm7tdmi_mister_emu.sv"),
        framework_root.join("Template.sv"),
    )?;
    fs::copy(
        repo_file(root, "examples/mister_framework/Template.sdc"),
        framework_root.join("Template.sdc"),
    )?;
    replace_once(
        &framework_root.join("rtl/pll/pll_0002.v"),
        "output_clock_frequency0(\"20.000000 MHz\")",
        &format!("output_clock_frequency0(\"{CLOCK_MHZ:.6} MHz\")"),
        "PLL frequency",
    )?;
    replace_once(
        &framework_root.join("Template.qsf"),
        "set_global_assignment -name SEED 1",
        &format!("set_global_assignment -name SEED {FITTER_SEED}"),
        "fitter seed",
    )?;
    let mut lines = vec![
        "# Generated by arm7tdmi-sv/verification/constrained_random_rs/src/mister_framework.rs."
            .to_owned(),
        "# The official framework sys/ directory is intentionally unmodified.".to_owned(),
        "set_global_assignment -name TIMEQUEST_MULTICORNER_ANALYSIS ON".to_owned(),
        String::new(),
    ];
    for source in &ordered {
        let relative = source
            .strip_prefix(root)
            .map_err(|_| fail("overlay source escaped repository"))?;
        lines.push(qip_line(&PathBuf::from("arm7tdmi").join(relative)));
    }
    lines.push(qip_line(Path::new("Template.sv")));
    lines.push("set_global_assignment -name SDC_FILE \"Template.sdc\"".to_owned());
    lines.push(String::new());
    fs::write(framework_root.join("files.qip"), lines.join("\n"))?;
    Ok(copied)
}

fn git_metadata(root: &Path) -> Result<Value, Failure> {
    let status = git(root, &["status", "--porcelain=v1", "--untracked-files=all"])?;
    Ok(json!({
        "commit": git(root, &["rev-parse", "HEAD"])?,
        "dirty": !status.is_empty(),
        "status": status.lines().collect::<Vec<_>>(),
        "source_sha256": source_digest(root)?,
    }))
}

fn ensure_framework(cache: &Path) -> Result<(), Failure> {
    let parent = cache.parent().ok_or_else(|| fail("cache has no parent"))?;
    fs::create_dir_all(parent)?;
    if !cache.exists() {
        run_text(
            &[
                "git".into(),
                "clone".into(),
                "--no-checkout".into(),
                URL.into(),
                cache.to_string_lossy().into_owned(),
            ],
            parent,
        )?;
    }
    if !cache.join(".git").is_dir() {
        return Err(fail(format!(
            "framework cache is not a Git checkout: {}",
            cache.display()
        )));
    }
    let remote = git(cache, &["config", "--get", "remote.origin.url"])?;
    if remote.trim_end_matches('/') != URL.trim_end_matches('/') {
        return Err(fail(format!(
            "framework cache has unexpected origin: {remote}"
        )));
    }
    let available = Command::new("git")
        .args(["cat-file", "-e", &format!("{COMMIT}^{{commit}}")])
        .current_dir(cache)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?
        .success();
    if !available {
        git(cache, &["fetch", "origin", COMMIT])?;
    }
    let tree = git(cache, &["rev-parse", &format!("{COMMIT}^{{tree}}")])?;
    if tree != TREE {
        return Err(fail(format!("framework tree mismatch: {tree}")));
    }
    Ok(())
}

fn copy_artifacts(
    output: &Path,
    artifact_dir: &Path,
    build_log: &Path,
    root: &Path,
) -> Result<Map<String, Value>, Failure> {
    fs::create_dir_all(artifact_dir)?;
    let mut copied = Map::new();
    for suffix in REPORT_SUFFIXES {
        let name = format!("{PROJECT}{suffix}");
        let destination = artifact_dir.join(&name);
        fs::copy(output.join(&name), &destination)?;
        copied.insert(name, file_entry(&destination, root)?);
    }
    copied.insert("build.log".to_owned(), file_entry(build_log, root)?);
    Ok(copied)
}

struct Args {
    cache: PathBuf,
    work_dir: PathBuf,
    report: PathBuf,
    artifacts: PathBuf,
    quartus_sh: String,
    parse_reports: Option<PathBuf>,
    validate_report: Option<PathBuf>,
    expected_commit: Option<String>,
}

impl Args {
    fn parse(arguments: &[String], root: &Path) -> Result<Self, Failure> {
        let mut args = Self {
            cache: root.join(".tools/mister-framework/upstream"),
            work_dir: root.join(".tools/mister-framework/work"),
            report: root.join("reports/generated/mister-framework-report.json"),
            artifacts: root.join("reports/generated/mister-framework"),
            quartus_sh: "quartus_sh".to_owned(),
            parse_reports: None,
            validate_report: None,
            expected_commit: None,
        };
        let mut input = arguments.iter();
        while let Some(name) = input.next() {
            let value = input
                .next()
                .ok_or_else(|| fail(format!("missing value for {name}")))?;
            match name.as_str() {
                "--cache" => args.cache = PathBuf::from(value),
                "--work-dir" => args.work_dir = PathBuf::from(value),
                "--output" => args.report = PathBuf::from(value),
                "--artifact-dir" => args.artifacts = PathBuf::from(value),
                "--quartus-sh" => args.quartus_sh = value.clone(),
                "--parse-reports" => args.parse_reports = Some(PathBuf::from(value)),
                "--validate-report" => args.validate_report = Some(PathBuf::from(value)),
                "--expected-commit" => args.expected_commit = Some(value.clone()),
                _ => return Err(fail(format!("unknown argument: {name}"))),
            }
        }
        for path in [
            &mut args.cache,
            &mut args.work_dir,
            &mut args.report,
            &mut args.artifacts,
        ] {
            if path.is_relative() {
                *path = root.join(&*path);
            }
        }
        if let Some(path) = &mut args.parse_reports {
            if path.is_relative() {
                *path = root.join(&*path);
            }
        }
        if let Some(path) = &mut args.validate_report {
            if path.is_relative() {
                *path = root.join(&*path);
            }
        }
        Ok(args)
    }
}

fn checked_remove_directory(path: &Path, root: &Path) -> Result<(), Failure> {
    let parent = path
        .parent()
        .ok_or_else(|| fail("directory has no parent"))?;
    fs::create_dir_all(parent)?;
    let parent = parent.canonicalize()?;
    let path = parent.join(
        path.file_name()
            .ok_or_else(|| fail("directory has no name"))?,
    );
    let root = root.canonicalize()?;
    if path == root
        || root.starts_with(&path)
        || fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err(fail(format!(
            "refusing to remove unsafe directory: {}",
            path.display()
        )));
    }
    if path.exists() {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

fn build(args: &Args, root: &Path) -> Result<Value, Failure> {
    let metadata = git_metadata(root)?;
    if metadata["dirty"] == true {
        return Err(fail(
            "MiSTer framework evidence requires a clean source tree",
        ));
    }
    ensure_framework(&args.cache)?;
    if args.work_dir == args.artifacts
        || args.work_dir.starts_with(&args.artifacts)
        || args.artifacts.starts_with(&args.work_dir)
    {
        return Err(fail(
            "MiSTer framework work and artifact directories overlap",
        ));
    }
    checked_remove_directory(&args.artifacts, root)?;
    fs::create_dir_all(&args.artifacts)?;
    let build_log = args.artifacts.join("build.log");
    let command = vec![
        args.quartus_sh.clone(),
        "--flow".into(),
        "compile".into(),
        PROJECT.into(),
    ];
    checked_remove_directory(&args.work_dir, root)?;
    run_text(
        &[
            "git".into(),
            "clone".into(),
            "--no-hardlinks".into(),
            args.cache.to_string_lossy().into_owned(),
            args.work_dir.to_string_lossy().into_owned(),
        ],
        args.work_dir
            .parent()
            .ok_or_else(|| fail("work directory has no parent"))?,
    )?;
    git(&args.work_dir, &["checkout", "--detach", COMMIT])?;
    git(&args.work_dir, &["remote", "set-url", "origin", URL])?;
    if !git(&args.work_dir, &["status", "--porcelain=v1"])?.is_empty() {
        return Err(fail("fresh framework checkout is not clean"));
    }
    if git(&args.work_dir, &["rev-parse", "HEAD^{tree}"])? != TREE {
        return Err(fail("fresh framework checkout tree mismatch"));
    }
    install_overlay(&args.work_dir, root)?;
    let returncode = run_logged(&command, &args.work_dir, &build_log)?;
    if returncode != 0 {
        return Err(fail(format!(
            "MiSTer framework build failed with exit {returncode}"
        )));
    }
    let output = args.work_dir.join("output_files");
    let (result, errors) = reports::parse_reports(&output);
    let artifacts = copy_artifacts(&output, &args.artifacts, &build_log, root)?;
    if !errors.is_empty() {
        return Err(fail(errors.join("; ")));
    }
    let tool_version = run_text(&[args.quartus_sh.clone(), "--version".into()], root)?;
    let mut inputs = Map::new();
    for path in local_inputs(root)? {
        let entry = file_entry(&path, root)?;
        inputs.insert(
            entry["path"].as_str().unwrap().to_owned(),
            json!({
                "bytes": entry["bytes"], "sha256": entry["sha256"],
            }),
        );
    }
    let report = json!({
        "schema": SCHEMA,
        "status": "passed",
        "created_utc": utc_now(),
        "git": metadata,
        "framework": {"url": URL, "commit": COMMIT, "tree": TREE},
        "command": command,
        "tools": {"quartus_sh": tool_version},
        "inputs": inputs,
        "result": result,
        "artifacts": artifacts,
    });
    write_json(&args.report, &report)?;
    reports::validate_evidence(
        &report,
        root,
        metadata["commit"].as_str().unwrap_or(""),
        true,
    )?;
    Ok(report)
}

pub fn command(arguments: &[String]) -> Result<(), String> {
    let root = repo_root();
    let args = Args::parse(arguments, &root).map_err(|error| error.to_string())?;
    if let Some(output) = &args.parse_reports {
        let (result, errors) = reports::parse_reports(output);
        println!("{}", json!({"result": result, "errors": errors}));
        return Ok(());
    }
    if let Some(path) = &args.validate_report {
        let expected_commit = args
            .expected_commit
            .as_deref()
            .ok_or_else(|| "--validate-report requires --expected-commit".to_owned())?;
        let report: Value =
            serde_json::from_slice(&fs::read(path).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
        let candidates = reports::validate_evidence(&report, &root, expected_commit, true)
            .map_err(|error| error.to_string())?;
        println!("{}", json!(candidates));
        return Ok(());
    }
    let report = build(&args, &root).map_err(|error| error.to_string())?;
    let result = &report["result"];
    println!(
        "[mister-framework] PASS {} at {:.3} MHz; {} ALMs; setup {:+.3} ns; hold {:+.3} ns",
        result["device"].as_str().expect("validated device"),
        result["core_clock_mhz"].as_f64().expect("validated clock"),
        result["resources"]["alm"]
            .as_u64()
            .expect("validated ALM count"),
        result["timing"]["minimum_setup_slack_ns"]
            .as_f64()
            .expect("validated setup slack"),
        result["timing"]["minimum_hold_slack_ns"]
            .as_f64()
            .expect("validated hold slack"),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_is_pinned_and_contains_all_local_sources() {
        let root = repo_root();
        let temporary = tempfile::tempdir().unwrap();
        let framework = temporary.path();
        fs::create_dir_all(framework.join("rtl/pll")).unwrap();
        fs::write(
            framework.join("rtl/pll/pll_0002.v"),
            "output_clock_frequency0(\"20.000000 MHz\")\n",
        )
        .unwrap();
        fs::write(
            framework.join("Template.qsf"),
            "set_global_assignment -name SEED 1\n",
        )
        .unwrap();
        install_overlay(framework, &root).unwrap();
        let qip = fs::read_to_string(framework.join("files.qip")).unwrap();
        for source in manifest_sources(&root).unwrap() {
            let relative = source.strip_prefix(&root).unwrap().to_string_lossy();
            assert!(qip.contains(relative.as_ref()));
        }
        assert!(qip.contains("examples/generic_soc/arm7tdmi_generic_soc.sv"));
        assert!(qip.contains("TIMEQUEST_MULTICORNER_ANALYSIS ON"));
        assert!(!qip.contains("$::quartus(qip_path)"));
        let emu = fs::read_to_string(framework.join("Template.sv")).unwrap();
        for marker in [
            "module emu",
            "arm7tdmi_generic_soc u_soc",
            "TEST_STATUS",
            "TEST_SIGNATURE",
            "32'h5041_5353",
            "32'h4641_494c",
            "barcode_bit",
        ] {
            assert!(
                emu.contains(marker),
                "missing MiSTer overlay marker: {marker}"
            );
        }
        let sdc = fs::read_to_string(framework.join("Template.sdc")).unwrap();
        for marker in [
            "derive_pll_clocks",
            "derive_clock_uncertainty",
            "HDMI_TX_CAPTURE_SCALED",
            "HDMI_TX_CAPTURE_DIRECT",
            "HDMI_SCLK_OUT",
            "HDMI_SCLK_PIN",
            "{HDMI_I2C_SDA HDMI_TX_INT IO_SDA SDCD_SPDIF}",
            "SDIO_CLK SDIO_CMD SDIO_DAT[*] SD_SPI_CS",
        ] {
            assert!(sdc.contains(marker), "missing MiSTer SDC marker: {marker}");
        }
        assert!(!sdc.contains("get_ports *]"));
        assert!(!sdc.contains("get_ports {*}"));
        assert!(fs::read_to_string(framework.join("rtl/pll/pll_0002.v"))
            .unwrap()
            .contains("12.500000 MHz"));
        assert!(fs::read_to_string(framework.join("Template.qsf"))
            .unwrap()
            .contains("SEED 4"));
        assert!(local_inputs(&root)
            .unwrap()
            .contains(&root.join("examples/generic_soc/program.S")));
    }
}
