//! VAL-010 deterministic sanitizer/X-state soak with failure-preserving
//! seed minimization and an exact reproducer.

use std::collections::BTreeSet;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use regex::Regex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use wait_timeout::ChildExt;

use crate::campaign::{git_text, repo_root, run_command, sha256, utc_now, write_json, Failure};

const SCHEMA: &str = "arm7tdmis-soak-v1";
const ASAN_OPTIONS: &str = "detect_leaks=1:halt_on_error=1:strict_string_checks=1";
const UBSAN_OPTIONS: &str = "halt_on_error=1:print_stacktrace=1";

fn failure(message: impl Into<String>) -> Failure {
    Failure::other(message)
}

fn option_or(arguments: &[String], name: &str, default: &str) -> Result<String, Failure> {
    if arguments.iter().any(|argument| argument == name) {
        crate::option(arguments, name).map_err(failure)
    } else {
        Ok(default.to_owned())
    }
}

fn absolute(path: PathBuf) -> Result<PathBuf, Failure> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn path_text(path: &Path) -> Result<String, Failure> {
    let path = fs::canonicalize(path)?;
    Ok(path
        .strip_prefix(repo_root())
        .unwrap_or(&path)
        .to_string_lossy()
        .into_owned())
}

fn seeds(base_seed: i64, count: usize) -> Result<Vec<u16>, Failure> {
    if count == 0 || count > 65_535 {
        return Err(failure("--seeds must be between 1 and 65535"));
    }
    let mut state = (base_seed as u16).max(1);
    let mut result = Vec::with_capacity(count);
    let mut seen = BTreeSet::new();
    while result.len() < count {
        if state != 0 {
            if !seen.insert(state) {
                return Err(failure("16-bit soak seed sequence repeated early"));
            }
            result.push(state);
        }
        state = state.wrapping_mul(25_173).wrapping_add(13_849);
    }
    Ok(result)
}

fn command(binary: &Path, seed: u16) -> Vec<String> {
    vec![
        binary.to_string_lossy().into_owned(),
        format!("+SEED={seed}"),
        format!("+verilator+seed+{seed}"),
    ]
}

fn shell_word(word: &str) -> String {
    if !word.is_empty()
        && word
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_/@%+=:,.-".contains(&byte))
    {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', "'\"'\"'"))
    }
}

fn shell_command(command: &[String]) -> String {
    command
        .iter()
        .map(|word| shell_word(word))
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_diagnostic(text: &str) -> String {
    Regex::new(r"0x[0-9a-fA-F]+|\b\d+\b")
        .unwrap()
        .replace_all(text, "#")
        .into_owned()
}

fn failure_signature(timed_out: bool, exit_code: Option<i32>, output: &str) -> String {
    if timed_out {
        return "timeout".into();
    }
    let address = Regex::new(r"ERROR: AddressSanitizer: ([^\s]+)").unwrap();
    if let Some(marker) = address.captures(output) {
        return format!("sanitizer:address:{}", &marker[1]);
    }
    if let Some(line) = output.lines().find(|line| line.contains("runtime error:")) {
        return format!("sanitizer:undefined:{}", normalize_diagnostic(line.trim()));
    }
    if let Some(diagnostic) = output.lines().find_map(|line| {
        line.split_once("[mister_wrapper] FAIL:")
            .map(|(_, text)| text.trim())
    }) {
        if !diagnostic.is_empty() {
            return format!("testbench:{}", normalize_diagnostic(diagnostic));
        }
    }
    if let Some(code) = exit_code.filter(|code| *code < 0) {
        return format!("signal:{}", -code);
    }
    match exit_code {
        Some(code) => format!("exit-code:{code}"),
        None => "exit-code:None".into(),
    }
}

#[cfg(unix)]
fn exit_code(status: ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    status
        .code()
        .or_else(|| status.signal().map(|signal| -signal))
}

#[cfg(not(unix))]
fn exit_code(status: ExitStatus) -> Option<i32> {
    status.code()
}

fn execute(binary: &Path, seed: u16, timeout: Duration) -> Result<(bool, Value, String), Failure> {
    let command = command(binary, seed);
    let scratch = tempfile::tempfile()?;
    let started = Instant::now();
    let mut child = Command::new(binary)
        .args(&command[1..])
        .current_dir(repo_root())
        .env("ASAN_OPTIONS", ASAN_OPTIONS)
        .env("UBSAN_OPTIONS", UBSAN_OPTIONS)
        .stdout(Stdio::from(scratch.try_clone()?))
        .stderr(Stdio::from(scratch.try_clone()?))
        .spawn()?;
    let status = child.wait_timeout(timeout)?;
    let timed_out = status.is_none();
    if timed_out {
        child.kill()?;
        child.wait()?;
    }
    let code = status.and_then(exit_code);
    let mut scratch = scratch;
    scratch.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    scratch.read_to_end(&mut bytes)?;
    let mut output = String::from_utf8_lossy(&bytes).into_owned();
    if timed_out {
        output.push_str(&format!(
            "\n[soak] TIMEOUT after {:.3} seconds",
            timeout.as_secs_f64()
        ));
    }
    let marker = format!("[mister_wrapper] PASS seed={seed}");
    let marker_found = output
        .lines()
        .any(|line| line.trim_end_matches('\r') == marker);
    let passed = !timed_out && code == Some(0) && marker_found;
    let signature = (!passed).then(|| failure_signature(timed_out, code, &output));
    let result = json!({
        "seed": seed,
        "command": command,
        "duration_seconds": (started.elapsed().as_secs_f64() * 1_000_000.0).round() / 1_000_000.0,
        "exit_code": code,
        "timed_out": timed_out,
        "pass_marker_found": marker_found,
        "output_sha256": format!("{:x}", Sha256::digest(output.as_bytes())),
        "failure_signature": signature,
        "status": if passed { "passed" } else { "failed" },
    });
    Ok((passed, result, output))
}

fn still_fails(
    binary: &Path,
    seed: u16,
    timeout: Duration,
    signature: Option<&str>,
) -> Result<bool, Failure> {
    let (passed, result, _) = execute(binary, seed, timeout)?;
    Ok(!passed && signature.is_none_or(|required| result["failure_signature"] == required))
}

fn minimize(
    binary: &Path,
    failing_seed: u16,
    timeout: Duration,
    signature: Option<&str>,
) -> Result<u16, Failure> {
    let mut minimized = failing_seed;
    for bit in (0..16).rev() {
        let candidate = minimized & !(1 << bit);
        if candidate != 0 && still_fails(binary, candidate, timeout, signature)? {
            minimized = candidate;
        }
    }
    Ok(minimized)
}

fn failure_artifacts(
    binary: &Path,
    failing_seed: u16,
    minimized_seed: u16,
    timeout: Duration,
    output: &str,
    directory: &Path,
    signature: &str,
) -> Result<Value, Failure> {
    fs::create_dir_all(directory)?;
    let log = directory.join(format!("seed-{failing_seed}.log"));
    let reproducer = directory.join(format!("seed-{failing_seed}-reproducer.json"));
    fs::write(&log, output)?;
    let repro_command = command(binary, minimized_seed);
    let value = json!({
        "schema": "arm7tdmis-soak-reproducer-v1",
        "failing_seed": failing_seed,
        "minimized_seed": minimized_seed,
        "failure_signature": signature,
        "binary": binary.to_string_lossy(),
        "binary_sha256": sha256(binary)?,
        "command": repro_command,
        "command_shell": shell_command(&repro_command),
        "timeout_seconds": timeout.as_secs_f64(),
        "environment": {"ASAN_OPTIONS": ASAN_OPTIONS, "UBSAN_OPTIONS": UBSAN_OPTIONS},
        "failure_log": path_text(&log)?,
        "failure_log_sha256": sha256(&log)?,
    });
    write_json(&reproducer, &value)?;
    Ok(json!({"log": path_text(&log)?, "reproducer": path_text(&reproducer)?}))
}

pub fn soak(arguments: &[String]) -> Result<(), String> {
    run(arguments).map_err(|error| error.to_string())
}

fn run(arguments: &[String]) -> Result<(), Failure> {
    let root = repo_root();
    let binary = fs::canonicalize(crate::option(arguments, "--binary").map_err(failure)?)?;
    if !binary.is_file() {
        return Err(failure(format!(
            "simulator binary is missing: {}",
            binary.display()
        )));
    }
    let seed_count: usize = option_or(arguments, "--seeds", "256")?
        .parse()
        .map_err(|_| failure("--seeds must be positive"))?;
    let base_seed: i64 = option_or(arguments, "--base-seed", "1")?
        .parse()
        .map_err(|_| failure("invalid --base-seed"))?;
    let timeout_seconds: f64 = option_or(arguments, "--timeout", "30")?
        .parse()
        .map_err(|_| failure("invalid --timeout"))?;
    if !timeout_seconds.is_finite() || timeout_seconds <= 0.0 {
        return Err(failure("--timeout must be positive and finite"));
    }
    let timeout = Duration::from_secs_f64(timeout_seconds);
    let output = absolute(PathBuf::from(option_or(
        arguments,
        "--output",
        &root
            .join("reports/generated/soak-report.json")
            .to_string_lossy(),
    )?))?;
    let failure_directory = absolute(PathBuf::from(option_or(
        arguments,
        "--failure-directory",
        &root
            .join("reports/generated/soak-failures")
            .to_string_lossy(),
    )?))?;
    let sequence = seeds(base_seed, seed_count)?;
    let binary_dependencies =
        run_command(&["ldd".into(), binary.to_string_lossy().into_owned()], None)?;
    for library in ["libasan", "libubsan"] {
        if !binary_dependencies.contains(library) {
            return Err(failure(format!(
                "simulator binary is not linked to {library}"
            )));
        }
    }
    let status = git_text(&["status", "--porcelain=v1", "--untracked-files=all"])?;
    let testbench = root.join("tb/integration/arm7tdmis_mister_wrapper_tb.sv");
    let mut report = json!({
        "schema": SCHEMA,
        "started_utc": utc_now(),
        "status": "running",
        "git": {"commit": git_text(&["rev-parse", "HEAD"])?, "dirty": !status.is_empty(), "status": status.lines().collect::<Vec<_>>()},
        "tools": {
            "verilator": run_command(&["verilator".into(), "--version".into()], None)?.trim(),
            "rust": env!("CARGO_PKG_VERSION"),
            "platform": run_command(&["uname".into(), "-srm".into()], None)?.trim(),
            "binary_dependencies": binary_dependencies.lines().collect::<Vec<_>>(),
        },
        "configuration": {
            "base_seed": base_seed, "seed_count": seed_count, "timeout_seconds": timeout_seconds,
            "x_assignment": "unique", "x_initialization": "unique",
            "sanitizers": ["address", "undefined"],
            "environment": {"ASAN_OPTIONS": ASAN_OPTIONS, "UBSAN_OPTIONS": UBSAN_OPTIONS},
        },
        "inputs": {
            "binary": {"path": binary.to_string_lossy(), "bytes": fs::metadata(&binary)?.len(), "sha256": sha256(&binary)?},
            "testbench": {"path": "tb/integration/arm7tdmis_mister_wrapper_tb.sv", "bytes": fs::metadata(&testbench)?.len(), "sha256": sha256(&testbench)?},
        },
        "seeds": [], "failures": [],
    });
    write_json(&output, &report)?;
    for (index, seed) in sequence.iter().enumerate() {
        println!("[soak] seed {}/{}: {seed}", index + 1, sequence.len());
        let (passed, result, simulation_output) = execute(&binary, *seed, timeout)?;
        report["seeds"].as_array_mut().unwrap().push(result);
        if !passed {
            println!("[soak] minimizing failing seed {seed}");
            let signature = report["seeds"].as_array().unwrap().last().unwrap()
                ["failure_signature"]
                .as_str()
                .unwrap()
                .to_owned();
            let minimized = minimize(&binary, *seed, timeout, Some(&signature))?;
            let artifacts = failure_artifacts(
                &binary,
                *seed,
                minimized,
                timeout,
                &simulation_output,
                &failure_directory,
                &signature,
            )?;
            report["failures"].as_array_mut().unwrap().push(json!({
                "failing_seed": seed, "minimized_seed": minimized, "failure_signature": signature,
                "log": artifacts["log"], "reproducer": artifacts["reproducer"],
            }));
            report["status"] = json!("failed");
            report["finished_utc"] = json!(utc_now());
            report["completed_seed_count"] = json!(report["seeds"].as_array().unwrap().len());
            write_json(&output, &report)?;
            println!(
                "[soak] FAIL seed={seed} minimized={minimized} reproducer={}",
                artifacts["reproducer"].as_str().unwrap()
            );
            return Err(failure("sanitizer soak failed"));
        }
        write_json(&output, &report)?;
    }
    report["status"] = json!("passed");
    report["finished_utc"] = json!(utc_now());
    report["completed_seed_count"] = json!(sequence.len());
    write_json(&output, &report)?;
    println!("[soak] PASS ({} deterministic seeds)", sequence.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_wiring_keeps_sanitizers_random_workload_and_evidence_mandatory() {
        let root = repo_root();
        let read = |path: &str| fs::read_to_string(root.join(path)).unwrap();
        let testbench = read("tb/integration/arm7tdmis_mister_wrapper_tb.sv");
        for marker in [
            "$value$plusargs(\"SEED=%d\"",
            "lfsr_seed",
            "[mister_wrapper] PASS seed=",
        ] {
            assert!(
                testbench.contains(marker),
                "missing workload marker: {marker}"
            );
        }
        let makefile = read("scripts/Makefile");
        for marker in [
            "SOAK_SEEDS ?= 256",
            "--x-assign unique",
            "--x-initial unique",
            "-fsanitize=address,undefined",
            "soak-build:",
            "soak:",
            "$(RANDOM_RUST_BIN)) soak",
        ] {
            assert!(
                makefile.contains(marker),
                "missing soak build marker: {marker}"
            );
        }
        let regression = read("verification/constrained_random_rs/src/regression.rs");
        assert!(regression.contains("\"soak\""));
        assert!(regression.contains("if !args.quick"));
        assert!(read(".github/workflows/verification.yml").contains("make -C scripts soak"));
        assert!(
            read("verification/constrained_random_rs/src/release_evidence/files/random.rs")
                .contains("soak-report.json")
        );
        let verification = read("docs/VERIFICATION.md");
        assert!(verification.contains("arm7tdmis-soak-v1"));
        assert!(verification.contains("make -C scripts soak"));
        let tasks = read("TASKS.md");
        let block = tasks
            .split("**VAL-010:**")
            .nth(1)
            .unwrap()
            .split("**VAL-011:**")
            .next()
            .unwrap();
        assert!(block.contains("soak.rs"));
        assert!(block.contains("256"));
    }

    #[test]
    fn seed_sequence_is_nonzero_unique_and_stable() {
        let first = seeds(0, 256).unwrap();
        assert_eq!(&first[..4], &[1, 39_022, 61_087, 20_196]);
        assert_eq!(first.iter().copied().collect::<BTreeSet<_>>().len(), 256);
        assert!(seeds(1, 65_536).is_err());
    }

    #[test]
    fn signatures_preserve_defect_class_across_address_changes() {
        assert_eq!(failure_signature(true, None, ""), "timeout");
        assert_eq!(
            failure_signature(
                false,
                Some(1),
                "ERROR: AddressSanitizer: heap-use-after-free"
            ),
            "sanitizer:address:heap-use-after-free"
        );
        let left = failure_signature(
            false,
            Some(1),
            "core.cc:12: runtime error: load of address 0x1234",
        );
        let right = failure_signature(
            false,
            Some(1),
            "core.cc:99: runtime error: load of address 0xabcd",
        );
        assert_eq!(left, right);
        assert_eq!(failure_signature(false, Some(-11), ""), "signal:11");
        assert_eq!(failure_signature(false, Some(9), ""), "exit-code:9");
    }

    #[cfg(unix)]
    #[test]
    fn minimization_keeps_the_same_failure_signature() {
        use std::os::unix::fs::PermissionsExt;
        let temporary = tempfile::tempdir().unwrap();
        let binary = temporary.path().join("seed script.sh");
        fs::write(&binary, "#!/bin/sh\nseed=${1#+SEED=}\nif [ $((seed & 1)) -eq 1 ]; then echo '[mister_wrapper] FAIL: same defect at 0x1234'; exit 9; fi\necho \"[mister_wrapper] PASS seed=$seed\"\n").unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
        let timeout = Duration::from_secs(2);
        let (passed, result, _) = execute(&binary, 0xf00f, timeout).unwrap();
        assert!(!passed);
        let signature = result["failure_signature"].as_str().unwrap();
        assert_eq!(
            minimize(&binary, 0xf00f, timeout, Some(signature)).unwrap(),
            1
        );
        assert!(!still_fails(&binary, 2, timeout, Some(signature)).unwrap());
        assert!(shell_command(&command(&binary, 1)).contains("'"));
    }

    #[test]
    fn failure_artifacts_keep_external_paths_and_exact_reproducer() {
        let temporary = tempfile::tempdir().unwrap();
        let binary = temporary.path().join("simulator");
        fs::write(&binary, b"binary").unwrap();
        let artifacts = failure_artifacts(
            &binary,
            0xffff,
            1,
            Duration::from_secs(2),
            "synthetic failure\n",
            &temporary.path().join("failures"),
            "exit-code:9",
        )
        .unwrap();
        let reproducer: Value =
            serde_json::from_slice(&fs::read(artifacts["reproducer"].as_str().unwrap()).unwrap())
                .unwrap();
        assert_eq!(reproducer["minimized_seed"], 1);
        assert_eq!(reproducer["failure_signature"], "exit-code:9");
        assert_eq!(reproducer["environment"]["ASAN_OPTIONS"], ASAN_OPTIONS);
        assert_eq!(
            fs::read_to_string(artifacts["log"].as_str().unwrap()).unwrap(),
            "synthetic failure\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn pass_marker_requires_the_whole_seed_line() {
        use std::os::unix::fs::PermissionsExt;
        let temporary = tempfile::tempdir().unwrap();
        let binary = temporary.path().join("wrong-seed.sh");
        fs::write(&binary, "#!/bin/sh\necho '[mister_wrapper] PASS seed=10'\n").unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
        let (passed, result, _) = execute(&binary, 1, Duration::from_secs(2)).unwrap();
        assert!(!passed);
        assert_eq!(result["pass_marker_found"], false);
    }
}
