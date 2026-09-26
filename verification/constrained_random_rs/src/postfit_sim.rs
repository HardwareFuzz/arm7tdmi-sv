//! Fit two isolated Cyclone V wrapper projects and run their functional netlists.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;
use std::time::Duration;

use chrono::{SecondsFormat, Utc};
use regex::Regex;
use serde_json::{json, Value};
use wait_timeout::ChildExt;

use crate::campaign::{repo_root, sha256, write_json, Failure};

const SCHEMA: &str = "arm7tdmis-postfit-v1";
const DEVICE: &str = "5CSEBA6U23I7";
const EXPECTED_QUARTUS: &str = "Version 17.0.2 Build 602 07/19/2017 SJ Lite Edition";
const ALLOWED_CRITICAL: [&str; 2] = ["169085", "332012"];
const ALLOWED_PRIMITIVES: [&str; 5] = [
    "dffeas",
    "cyclonev_lcell_comb",
    "cyclonev_io_ibuf",
    "cyclonev_io_obuf",
    "cyclonev_clkena",
];
const FORBIDDEN_PRIMITIVES: [&str; 3] = ["cyclonev_mac", "_encrypted", "altera_mf"];

fn fail(message: impl Into<String>) -> Failure {
    Failure::other(message)
}

fn file_entry(path: &Path, root: &Path) -> Result<Value, Failure> {
    let resolved = path.canonicalize()?;
    let relative = resolved.strip_prefix(root.canonicalize()?).map_err(|_| {
        fail(format!(
            "post-fit artifact is outside repository: {}",
            path.display()
        ))
    })?;
    Ok(json!({
        "path": relative.to_string_lossy().replace('\\', "/"),
        "bytes": fs::metadata(&resolved)?.len(),
        "sha256": sha256(&resolved)?,
    }))
}

fn write_text_atomic(path: &Path, bytes: &[u8]) -> Result<(), Failure> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!(
        "{}tmp",
        path.extension()
            .and_then(|value| value.to_str())
            .map_or(String::new(), |value| format!("{value}."))
    ));
    fs::write(&temporary, bytes)?;
    fs::rename(temporary, path)?;
    Ok(())
}

fn capture(
    arguments: &[String],
    cwd: &Path,
    timeout: Duration,
    log_path: Option<&Path>,
) -> Result<String, Failure> {
    let (program, args) = arguments
        .split_first()
        .ok_or_else(|| fail("post-fit command is empty"))?;
    let mut scratch = tempfile::tempfile()?;
    let stdout = scratch.try_clone()?;
    let stderr = scratch.try_clone()?;
    let mut process = Command::new(program);
    let mut child = process
        .args(args)
        .current_dir(cwd)
        .process_group(0)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|error| fail(format!("cannot start {}: {error}", arguments.join(" "))))?;
    let status = child.wait_timeout(timeout)?;
    let timed_out = status.is_none();
    if timed_out {
        if let Ok(pid) = i32::try_from(child.id()) {
            // The tools spawn workers; stop the whole isolated command group.
            unsafe { libc::kill(-pid, libc::SIGKILL) };
        }
        let _ = child.kill();
        child.wait()?;
    }
    scratch.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    scratch.read_to_end(&mut bytes)?;
    if let Some(path) = log_path {
        write_text_atomic(path, &bytes)?;
    }
    let output = String::from_utf8_lossy(&bytes).trim().to_owned();
    if timed_out {
        return Err(fail(format!(
            "post-fit command timed out after {}s: {}",
            timeout.as_secs(),
            arguments.join(" ")
        )));
    }
    let status = status.unwrap();
    if !status.success() {
        return Err(fail(format!(
            "post-fit command exited {}: {}\n{output}",
            status.code().unwrap_or(1),
            arguments.join(" ")
        )));
    }
    Ok(output)
}

fn find_command(command: &str) -> Result<PathBuf, Failure> {
    let path = Path::new(command);
    let candidates = if path.components().count() > 1 {
        vec![path.to_path_buf()]
    } else {
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|directory| directory.join(command))
            .collect()
    };
    for candidate in candidates {
        if candidate.is_file() {
            return Ok(candidate.canonicalize()?);
        }
    }
    Err(fail(format!(
        "required command is not installed: {command}"
    )))
}

fn command_identity(command: &str, root: &Path) -> Result<Value, Failure> {
    let path = find_command(command)?;
    let version = capture(
        &[path.to_string_lossy().into_owned(), "--version".to_owned()],
        root,
        Duration::from_secs(30),
        None,
    )?;
    Ok(json!({
        "command": command,
        "path": path.to_string_lossy(),
        "bytes": fs::metadata(&path)?.len(),
        "sha256": sha256(&path)?,
        "version": version,
    }))
}

fn source_paths(root: &Path) -> Result<BTreeSet<PathBuf>, Failure> {
    let filelist = root.join("fpga/arm7tdmi_mister.f");
    let mut paths = BTreeSet::from([
        root.join("verification/constrained_random_rs/src/postfit_sim.rs"),
        root.join("verification/constrained_random_rs/src/main.rs"),
        root.join("verification/constrained_random_rs/src/campaign.rs"),
        root.join("verification/constrained_random_rs/Cargo.toml"),
        root.join("verification/constrained_random_rs/Cargo.lock"),
        filelist.clone(),
        root.join("fpga/arm7tdmi_mister.qip"),
        root.join("verification/intel_cyclonev_postfit_primitives.sv"),
        root.join("tb/postfit/arm7tdmi_postfit_tb.sv"),
    ]);
    for raw_line in fs::read_to_string(&filelist)?.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('+') {
            continue;
        }
        paths.insert(filelist.parent().unwrap().join(line));
    }
    let mut canonical = BTreeSet::new();
    for path in paths {
        if !path.is_file() {
            return Err(fail(format!(
                "post-fit input is missing: {}",
                path.display()
            )));
        }
        canonical.insert(path.canonicalize()?);
    }
    Ok(canonical)
}

fn qsf_text(root: &Path, big_endian: bool) -> Result<String, Failure> {
    let qip = root.join("fpga/arm7tdmi_mister.qip").canonicalize()?;
    let mut qip_path = qip.to_string_lossy().into_owned();
    for (original, escaped) in [
        ("\\", "\\\\"),
        ("\"", "\\\""),
        ("$", "\\$"),
        ("[", "\\["),
        ("]", "\\]"),
    ] {
        qip_path = qip_path.replace(original, escaped);
    }
    Ok(format!(
        "set_global_assignment -name FAMILY \"Cyclone V\"\n\
set_global_assignment -name DEVICE {DEVICE}\n\
set_global_assignment -name TOP_LEVEL_ENTITY arm7tdmi_mister\n\
set_global_assignment -name QIP_FILE \"{qip_path}\"\n\
set_global_assignment -name PROJECT_OUTPUT_DIRECTORY output\n\
set_global_assignment -name NUM_PARALLEL_PROCESSORS ALL\n\
set_global_assignment -name AUTO_DSP_RECOGNITION OFF\n\
set_parameter -name BIG_ENDIAN {}\n\
set_parameter -name ENABLE_DEBUG 0\n\
set_parameter -name ENABLE_COPROCESSOR 0\n",
        u8::from(big_endian)
    ))
}

fn validate_quartus_log(text: &str, phase: &str) -> Result<BTreeSet<String>, Failure> {
    static ERROR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^Error \(").unwrap());
    static CRITICAL: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"Critical Warning \((\d+)\)").unwrap());
    if ERROR.is_match(text) {
        return Err(fail(format!("{phase} log contains a Quartus error")));
    }
    let critical: BTreeSet<_> = CRITICAL
        .captures_iter(text)
        .map(|capture| capture[1].to_owned())
        .collect();
    let allowed: BTreeSet<_> = ALLOWED_CRITICAL.into_iter().map(str::to_owned).collect();
    let unexpected: Vec<_> = critical.difference(&allowed).cloned().collect();
    if !unexpected.is_empty() {
        return Err(fail(format!(
            "{phase} has unexpected critical warning IDs: {unexpected:?}"
        )));
    }
    Ok(critical)
}

fn validate_netlist(path: &Path) -> Result<Vec<String>, Failure> {
    static PRIMITIVE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?m)^\s*(cyclonev_[A-Za-z0-9_]+|dffeas)\s+\\").unwrap());
    let text = fs::read_to_string(path)?;
    for forbidden in FORBIDDEN_PRIMITIVES {
        if text.contains(forbidden) {
            return Err(fail(format!(
                "fitted netlist requires forbidden primitive {forbidden}"
            )));
        }
    }
    let primitives: BTreeSet<_> = PRIMITIVE
        .captures_iter(&text)
        .map(|capture| capture[1].to_owned())
        .collect();
    let allowed: BTreeSet<_> = ALLOWED_PRIMITIVES.into_iter().map(str::to_owned).collect();
    let unknown: Vec<_> = primitives.difference(&allowed).cloned().collect();
    if !unknown.is_empty() {
        return Err(fail(format!(
            "fitted netlist has unmodeled primitives: {unknown:?}"
        )));
    }
    if primitives != allowed {
        return Err(fail(format!(
            "fitted netlist primitive inventory changed: {primitives:?}"
        )));
    }
    Ok(primitives.into_iter().collect())
}

struct Args {
    output: PathBuf,
    quartus_map: String,
    quartus_fit: String,
    quartus_eda: String,
    verilator: String,
    timeout: Duration,
}

impl Args {
    fn parse(arguments: &[String], root: &Path) -> Result<Self, Failure> {
        let mut args = Self {
            output: root.join("reports/generated/postfit-report.json"),
            quartus_map: "quartus_map".to_owned(),
            quartus_fit: "quartus_fit".to_owned(),
            quartus_eda: "quartus_eda".to_owned(),
            verilator: "verilator".to_owned(),
            timeout: Duration::from_secs(300),
        };
        let mut position = 0;
        while position < arguments.len() {
            let name = &arguments[position];
            let value = arguments
                .get(position + 1)
                .ok_or_else(|| fail(format!("missing value for {name}")))?;
            match name.as_str() {
                "--output" => args.output = PathBuf::from(value),
                "--quartus-map" => args.quartus_map = value.clone(),
                "--quartus-fit" => args.quartus_fit = value.clone(),
                "--quartus-eda" => args.quartus_eda = value.clone(),
                "--verilator" => args.verilator = value.clone(),
                "--timeout-seconds" => {
                    let seconds = value
                        .parse::<u64>()
                        .map_err(|_| fail("timeout must be positive"))?;
                    if seconds == 0 {
                        return Err(fail("timeout must be positive"));
                    }
                    args.timeout = Duration::from_secs(seconds);
                }
                _ => return Err(fail(format!("unknown argument: {name}"))),
            }
            position += 2;
        }
        if !args.output.is_absolute() {
            args.output = std::env::current_dir()?.join(args.output);
        }
        let mut normalized = PathBuf::new();
        for component in args.output.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    normalized.pop();
                }
                other => normalized.push(other.as_os_str()),
            }
        }
        args.output = normalized;
        if !args.output.starts_with(root) {
            return Err(fail("post-fit output must be inside the repository"));
        }
        Ok(args)
    }
}

fn profile_result(
    name: &str,
    big_endian: bool,
    work_root: &Path,
    report_root: &Path,
    root: &Path,
    args: &Args,
) -> Result<Value, Failure> {
    let profile_root = work_root.join(name);
    fs::create_dir_all(&profile_root)?;
    let project = format!("arm7tdmi_postfit_{name}");
    fs::write(
        profile_root.join(format!("{project}.qsf")),
        qsf_text(root, big_endian)?,
    )?;
    let netlist_root = profile_root.join("netlist");
    let obj_root = profile_root.join("obj");
    let log_root = report_root.join(name);
    let mut commands = BTreeMap::<String, Vec<String>>::from([
        (
            "map".to_owned(),
            vec![
                args.quartus_map.clone(),
                project.clone(),
                "--read_settings_files=on".to_owned(),
                "--write_settings_files=off".to_owned(),
            ],
        ),
        (
            "fit".to_owned(),
            vec![
                args.quartus_fit.clone(),
                project.clone(),
                "--read_settings_files=on".to_owned(),
                "--write_settings_files=off".to_owned(),
            ],
        ),
        (
            "eda".to_owned(),
            vec![
                args.quartus_eda.clone(),
                project.clone(),
                "--simulation".to_owned(),
                "--tool=modelsim".to_owned(),
                "--format=verilog".to_owned(),
                "--functional".to_owned(),
                format!("--output_directory={}", netlist_root.display()),
                "--write_settings_files=off".to_owned(),
            ],
        ),
    ]);
    let mut critical_warning_ids = BTreeSet::new();
    let mut logs = BTreeMap::<String, Value>::new();
    for phase in ["map", "fit", "eda"] {
        let log_path = log_root.join(format!("{phase}.log"));
        let output = capture(
            &commands[phase],
            &profile_root,
            args.timeout,
            Some(&log_path),
        )?;
        critical_warning_ids.extend(validate_quartus_log(&output, &format!("{name}/{phase}"))?);
        logs.insert(phase.to_owned(), file_entry(&log_path, root)?);
    }
    let netlist_path = netlist_root.join(format!("{project}.vo"));
    if !netlist_path.is_file() || fs::metadata(&netlist_path)?.len() == 0 {
        return Err(fail(format!(
            "{name} EDA writer produced no functional netlist"
        )));
    }
    let primitives = validate_netlist(&netlist_path)?;
    commands.insert(
        "verilator".to_owned(),
        vec![
            args.verilator.clone(),
            "--binary".to_owned(),
            "--timing".to_owned(),
            "-j".to_owned(),
            "0".to_owned(),
            "-Wno-fatal".to_owned(),
            "-Wno-SPECIFYIGN".to_owned(),
            "-Wno-TIMESCALEMOD".to_owned(),
            "-Wno-WIDTHEXPAND".to_owned(),
            "-Wno-WIDTHTRUNC".to_owned(),
            "--top-module".to_owned(),
            "arm7tdmi_postfit_tb".to_owned(),
            "--Mdir".to_owned(),
            obj_root.to_string_lossy().into_owned(),
            format!("-DPOSTFIT_BIG_ENDIAN={}", u8::from(big_endian)),
            root.join("verification/intel_cyclonev_postfit_primitives.sv")
                .to_string_lossy()
                .into_owned(),
            netlist_path.to_string_lossy().into_owned(),
            root.join("tb/postfit/arm7tdmi_postfit_tb.sv")
                .to_string_lossy()
                .into_owned(),
        ],
    );
    let verilator_log = log_root.join("verilator.log");
    capture(
        &commands["verilator"],
        root,
        args.timeout,
        Some(&verilator_log),
    )?;
    logs.insert("verilator".to_owned(), file_entry(&verilator_log, root)?);
    let binary_path = obj_root.join("Varm7tdmi_postfit_tb");
    if !binary_path.is_file() {
        return Err(fail(format!(
            "{name} Verilator build produced no executable"
        )));
    }
    commands.insert(
        "simulation".to_owned(),
        vec![binary_path.to_string_lossy().into_owned()],
    );
    let simulation_log = log_root.join("simulation.log");
    let simulation_text = capture(
        &commands["simulation"],
        root,
        Duration::from_secs(30),
        Some(&simulation_log),
    )?;
    static PASS: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"\[postfit\] PASS endian=(\d+) accepted=(\d+) ce_low=(\d+) longest_wait=(\d+)")
            .unwrap()
    });
    let metrics = PASS
        .captures(&simulation_text)
        .filter(|captures| captures[1] == u8::from(big_endian).to_string())
        .ok_or_else(|| {
            fail(format!(
                "{name} simulation has no exact [postfit] PASS marker"
            ))
        })?;
    let integer = |index: usize| -> Result<u64, Failure> {
        metrics[index]
            .parse::<u64>()
            .map_err(|error| fail(error.to_string()))
    };
    logs.insert("simulation".to_owned(), file_entry(&simulation_log, root)?);
    Ok(json!({
        "status": "passed",
        "big_endian": big_endian,
        "project": project,
        "device": DEVICE,
        "synthesis_policy": {
            "auto_dsp_recognition": "off",
            "reason": "portable structural simulation; production DSP inference is independently checked by FPGA-005",
        },
        "netlist": {
            "bytes": fs::metadata(&netlist_path)?.len(),
            "sha256": sha256(&netlist_path)?,
            "primitives": primitives,
        },
        "metrics": {
            "accepted_transactions": integer(2)?,
            "accepted_while_cpu_ce_low": integer(3)?,
            "longest_wait_cycles": integer(4)?,
        },
        "critical_warning_ids": critical_warning_ids,
        "commands": commands,
        "logs": logs,
    }))
}

pub fn command(arguments: &[String]) -> Result<(), String> {
    run(arguments).map_err(|error| error.to_string())
}

fn run(arguments: &[String]) -> Result<(), Failure> {
    let root = repo_root();
    let args = Args::parse(arguments, &root)?;
    let output_parent = args
        .output
        .parent()
        .ok_or_else(|| fail("post-fit output has no parent directory"))?;
    fs::create_dir_all(output_parent)?;
    if !output_parent
        .canonicalize()?
        .starts_with(root.canonicalize()?)
    {
        return Err(fail("post-fit output must be inside the repository"));
    }
    let report_root = args.output.parent().unwrap().join("postfit");
    let tools = BTreeMap::from([
        ("quartus_map", command_identity(&args.quartus_map, &root)?),
        ("quartus_fit", command_identity(&args.quartus_fit, &root)?),
        ("quartus_eda", command_identity(&args.quartus_eda, &root)?),
        ("verilator", command_identity(&args.verilator, &root)?),
    ]);
    for label in ["quartus_map", "quartus_fit", "quartus_eda"] {
        let version = tools[label]["version"].as_str().unwrap_or_default();
        if !version.contains(EXPECTED_QUARTUS) {
            return Err(fail(format!(
                "post-fit validation requires checked Quartus {EXPECTED_QUARTUS} for {label}"
            )));
        }
    }
    let git_status = capture(
        &[
            "git".to_owned(),
            "status".to_owned(),
            "--porcelain=v1".to_owned(),
            "--untracked-files=all".to_owned(),
        ],
        &root,
        Duration::from_secs(30),
        None,
    )?;
    let git_commit = capture(
        &["git".to_owned(), "rev-parse".to_owned(), "HEAD".to_owned()],
        &root,
        Duration::from_secs(30),
        None,
    )?;
    let mut inputs = BTreeMap::new();
    for path in source_paths(&root)? {
        let entry = file_entry(&path, &root)?;
        let key = entry["path"].as_str().unwrap().to_owned();
        inputs.insert(key, entry);
    }
    let mut report = json!({
        "schema": SCHEMA,
        "status": "running",
        "created_utc": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, false),
        "git": {
            "commit": git_commit,
            "dirty": !git_status.is_empty(),
            "status": git_status.lines().collect::<Vec<_>>(),
        },
        "device": DEVICE,
        "profile_exceptions": {
            "critical_warning_ids": ALLOWED_CRITICAL,
            "explanation": "The temporary simulation-only projects intentionally have no board pinout or SDC; production fit/timing closure is separately mandatory under FPGA-003.",
        },
        "tools": tools,
        "inputs": inputs,
        "profiles": {},
    });
    let result = (|| -> Result<(), Failure> {
        let temporary = tempfile::Builder::new()
            .prefix("arm7tdmi-postfit-")
            .tempdir()?;
        for (name, big_endian) in [("little", false), ("big", true)] {
            report["profiles"][name] = profile_result(
                name,
                big_endian,
                temporary.path(),
                &report_root,
                &root,
                &args,
            )?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        report["status"] = json!("failed");
        write_json(&args.output, &report)?;
        return Err(error);
    }
    report["status"] = json!("passed");
    write_json(&args.output, &report)?;
    println!(
        "[postfit] PASS (little={}, big={})",
        report["profiles"]["little"]["netlist"]["sha256"]
            .as_str()
            .and_then(|value| value.get(..12))
            .unwrap_or_default(),
        report["profiles"]["big"]["netlist"]["sha256"]
            .as_str()
            .and_then(|value| value.get(..12))
            .unwrap_or_default(),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn executable(path: &Path, script: &str) {
        fs::write(path, script).unwrap();
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }

    #[test]
    fn qsf_and_log_policy_are_revision_locked() {
        let qsf = qsf_text(&repo_root(), true).unwrap();
        assert!(qsf.contains("AUTO_DSP_RECOGNITION OFF"));
        assert!(qsf.contains("BIG_ENDIAN 1"));
        assert!(qsf.contains("5CSEBA6U23I7"));
        assert_eq!(
            validate_quartus_log(
                "Critical Warning (169085)\nCritical Warning (332012)",
                "test",
            )
            .unwrap()
            .len(),
            2
        );
        assert!(validate_quartus_log("Critical Warning (999999)", "test").is_err());
        assert!(validate_quartus_log("Error (1234): failed", "test").is_err());
    }

    #[test]
    fn mocked_toolchain_exercises_both_profiles_and_hashed_logs() {
        let root = repo_root();
        fs::create_dir_all(root.join(".tools")).unwrap();
        let temporary = tempfile::Builder::new()
            .prefix("postfit-contract-")
            .tempdir_in(root.join(".tools"))
            .unwrap();
        let quartus = temporary.path().join("quartus-fake");
        executable(
            &quartus,
            r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  echo 'Version 17.0.2 Build 602 07/19/2017 SJ Lite Edition'
  exit 0
fi
echo 'Critical Warning (169085)'
echo 'Critical Warning (332012)'
"#,
        );
        let eda = temporary.path().join("quartus-eda-fake");
        executable(
            &eda,
            r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  echo 'Version 17.0.2 Build 602 07/19/2017 SJ Lite Edition'
  exit 0
fi
project=$1
for arg in "$@"; do
  case "$arg" in
    --output_directory=*) output=$(printf '%s' "$arg" | sed 's/^--output_directory=//') ;;
  esac
done
mkdir -p "$output"
cat > "$output/$project.vo" <<'NET'
dffeas \u0 (...);
cyclonev_lcell_comb \u1 (...);
cyclonev_io_ibuf \u2 (...);
cyclonev_io_obuf \u3 (...);
cyclonev_clkena \u4 (...);
NET
echo 'Critical Warning (169085)'
echo 'Critical Warning (332012)'
"#,
        );
        let verilator = temporary.path().join("verilator-fake");
        executable(
            &verilator,
            r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  echo 'Verilator 5.0'
  exit 0
fi
endian=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --Mdir) shift; output=$1 ;;
    -DPOSTFIT_BIG_ENDIAN=1) endian=1 ;;
  esac
  shift
done
mkdir -p "$output"
printf '#!/bin/sh\necho "[postfit] PASS endian=%s accepted=40 ce_low=2 longest_wait=3"\n' "$endian" > "$output/Varm7tdmi_postfit_tb"
chmod +x "$output/Varm7tdmi_postfit_tb"
"#,
        );
        let report_path = temporary.path().join("postfit-report.json");
        run(&[
            "--output".to_owned(),
            report_path.to_string_lossy().into_owned(),
            "--quartus-map".to_owned(),
            quartus.to_string_lossy().into_owned(),
            "--quartus-fit".to_owned(),
            quartus.to_string_lossy().into_owned(),
            "--quartus-eda".to_owned(),
            eda.to_string_lossy().into_owned(),
            "--verilator".to_owned(),
            verilator.to_string_lossy().into_owned(),
            "--timeout-seconds".to_owned(),
            "10".to_owned(),
        ])
        .unwrap();
        let report: Value = serde_json::from_slice(&fs::read(report_path).unwrap()).unwrap();
        assert_eq!(report["schema"], SCHEMA);
        assert_eq!(report["status"], "passed");
        assert_eq!(report["profiles"]["little"]["big_endian"], false);
        assert_eq!(report["profiles"]["big"]["big_endian"], true);
        assert_eq!(
            report["profiles"]["little"]["metrics"]["accepted_transactions"],
            40
        );
        assert_eq!(
            report["profiles"]["big"]["critical_warning_ids"],
            json!(["169085", "332012"])
        );
        for name in ["little", "big"] {
            for phase in ["map", "fit", "eda", "verilator", "simulation"] {
                let entry = &report["profiles"][name]["logs"][phase];
                let log = root.join(entry["path"].as_str().unwrap());
                assert_eq!(entry["sha256"], sha256(&log).unwrap());
            }
        }
    }

    #[test]
    fn netlist_and_release_boundary_reject_unmodeled_primitives() {
        let root = repo_root();
        let models =
            fs::read_to_string(root.join("verification/intel_cyclonev_postfit_primitives.sv"))
                .unwrap();
        for primitive in ALLOWED_PRIMITIVES {
            assert!(models.contains(&format!("module {primitive}")));
        }
        let testbench = fs::read_to_string(root.join("tb/postfit/arm7tdmi_postfit_tb.sv")).unwrap();
        for required in [
            "POSTFIT_BIG_ENDIAN",
            "CPU_CE",
            "MEM_READY",
            "MEM_BYTE_ENABLE",
            "saw_irq_vector",
            "saw_reset_vector",
            "accepted_while_ce_low",
            "longest_wait",
            "[postfit] PASS",
            "$fatal",
        ] {
            assert!(
                testbench.contains(required),
                "missing post-fit scoreboard check: {required}"
            );
        }
        let release = fs::read_to_string(
            root.join("verification/constrained_random_rs/src/release_evidence/files/fpga.rs"),
        )
        .unwrap();
        assert!(release.contains("arm7tdmis-postfit-v1"));
        assert!(release.contains("post-fit report lacks both endian profiles"));
        let makefile = fs::read_to_string(root.join("scripts/Makefile")).unwrap();
        assert!(makefile.contains("postfit-sim: $(RANDOM_RUST_BIN)"));

        let temp = tempfile::tempdir().unwrap();
        let netlist = temp.path().join("netlist.vo");
        fs::write(
            &netlist,
            "dffeas \\u0 ();\ncyclonev_lcell_comb \\u1 ();\ncyclonev_io_ibuf \\u2 ();\ncyclonev_io_obuf \\u3 ();\ncyclonev_clkena \\u4 ();\n",
        ).unwrap();
        assert_eq!(validate_netlist(&netlist).unwrap().len(), 5);
        fs::write(&netlist, "cyclonev_mac \\u0 ();\n").unwrap();
        assert!(validate_netlist(&netlist)
            .unwrap_err()
            .to_string()
            .contains("forbidden primitive"));
    }
}
