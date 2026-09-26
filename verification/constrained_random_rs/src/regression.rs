//! Fail-hard, source-identified regression orchestration.

use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Write};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use serde::Serialize;
use serde_json::{json, Value};

use crate::campaign::{repo_root, sha256, utc_now, write_json, Failure};
use crate::coverage_report::source_digest;

const SCHEMA: &str = "arm7tdmis-regression-v1";
const QUICK_INTEGRATIONS: [&str; 2] = ["qemu_diff", "compiler"];

fn fail(message: impl Into<String>) -> Failure {
    Failure::other(message)
}

#[derive(Clone)]
struct Args {
    units: Vec<String>,
    integrations: Vec<String>,
    variant: String,
    seed: i64,
    report: PathBuf,
    quick: bool,
    simulation_only: bool,
    plan_json: bool,
}

impl Args {
    fn parse(arguments: &[String], root: &Path) -> Result<Self, Failure> {
        let mut args = Self {
            units: Vec::new(),
            integrations: Vec::new(),
            variant: env::var("REGRESSION_VARIANT").unwrap_or_else(|_| "default".to_owned()),
            seed: env::var("REGRESSION_SEED")
                .unwrap_or_else(|_| "1".to_owned())
                .parse()
                .map_err(|_| fail("REGRESSION_SEED is not an integer"))?,
            report: root.join("reports/generated/regression.json"),
            quick: false,
            simulation_only: false,
            plan_json: false,
        };
        let mut saw_units = false;
        let mut saw_integrations = false;
        let mut index = 0;
        while index < arguments.len() {
            let name = &arguments[index];
            if matches!(
                name.as_str(),
                "--quick" | "--simulation-only" | "--plan-json"
            ) {
                match name.as_str() {
                    "--quick" => args.quick = true,
                    "--simulation-only" => args.simulation_only = true,
                    _ => args.plan_json = true,
                }
                index += 1;
                continue;
            }
            let value = arguments
                .get(index + 1)
                .ok_or_else(|| fail(format!("missing value for {name}")))?;
            match name.as_str() {
                "--unit-tests" => {
                    args.units = value.split_whitespace().map(str::to_owned).collect();
                    saw_units = true;
                }
                "--integration-tests" => {
                    args.integrations = value.split_whitespace().map(str::to_owned).collect();
                    saw_integrations = true;
                }
                "--variant" => args.variant = value.clone(),
                "--seed" => args.seed = value.parse().map_err(|_| fail("invalid --seed"))?,
                "--report" => args.report = PathBuf::from(value),
                _ => return Err(fail(format!("unknown argument: {name}"))),
            }
            index += 2;
        }
        if !saw_units || !saw_integrations {
            return Err(fail("--unit-tests and --integration-tests are required"));
        }
        if !args.report.is_absolute() {
            args.report = root.join(&args.report);
        }
        args.report = normalize(&args.report);
        if !args.quick && !args.report.starts_with(root) {
            return Err(fail(format!(
                "full regression report must be inside {}",
                root.display()
            )));
        }
        Ok(args)
    }
}

fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

#[derive(Clone, Debug, Serialize)]
struct Phase {
    name: String,
    command: Vec<String>,
}

fn make_phase(root: &Path, name: &str, target: &str, extra: &[String]) -> Phase {
    let mut command = vec![
        "make".to_owned(),
        "-C".to_owned(),
        root.join("scripts").to_string_lossy().into_owned(),
        target.to_owned(),
    ];
    command.extend_from_slice(extra);
    Phase {
        name: name.to_owned(),
        command,
    }
}

fn phases(args: &Args, root: &Path) -> Vec<Phase> {
    let mut result = Vec::new();
    macro_rules! add {
        ($name:expr, $target:expr $(,)?) => {
            result.push(make_phase(root, $name, $target, &[]))
        };
    }
    for (name, target) in [
        ("clean", "clean"),
        ("lint-rtl", "lint"),
        ("lint-mister-wrapper", "lint-mister"),
        ("lint-fpga-package-example", "lint-example"),
        ("lint-generic-soc", "lint-generic-soc"),
        ("lint-generic-soc-slang", "lint-generic-soc-slang"),
        ("generic-soc-rom-check", "generic-soc-rom-check"),
        ("lint-independent", "lint-independent"),
        ("cdc-rdc", "cdc-rdc"),
        ("sim-generic-soc", "sim-generic-soc"),
    ] {
        add!(name, target);
    }
    if !args.simulation_only {
        add!("quartus-analysis", "quartus-analysis");
        add!(
            "quartus-conformance-analysis",
            "quartus-conformance-analysis",
        );
        if !args.quick {
            for (name, target) in [
                ("quartus-compile", "quartus-compile"),
                ("quartus-conformance-compile", "quartus-conformance-compile"),
                (
                    "quartus-option-characterization",
                    "quartus-option-characterization",
                ),
                ("postfit-sim", "postfit-sim"),
                ("mister-framework", "mister-framework"),
            ] {
                add!(name, target);
            }
        }
    }
    for (name, target) in [
        ("lint-testbench", "lint-tb"),
        ("harness-unit", "harness-unit"),
        ("harness-expected-failure", "harness-self-test"),
        ("raw-checker-expected-failure", "raw-checker-self-test"),
    ] {
        add!(name, target);
    }
    let units = if args.quick {
        args.units.iter().take(1).collect::<Vec<_>>()
    } else {
        args.units.iter().collect::<Vec<_>>()
    };
    for unit in units {
        add!(&format!("unit-{unit}"), &format!("unit-{unit}"));
    }
    let integrations = if args.quick {
        let selected = args
            .integrations
            .iter()
            .filter(|name| QUICK_INTEGRATIONS.contains(&name.as_str()))
            .collect::<Vec<_>>();
        if selected.is_empty() {
            args.integrations.iter().take(1).collect::<Vec<_>>()
        } else {
            selected
        }
    } else {
        args.integrations.iter().collect::<Vec<_>>()
    };
    for integration in integrations {
        add!(
            &format!("integration-{integration}"),
            &format!("integ-{integration}"),
        );
    }
    if !args.quick {
        add!("formal", "formal");
        let selected_report = args.report.to_string_lossy();
        let assignment = format!("REGRESSION_REPORT={selected_report}");
        result.push(make_phase(
            root,
            "table7-cross",
            "table7-cross",
            std::slice::from_ref(&assignment),
        ));
        result.push(make_phase(
            root,
            "functional-coverage",
            "functional-coverage",
            &[assignment],
        ));
    }
    add!("public-suite", "public-suite");
    if args.quick {
        add!("random-validation-quick", "random-validation-quick");
    } else {
        add!("random-validation", "random-validation");
        add!("random-events", "random-events");
        add!("soak", "soak");
    }
    for (name, target) in [
        ("smoke", "run"),
        ("trm-coverage", "trm-coverage"),
        ("ac-timing", "ac-timing"),
        ("traceability", "traceability"),
    ] {
        add!(name, target);
    }
    result
}

fn run_text(arguments: &[&str], root: &Path) -> Result<String, Failure> {
    let (program, args) = arguments
        .split_first()
        .ok_or_else(|| fail("empty command"))?;
    let output = Command::new(program)
        .args(args)
        .current_dir(root)
        .output()?;
    if !output.status.success() {
        return Err(fail(format!(
            "{} exited {}: {}",
            arguments.join(" "),
            output.status.code().unwrap_or(1),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn first_line(value: String) -> String {
    value.lines().next().unwrap_or_default().to_owned()
}

fn manifest(args: &Args) -> Value {
    json!({
        "lint": [
            "raw-core", "mister-wrapper", "fpga-package-example",
            "generic-soc-verilator", "generic-soc-slang",
            "generic-soc-rom-equivalence", "all-public-tops-slang",
            "structural-cdc-rdc", "testbench",
        ],
        "harness": [
            "unit", "expected-failure", "raw-bus-checker",
            "trm-coverage", "ac-timing", "traceability",
        ],
        "fpga": if args.simulation_only {
            Vec::<&str>::new()
        } else {
            vec![
                "quartus-analysis", "quartus-conformance-analysis",
                "quartus-compile", "quartus-conformance-compile",
                "quartus-option-characterization", "postfit-sim",
                "mister-framework-build",
            ]
        },
        "unit": args.units,
        "integration": args.integrations,
        "public_suite": ["pinned-gba-suite-arm-thumb"],
        "cycle_cross": if args.quick {vec![]} else {vec!["chapter7-required-crosses"]},
        "functional_coverage": if args.quick {vec![]} else {vec!["armv4t-required-encoding-policy-bins"]},
        "constrained_random": [if args.quick {"2-seed-quick"} else {"32-seed-release"}],
        "random_events": if args.quick {vec![]} else {vec!["32-seed-all-class-all-event"]},
        "formal": if args.quick {vec![]} else {vec!["VAL-007-proofs-and-VAL-008-covers"]},
        "soak": if args.quick {vec![]} else {vec!["mister-wrapper-256-seed-sanitized"]},
        "examples": ["generic-soc"],
        "smoke": ["arm7tdmis_tb_top"],
    })
}

fn collect_metadata(args: &Args, root: &Path) -> Result<Value, Failure> {
    let status = run_text(
        &["git", "status", "--porcelain=v1", "--untracked-files=all"],
        root,
    )?;
    let platform = run_text(&["uname", "-srmo"], root)
        .unwrap_or_else(|_| format!("{}-{}", env::consts::OS, env::consts::ARCH));
    Ok(json!({
        "schema": SCHEMA,
        "started_utc": utc_now(),
        "git": {
            "commit": run_text(&["git", "rev-parse", "HEAD"], root)?,
            "describe": run_text(&["git", "describe", "--always", "--dirty", "--broken"], root)?,
            "dirty": !status.is_empty(),
            "status": status.lines().collect::<Vec<_>>(),
            "source_sha256": source_digest(root)?.to_owned(),
        },
        "tools": {
            "verilator": run_text(&["verilator", "--version"], root)?,
            "clang": first_line(run_text(&["clang", "--version"], root)?),
            "qemu": first_line(run_text(&["qemu-system-arm", "--version"], root)?),
            "quartus": if args.simulation_only {
                "not requested (simulation-only profile)".to_owned()
            } else {
                run_text(&["quartus_map", "--version"], root)?
            },
            "make": first_line(run_text(&["make", "--version"], root)?),
            "git": run_text(&["git", "--version"], root)?,
            "rustc": run_text(&["rustc", "--version"], root)?,
            "platform": platform,
        },
        "variant": args.variant,
        "seed": args.seed,
        "manifest": manifest(args),
    }))
}

fn report_log_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn run_phase(phase: &Phase, log_directory: &Path, root: &Path) -> Result<Value, Failure> {
    let started_utc = utc_now();
    let started = Instant::now();
    fs::create_dir_all(log_directory)?;
    let safe_name: String = phase
        .name
        .chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect();
    let log_path = log_directory.join(format!("{safe_name}.log"));
    println!(
        "\n[regression] START {}: {}",
        phase.name,
        phase.command.join(" ")
    );
    io::stdout().flush()?;
    let (receiver, sender) = UnixStream::pair()?;
    let stdout: OwnedFd = sender.try_clone()?.into();
    let stderr: OwnedFd = sender.into();
    let (program, arguments) = phase
        .command
        .split_first()
        .ok_or_else(|| fail("empty regression phase command"))?;
    let mut child = Command::new(program)
        .args(arguments)
        .current_dir(root)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|error| fail(format!("cannot start {}: {error}", phase.name)))?;
    let mut log = File::create(&log_path)?;
    let mut output = io::stdout().lock();
    let mut reader = BufReader::new(receiver);
    let mut line = Vec::new();
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        output.write_all(&line)?;
        output.flush()?;
        log.write_all(&line)?;
    }
    log.sync_all()?;
    let exit = child.wait()?;
    let exit_code = exit.code().unwrap_or_else(|| {
        use std::os::unix::process::ExitStatusExt;
        -exit.signal().unwrap_or(1)
    });
    let elapsed = (started.elapsed().as_secs_f64() * 1_000_000.0).round() / 1_000_000.0;
    let status = if exit.success() { "passed" } else { "failed" };
    println!(
        "[regression] {} {} ({elapsed:.3}s, exit={exit_code})",
        status.to_uppercase(),
        phase.name,
    );
    Ok(json!({
        "name": phase.name,
        "command": phase.command,
        "started_utc": started_utc,
        "finished_utc": utc_now(),
        "duration_seconds": elapsed,
        "exit_code": exit_code,
        "status": status,
        "log": report_log_path(root, &log_path),
        "log_sha256": sha256(&log_path)?,
    }))
}

pub fn command(arguments: &[String]) -> Result<(), String> {
    run(arguments).map_err(|error| error.to_string())
}

fn run(arguments: &[String]) -> Result<(), Failure> {
    let root = repo_root();
    let args = Args::parse(arguments, &root)?;
    let planned = phases(&args, &root);
    if args.plan_json {
        println!(
            "{}",
            json!({
                "phases": planned,
                "manifest": manifest(&args),
                "mode": if args.quick {"quick"} else {"full"},
                "scope": if args.simulation_only {"simulation-only"} else {"release"},
            })
        );
        return Ok(());
    }
    let parent = args
        .report
        .parent()
        .ok_or_else(|| fail("report has no parent"))?;
    fs::create_dir_all(parent)?;
    if !args.quick && !parent.canonicalize()?.starts_with(root.canonicalize()?) {
        return Err(fail(format!(
            "full regression report must be inside {}",
            root.display()
        )));
    }
    let report = collect_metadata(&args, &root)?;
    execute_plan(&args, &root, &planned, report)
}

fn execute_plan(
    args: &Args,
    root: &Path,
    planned: &[Phase],
    mut report: Value,
) -> Result<(), Failure> {
    let log_directory = args
        .report
        .parent()
        .ok_or_else(|| fail("report has no parent"))?
        .join("logs");
    report["mode"] = json!(if args.quick { "quick" } else { "full" });
    report["scope"] = json!(if args.simulation_only {
        "simulation-only"
    } else {
        "release"
    });
    report["status"] = json!("running");
    report["results"] = json!([]);
    write_json(&args.report, &report)?;
    println!(
        "[regression] commit={} source_sha256={} dirty={} variant={} seed={}",
        report["git"]["commit"],
        report["git"]["source_sha256"],
        report["git"]["dirty"],
        args.variant,
        args.seed
    );
    println!("[regression] report={}", args.report.display());
    for phase in planned {
        let result = match run_phase(phase, &log_directory, root) {
            Ok(result) => result,
            Err(error) => {
                report["status"] = json!("failed");
                report["finished_utc"] = json!(utc_now());
                report["failure"] = json!(error.to_string());
                write_json(&args.report, &report)?;
                return Err(error);
            }
        };
        let failed = result["status"] == "failed";
        report["results"].as_array_mut().unwrap().push(result);
        if failed {
            report["status"] = json!("failed");
            report["finished_utc"] = json!(utc_now());
            write_json(&args.report, &report)?;
            return Err(fail(format!("regression phase failed: {}", phase.name)));
        }
        write_json(&args.report, &report)?;
    }
    report["status"] = json!("passed");
    report["finished_utc"] = json!(utc_now());
    report["result_count"] = json!(report["results"].as_array().unwrap().len());
    write_json(&args.report, &report)?;
    println!("[regression] PASS ({} phases)", planned.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(quick: bool, simulation_only: bool) -> Args {
        Args {
            units: vec!["alpha".into(), "beta".into()],
            integrations: vec![
                "ordinary".into(),
                "compiler".into(),
                "qemu_diff".into(),
                "later".into(),
            ],
            variant: "test".into(),
            seed: 123,
            report: repo_root().join("reports/generated/alternate-regression.json"),
            quick,
            simulation_only,
            plan_json: false,
        }
    }

    #[test]
    fn full_and_quick_phase_plans_preserve_mandatory_gates() {
        let root = repo_root();
        let full = phases(&args(false, false), &root);
        let quick = phases(&args(true, false), &root);
        let ci = phases(&args(true, true), &root);
        fn names(phases: &[Phase]) -> Vec<&str> {
            phases.iter().map(|phase| phase.name.as_str()).collect()
        }
        assert_eq!(full.first().unwrap().name, "clean");
        assert!(names(&full).contains(&"quartus-option-characterization"));
        assert!(names(&full).contains(&"postfit-sim"));
        assert!(names(&full).contains(&"mister-framework"));
        assert!(!names(&quick).contains(&"postfit-sim"));
        assert!(names(&quick).contains(&"quartus-analysis"));
        assert!(!names(&ci).iter().any(|name| name.starts_with("quartus-")));
        assert!(names(&ci).contains(&"integration-compiler"));
        assert!(names(&ci).contains(&"integration-qemu_diff"));
        assert!(!names(&ci).contains(&"integration-ordinary"));
        assert!(names(&ci).contains(&"trm-coverage"));
        assert!(names(&ci).contains(&"ac-timing"));
        assert!(names(&ci).contains(&"traceability"));
        let table = full
            .iter()
            .find(|phase| phase.name == "table7-cross")
            .unwrap();
        let functional = full
            .iter()
            .find(|phase| phase.name == "functional-coverage")
            .unwrap();
        let assignment = format!("REGRESSION_REPORT={}", args(false, false).report.display());
        assert_eq!(table.command.last().unwrap(), &assignment);
        assert_eq!(functional.command.last().unwrap(), &assignment);
    }

    #[test]
    fn phase_log_has_exact_status_digest_and_is_not_inferred_from_marker() {
        let root = repo_root();
        fs::create_dir_all(root.join(".tools")).unwrap();
        let temporary = tempfile::Builder::new()
            .prefix("regression-contract-")
            .tempdir_in(root.join(".tools"))
            .unwrap();
        let passed = Phase {
            name: "pass".into(),
            command: vec!["sh".into(), "-c".into(), "printf 'PASS\\n'".into()],
        };
        let result = run_phase(&passed, temporary.path(), &root).unwrap();
        assert_eq!(result["status"], "passed");
        assert_eq!(result["exit_code"], 0);
        let log = root.join(result["log"].as_str().unwrap());
        assert_eq!(result["log_sha256"], sha256(&log).unwrap());
        let failed = Phase {
            name: "fail".into(),
            command: vec!["sh".into(), "-c".into(), "printf 'PASS\\n'; exit 7".into()],
        };
        let result = run_phase(&failed, temporary.path(), &root).unwrap();
        assert_eq!(result["status"], "failed");
        assert_eq!(result["exit_code"], 7);
    }

    #[test]
    fn failed_phase_persists_complete_report_and_stops_the_plan() {
        let root = repo_root();
        let temporary = tempfile::tempdir().unwrap();
        let mut args = args(true, true);
        args.report = temporary.path().join("regression.json");
        let planned = vec![
            Phase {
                name: "first".into(),
                command: vec!["sh".into(), "-c".into(), "printf 'first\\n'".into()],
            },
            Phase {
                name: "failure".into(),
                command: vec![
                    "sh".into(),
                    "-c".into(),
                    "printf 'failure\\n'; exit 7".into(),
                ],
            },
            Phase {
                name: "never-run".into(),
                command: vec!["sh".into(), "-c".into(), "exit 0".into()],
            },
        ];
        let metadata = json!({
            "git": {"commit": "abc", "source_sha256": "def", "dirty": true},
            "manifest": manifest(&args),
        });
        assert!(execute_plan(&args, &root, &planned, metadata).is_err());
        let report: Value = serde_json::from_slice(&fs::read(&args.report).unwrap()).unwrap();
        assert_eq!(report["status"], "failed");
        assert_eq!(report["mode"], "quick");
        assert_eq!(report["scope"], "simulation-only");
        assert!(report["finished_utc"].is_string());
        let results = report["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["status"], "passed");
        assert_eq!(results[1]["status"], "failed");
        assert_eq!(results[1]["exit_code"], 7);
        for result in results {
            let log = PathBuf::from(result["log"].as_str().unwrap());
            assert_eq!(result["log_sha256"], sha256(&log).unwrap());
        }
        assert!(!temporary.path().join("logs/never_run.log").exists());
    }

    #[test]
    fn full_report_path_cannot_leave_checkout() {
        let root = repo_root();
        let outside = tempfile::tempdir().unwrap();
        let parse = |quick| {
            Args::parse(
                &[
                    "--unit-tests".into(),
                    "a".into(),
                    "--integration-tests".into(),
                    "b".into(),
                    "--report".into(),
                    outside
                        .path()
                        .join("report.json")
                        .to_string_lossy()
                        .into_owned(),
                    if quick {
                        "--quick"
                    } else {
                        "--simulation-only"
                    }
                    .into(),
                ],
                &root,
            )
        };
        assert!(parse(false).is_err());
        assert!(parse(true).is_ok());
    }
}
