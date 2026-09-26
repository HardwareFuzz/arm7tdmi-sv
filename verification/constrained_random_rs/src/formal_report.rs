//! Run the pinned SBY matrix and emit fail-hard VAL-007/VAL-008 evidence.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use chrono::{SecondsFormat, Utc};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::campaign::{repo_root, run_command};
use crate::pinned_archive::{file_sha256, run};

const SCHEMA: &str = "arm7tdmis-formal-v1";

#[derive(Deserialize)]
struct Manifest {
    toolchain: Value,
    proofs: Vec<Proof>,
    covers: Covers,
    cover_tasks: CoverTasks,
}

#[derive(Deserialize)]
struct Proof {
    id: String,
    task: String,
    engine: String,
    depth: usize,
    property: String,
    source: String,
}

#[derive(Deserialize)]
struct Covers {
    fsm_states: Vec<String>,
    fsm_transitions: Vec<String>,
    exceptions: Vec<String>,
    debug: Vec<String>,
}

#[derive(Clone, Deserialize)]
struct CoverTask {
    task: String,
    engine: String,
    depth: usize,
    source: String,
}

#[derive(Deserialize)]
struct CoverTasks {
    fsm: CoverTask,
    exception: CoverTask,
    debug: CoverTask,
}

#[derive(Clone)]
struct CoverObligation {
    task: String,
    property: String,
    settings: CoverTask,
}

struct TaskEvidence {
    summary: String,
    log: Value,
    covers: BTreeMap<String, Vec<Value>>,
}

struct Args {
    manifest: PathBuf,
    sby_file: PathBuf,
    output: PathBuf,
    work_prefix: PathBuf,
    tool_directory: PathBuf,
    jobs: usize,
}

impl Args {
    fn parse(arguments: &[String]) -> Result<Self, String> {
        let root = repo_root();
        let mut args = Self {
            manifest: root.join("verification/formal_requirements.json"),
            sby_file: root.join("tb/formal/formal.sby"),
            output: root.join("reports/generated/formal-report.json"),
            work_prefix: root.join("reports/generated/formal/work"),
            tool_directory: root.join(".tools/oss-cad-suite-20260729"),
            jobs: 2,
        };
        let mut values = arguments.iter();
        while let Some(argument) = values.next() {
            let value = values
                .next()
                .ok_or_else(|| format!("missing value for {argument}"))?;
            match argument.as_str() {
                "--manifest" => args.manifest = PathBuf::from(value),
                "--sby-file" => args.sby_file = PathBuf::from(value),
                "--output" => args.output = PathBuf::from(value),
                "--work-prefix" => args.work_prefix = PathBuf::from(value),
                "--tool-directory" => args.tool_directory = PathBuf::from(value),
                "--jobs" => {
                    args.jobs = value
                        .parse()
                        .map_err(|_| format!("invalid --jobs: {value}"))?;
                }
                other => return Err(format!("unknown formal-report argument: {other}")),
            }
        }
        if args.jobs == 0 {
            return Err("--jobs must be positive".into());
        }
        let absolute = |path: PathBuf| -> Result<PathBuf, String> {
            if path.is_absolute() {
                Ok(path)
            } else {
                Ok(std::env::current_dir()
                    .map_err(|error| error.to_string())?
                    .join(path))
            }
        };
        args.manifest = absolute(args.manifest)?;
        args.sby_file = absolute(args.sby_file)?;
        args.output = absolute(args.output)?;
        args.work_prefix = absolute(args.work_prefix)?;
        args.tool_directory = absolute(args.tool_directory)?;
        Ok(args)
    }
}

fn obligations(manifest: &Manifest) -> Result<BTreeMap<String, CoverObligation>, String> {
    let mut result = BTreeMap::new();
    let mut add = |id: String, property: String, settings: &CoverTask| {
        result.insert(
            id,
            CoverObligation {
                task: settings.task.clone(),
                property,
                settings: settings.clone(),
            },
        );
    };
    for state in &manifest.covers.fsm_states {
        add(
            format!("fsm.state.{state}"),
            format!("cover_state_{state}"),
            &manifest.cover_tasks.fsm,
        );
    }
    for transition in &manifest.covers.fsm_transitions {
        let (start, finish) = transition
            .split_once("->")
            .ok_or_else(|| format!("invalid FSM transition: {transition}"))?;
        add(
            format!("fsm.transition.{transition}"),
            format!("cover_transition_{start}_to_{finish}"),
            &manifest.cover_tasks.fsm,
        );
    }
    for exception in &manifest.covers.exceptions {
        add(
            format!("exception.{exception}"),
            format!("cover_exception_{}", exception.replace('.', "_")),
            &manifest.cover_tasks.exception,
        );
    }
    for debug in &manifest.covers.debug {
        add(
            format!("debug.{debug}"),
            format!("cover_debug_{}", debug.replace('.', "_")),
            &manifest.cover_tasks.debug,
        );
    }
    Ok(result)
}

fn file_entry(path: &Path, root: &Path) -> Result<Value, String> {
    let relative = path.strip_prefix(root).map_err(|error| error.to_string())?;
    let size = fs::metadata(path).map_err(|error| error.to_string())?.len();
    Ok(json!({
        "path": relative.to_string_lossy().replace('\\', "/"),
        "bytes": size,
        "sha256": file_sha256(path)?,
    }))
}

fn cover_witnesses(
    task_dir: &Path,
    summary: &str,
    root: &Path,
) -> Result<BTreeMap<String, Vec<Value>>, String> {
    let mut current = Vec::<PathBuf>::new();
    let mut result = BTreeMap::new();
    for raw in summary.lines() {
        let line = raw.trim();
        if let Some(relative) = line.strip_prefix("cover trace: ") {
            let relative = relative.trim();
            if relative.ends_with(".vcd") {
                current.clear();
            }
            let witness = task_dir.join(relative);
            if !witness.is_file() {
                return Err(format!(
                    "cover summary references missing witness: {}",
                    witness.display()
                ));
            }
            current.push(witness);
            continue;
        }
        let Some((_, qualified)) = line.split_once("reached cover statement ") else {
            continue;
        };
        let qualified = qualified.split_whitespace().next().unwrap_or_default();
        let property = qualified.rsplit('.').next().unwrap_or_default().to_owned();
        let suffixes: BTreeSet<_> = current
            .iter()
            .filter_map(|path| path.extension().and_then(|value| value.to_str()))
            .collect();
        if suffixes != BTreeSet::from(["vcd", "yw"]) {
            return Err(format!("{property} lacks a VCD/Yosys witness pair"));
        }
        let mut entries = Vec::new();
        for witness in &current {
            entries.push(file_entry(witness, root)?);
        }
        result.insert(property, entries);
    }
    Ok(result)
}

fn git() -> Result<Value, String> {
    let output = |parts: &[&str]| -> Result<String, String> {
        run_command(
            &parts
                .iter()
                .map(|value| (*value).to_owned())
                .collect::<Vec<_>>(),
            None,
        )
        .map(|text| text.trim().to_owned())
        .map_err(|error| error.to_string())
    };
    let status = output(&["git", "status", "--porcelain=v1", "--untracked-files=all"])?;
    let commit = output(&["git", "rev-parse", "HEAD"])?;
    Ok(json!({
        "commit": commit,
        "dirty": !status.is_empty(),
        "status": status.lines().collect::<Vec<_>>(),
    }))
}

fn write_report(path: &Path, report: &Value) -> Result<(), String> {
    let parent = path.parent().ok_or("formal report path has no parent")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    serde_json::to_writer_pretty(&mut temporary, report).map_err(|error| error.to_string())?;
    std::io::Write::write_all(&mut temporary, b"\n").map_err(|error| error.to_string())?;
    temporary.persist(path).map_err(|error| error.to_string())?;
    Ok(())
}

fn formal(args: &Args) -> Result<String, String> {
    let root = repo_root();
    let manifest: Manifest =
        serde_json::from_slice(&fs::read(&args.manifest).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let obligations = obligations(&manifest)?;
    let proofs: BTreeMap<_, _> = manifest
        .proofs
        .iter()
        .map(|proof| (proof.id.clone(), proof))
        .collect();
    let tasks: BTreeSet<_> = manifest
        .proofs
        .iter()
        .map(|proof| proof.task.clone())
        .chain(obligations.values().map(|cover| cover.task.clone()))
        .collect();
    let sby = args.tool_directory.join("bin/sby");
    if !sby.is_file() {
        return Err("pinned sby is missing: run make -C scripts formal".into());
    }

    let runner_log = args
        .output
        .parent()
        .ok_or("formal report path has no parent")?
        .join("formal/runner.log");
    fs::create_dir_all(runner_log.parent().unwrap()).map_err(|error| error.to_string())?;
    let log = File::create(&runner_log).map_err(|error| error.to_string())?;
    println!(
        "[formal] START {}",
        tasks.iter().cloned().collect::<Vec<_>>().join(" ")
    );
    let status = Command::new(&sby)
        .arg("-f")
        .arg("-j")
        .arg(args.jobs.to_string())
        .arg("--prefix")
        .arg(&args.work_prefix)
        .arg(&args.sby_file)
        .args(&tasks)
        .current_dir(&root)
        .stdout(Stdio::from(
            log.try_clone().map_err(|error| error.to_string())?,
        ))
        .stderr(Stdio::from(log))
        .status()
        .map_err(|error| error.to_string())?;

    let mut task_evidence = BTreeMap::<String, TaskEvidence>::new();
    let mut failures = Vec::<String>::new();
    for task in &tasks {
        let task_dir = PathBuf::from(format!("{}_{}", args.work_prefix.display(), task));
        let pass = task_dir.join("PASS");
        let logfile = task_dir.join("logfile.txt");
        if !pass.is_file() || !logfile.is_file() {
            failures.push(format!("{task}: missing PASS or logfile.txt"));
            continue;
        }
        let summary = String::from_utf8_lossy(&fs::read(pass).map_err(|error| error.to_string())?)
            .into_owned();
        task_evidence.insert(
            task.clone(),
            TaskEvidence {
                covers: cover_witnesses(&task_dir, &summary, &root)?,
                log: file_entry(&logfile, &root)?,
                summary,
            },
        );
    }
    for (id, cover) in &obligations {
        let reached = task_evidence.get(&cover.task).is_some_and(|evidence| {
            evidence.summary.contains(&cover.property)
                && evidence.covers.contains_key(&cover.property)
        });
        if !reached {
            failures.push(format!(
                "{id}: {} was not reached by {}",
                cover.property, cover.task
            ));
        }
    }

    let required_proofs: Vec<_> = proofs.keys().cloned().collect();
    let required_covers: Vec<_> = obligations.keys().cloned().collect();
    let passed = status.success() && failures.is_empty();
    let mut results = BTreeMap::<String, Value>::new();
    if passed {
        for (id, proof) in &proofs {
            let evidence = task_evidence
                .get(&proof.task)
                .ok_or("missing proof task evidence")?;
            results.insert(
                id.clone(),
                json!({
                    "status": "passed", "task": proof.task, "property": proof.property,
                    "engine": proof.engine, "depth": proof.depth, "log": evidence.log,
                    "source": file_entry(&root.join(&proof.source), &root)?,
                }),
            );
        }
        for (id, cover) in &obligations {
            let evidence = task_evidence
                .get(&cover.task)
                .ok_or("missing cover task evidence")?;
            results.insert(id.clone(), json!({
                "status": "passed", "task": cover.task, "property": cover.property,
                "engine": cover.settings.engine, "depth": cover.settings.depth,
                "log": evidence.log,
                "source": file_entry(&root.join(&cover.settings.source), &root)?,
                "witnesses": evidence.covers.get(&cover.property).ok_or("missing cover witnesses")?,
            }));
        }
    }
    let version = |name: &str| -> Result<String, String> {
        let output =
            run(Command::new(args.tool_directory.join("bin").join(name)).arg("--version"))?;
        Ok(output.trim().lines().next().unwrap_or_default().to_owned())
    };
    let report = json!({
        "schema": SCHEMA,
        "status": if passed { "passed" } else { "failed" },
        "failure": if passed { Value::Null } else { json!({
            "runner_exit_code": status.code().unwrap_or(1),
            "messages": failures,
            "runner_log": file_entry(&runner_log, &root)?,
        }) },
        "created_utc": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, false),
        "git": git()?,
        "toolchain": manifest.toolchain,
        "tools": {"sby": version("sby")?, "yosys": version("yosys")?, "boolector": version("boolector")?},
        "manifest": file_entry(&args.manifest, &root)?,
        "sby_file": file_entry(&args.sby_file, &root)?,
        "runner_log": file_entry(&runner_log, &root)?,
        "required_proofs": required_proofs,
        "proven_proofs": if passed { required_proofs.clone() } else { Vec::new() },
        "required_covers": required_covers,
        "covered_covers": if passed { required_covers.clone() } else { Vec::new() },
        "uncovered_covers": if passed { Vec::new() } else { required_covers.clone() },
        "results": results,
    });
    write_report(&args.output, &report)?;
    if !passed {
        return Err(format!("[formal] FAIL; inspect {}", runner_log.display()));
    }
    Ok(format!(
        "[formal] PASS {} proofs, {} covers",
        required_proofs.len(),
        required_covers.len()
    ))
}

pub fn command(arguments: &[String]) -> Result<String, String> {
    formal(&Args::parse(arguments)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_freezes_each_formal_proof_and_cover_obligation() {
        let root = repo_root();
        let value: Value = serde_json::from_slice(
            &fs::read(root.join("verification/formal_requirements.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(value["schema"], "arm7tdmis-formal-map-v1");
        assert_eq!(
            value["toolchain"],
            json!({
                "name": "oss-cad-suite",
                "version": "2026-07-29",
                "archive_sha256": "89ea1152ea84bc600f18cc685f721d534d1f018e09831662787865a3d79ce4aa",
            })
        );
        let expected_proofs: BTreeSet<_> = "no_ghost_commits condition_suppression mode_bank_isolation pc_alignment flag_preservation bus_stability_clken one_completion_per_request lock_lifetime exception_priority abort_suppression tap_transitions cdc_reset_assumptions forward_progress no_deadlock".split_whitespace().collect();
        let actual_proofs: BTreeSet<_> = value["proofs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|proof| proof["id"].as_str().unwrap())
            .collect();
        assert_eq!(actual_proofs, expected_proofs);

        for (field, expected) in [
            ("fsm_states", "S_EXEC S_DDATA S_BLOCK_DATA S_SWP_RDATA S_SWP_WDATA S_MULL_HI S_MUL_BUSY S_MULL_ACC S_BLOCK_WB S_DP_SHIFT S_LOAD_WB S_SWP_WB S_CP_WAIT S_CP_MCR_DATA S_CP_MRC_DATA S_CP_MRC_WB S_UNDEF_WAIT"),
            ("fsm_transitions", "S_EXEC->S_EXEC S_EXEC->S_DDATA S_EXEC->S_BLOCK_DATA S_EXEC->S_SWP_RDATA S_EXEC->S_MUL_BUSY S_EXEC->S_MULL_ACC S_EXEC->S_DP_SHIFT S_EXEC->S_CP_WAIT S_EXEC->S_CP_MCR_DATA S_EXEC->S_CP_MRC_DATA S_EXEC->S_UNDEF_WAIT S_DDATA->S_EXEC S_DDATA->S_LOAD_WB S_LOAD_WB->S_EXEC S_BLOCK_DATA->S_BLOCK_DATA S_BLOCK_DATA->S_BLOCK_WB S_BLOCK_DATA->S_EXEC S_BLOCK_WB->S_BLOCK_WB S_BLOCK_WB->S_EXEC S_SWP_RDATA->S_EXEC S_SWP_RDATA->S_SWP_WDATA S_SWP_WDATA->S_SWP_WB S_SWP_WB->S_EXEC S_MUL_BUSY->S_MUL_BUSY S_MUL_BUSY->S_MULL_HI S_MUL_BUSY->S_EXEC S_MULL_ACC->S_MUL_BUSY S_MULL_HI->S_EXEC S_DP_SHIFT->S_EXEC S_CP_WAIT->S_CP_WAIT S_CP_WAIT->S_CP_MCR_DATA S_CP_WAIT->S_CP_MRC_DATA S_CP_WAIT->S_EXEC S_CP_MCR_DATA->S_CP_MCR_DATA S_CP_MCR_DATA->S_CP_MRC_WB S_CP_MCR_DATA->S_EXEC S_CP_MRC_DATA->S_CP_MRC_DATA S_CP_MRC_DATA->S_CP_MRC_WB S_CP_MRC_WB->S_EXEC S_UNDEF_WAIT->S_EXEC"),
            ("exceptions", "entry.reset entry.undefined entry.swi entry.prefetch_abort entry.data_abort entry.irq entry.fiq return.undefined return.swi return.prefetch_abort return.data_abort return.irq return.fiq"),
            ("debug", "entry.breakpoint entry.watchpoint entry.dbgrq entry.external_break return.restart return.pc_resume return.system_speed"),
        ] {
            let actual: BTreeSet<_> = value["covers"][field]
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| entry.as_str().unwrap())
                .collect();
            assert_eq!(actual, expected.split_whitespace().collect(), "{field}");
        }
        for proof in value["proofs"].as_array().unwrap() {
            assert!(matches!(proof["mode"].as_str(), Some("prove" | "live")));
            assert!(proof["property"]
                .as_str()
                .is_some_and(|text| !text.is_empty()));
            assert!(proof["citation"]
                .as_str()
                .is_some_and(|text| { text.contains("ARM DDI 0234") || text.contains("VAL-007") }));
            assert!(root.join(proof["source"].as_str().unwrap()).is_file());
        }
        for source in value["sources"].as_array().unwrap() {
            assert!(root.join(source.as_str().unwrap()).is_file());
        }
    }

    #[test]
    fn pinned_formal_flow_is_in_regression_ci_and_release_archive() {
        let root = repo_root();
        let read = |path: &str| fs::read_to_string(root.join(path)).unwrap();
        let installer = read("verification/constrained_random_rs/src/oss_cad_suite.rs");
        for marker in [
            "2026-07-29",
            "89ea1152ea84bc600f18cc685f721d534d1f018e09831662787865a3d79ce4aa",
        ] {
            assert!(installer.contains(marker));
        }
        let makefile = read("scripts/Makefile");
        assert!(makefile.contains("formal:"));
        assert!(makefile.contains("formal-report"));
        let regression = read("verification/constrained_random_rs/src/regression.rs");
        assert!(regression.contains("\"formal\""));
        assert!(regression.contains("if !args.quick"));
        assert!(
            read("verification/constrained_random_rs/src/release_evidence/files.rs")
                .contains("formal-report.json")
        );
        assert!(
            read("verification/constrained_random_rs/src/release_evidence.rs")
                .contains("validate_formal")
        );
        let workflow = read(".github/workflows/verification.yml");
        assert!(workflow.contains("formal"));
        assert!(workflow.contains("oss-cad-suite-install"));
        let traceability = read("verification/traceability.json");
        assert!(traceability.contains("\"VAL-007\""));
        assert!(traceability.contains("\"VAL-008\""));
        let tasks = read("TASKS.md");
        assert!(tasks.contains("- [x] **VAL-007:**"));
        assert!(tasks.contains("- [x] **VAL-008:**"));
        assert!(read("docs/VERIFICATION.md").contains("arm7tdmis-formal-v1"));
    }

    #[test]
    fn pinned_manifest_expands_every_cover_obligation() {
        let path = repo_root().join("verification/formal_requirements.json");
        let manifest: Manifest = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        let expanded = obligations(&manifest).unwrap();
        assert_eq!(manifest.proofs.len(), 14);
        assert_eq!(expanded.len(), 77);
        assert_eq!(
            expanded["exception.entry.undefined"].property,
            "cover_exception_entry_undefined"
        );
        assert_eq!(
            expanded["fsm.transition.S_EXEC->S_DDATA"].property,
            "cover_transition_S_EXEC_to_S_DDATA"
        );
    }
}
