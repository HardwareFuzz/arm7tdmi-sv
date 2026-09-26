//! Fail-closed architectural-control mutation gate.
//!
//! A mutant counts as killed only when its designated architectural test
//! emits the expected diagnostic. Compile failures and unrelated fatal exits
//! do not count as mutation coverage.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use wait_timeout::ChildExt;

#[derive(Clone, Copy, Debug)]
pub struct Mutation {
    pub name: &'static str,
    pub command: &'static [&'static str],
    pub expected_diagnostic: &'static str,
}

pub const MUTATIONS: &[Mutation] = &[
    Mutation {
        name: "writeback",
        command: &[
            "make",
            "integ-sequence_dependencies",
            "INTEG_EXTRA_FLAGS_sequence_dependencies=-DARM7TDMIS_MUTATE_WRITEBACK",
        ],
        expected_diagnostic: "[sequence_dependencies] FAIL",
    },
    Mutation {
        name: "flags",
        command: &[
            "make",
            "integ-flags_preserve",
            "INTEG_EXTRA_FLAGS_flags_preserve=-DARM7TDMIS_MUTATE_FLAGS",
        ],
        expected_diagnostic: "[flags_preserve] FAIL",
    },
    Mutation {
        name: "exception",
        command: &[
            "make",
            "integ-arm_exception_lr",
            "INTEG_EXTRA_FLAGS_arm_exception_lr=-DARM7TDMIS_MUTATE_EXCEPTION",
        ],
        expected_diagnostic: "[arm_exception_lr/SWI] FAIL",
    },
    Mutation {
        name: "bus",
        command: &[
            "make",
            "integ-fetch_sequence",
            "INTEG_EXTRA_FLAGS_fetch_sequence=-DARM7TDMIS_MUTATE_BUS",
        ],
        expected_diagnostic: "CPnMREQ does not mirror TRANS active",
    },
];

#[derive(Debug)]
pub struct Observation {
    pub returncode: i32,
    pub output: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct MutationResult {
    pub name: &'static str,
    pub returncode: i32,
    pub expected_diagnostic: &'static str,
}

pub fn run_mutations<F>(
    mutations: &[Mutation],
    mut runner: F,
) -> Result<Vec<MutationResult>, String>
where
    F: FnMut(&[&str]) -> Result<Observation, String>,
{
    let mut results = Vec::with_capacity(mutations.len());
    for mutation in mutations {
        let observation = runner(mutation.command)?;
        if observation.returncode == 0 {
            return Err(format!(
                "{} mutant survived: {}",
                mutation.name,
                mutation.command.join(" ")
            ));
        }
        if !observation.output.contains(mutation.expected_diagnostic) {
            let lines: Vec<_> = observation.output.lines().collect();
            let tail = lines[lines.len().saturating_sub(20)..].join("\n");
            return Err(format!(
                "{} mutant reached wrong detector; expected {:?}\n{}",
                mutation.name, mutation.expected_diagnostic, tail
            ));
        }
        results.push(MutationResult {
            name: mutation.name,
            returncode: observation.returncode,
            expected_diagnostic: mutation.expected_diagnostic,
        });
    }
    Ok(results)
}

fn kill_group(pid: u32) {
    // The child is started with process_group(0), so its PID is the PGID.
    unsafe {
        libc::killpg(pid as libc::pid_t, libc::SIGKILL);
    }
}

fn run_command(
    command: &[&str],
    scripts_dir: &Path,
    timeout: Duration,
) -> Result<Observation, String> {
    let mut log: File = tempfile::tempfile().map_err(|error| error.to_string())?;
    let stdout = log.try_clone().map_err(|error| error.to_string())?;
    let stderr = log.try_clone().map_err(|error| error.to_string())?;
    let mut child = Command::new(command[0])
        .args(&command[1..])
        .current_dir(scripts_dir)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|error| format!("cannot run {}: {error}", command.join(" ")))?;
    let status = match child
        .wait_timeout(timeout)
        .map_err(|error| error.to_string())?
    {
        Some(status) => status,
        None => {
            kill_group(child.id());
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "mutation command timed out after {}s: {}",
                timeout.as_secs(),
                command.join(" ")
            ));
        }
    };
    log.seek(SeekFrom::Start(0))
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    log.read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    let returncode = status
        .code()
        .or_else(|| status.signal().map(|signal| -signal))
        .unwrap_or(-1);
    Ok(Observation {
        returncode,
        output: String::from_utf8_lossy(&bytes).into_owned(),
    })
}

pub fn mutation_self_test(arguments: &[String]) -> Result<String, String> {
    let mut scripts_dir = std::path::PathBuf::from(".");
    let mut timeout_seconds = 300u64;
    let mut args = arguments.iter();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--scripts-dir" => {
                scripts_dir = args.next().ok_or("missing --scripts-dir value")?.into();
            }
            "--timeout-seconds" => {
                timeout_seconds = args
                    .next()
                    .ok_or("missing --timeout-seconds value")?
                    .parse()
                    .map_err(|_| "invalid --timeout-seconds value")?;
            }
            other => return Err(format!("unknown mutation-self-test argument: {other}")),
        }
    }
    if timeout_seconds == 0 {
        return Err("--timeout-seconds must be positive".into());
    }
    if !scripts_dir.join("Makefile").is_file() {
        return Err(format!("{} has no Makefile", scripts_dir.display()));
    }
    let results = run_mutations(MUTATIONS, |command| {
        run_command(command, &scripts_dir, Duration::from_secs(timeout_seconds))
    })?;
    let mut lines = results
        .iter()
        .map(|result| {
            format!(
                "[mutation/{}] PASS: killed by {}",
                result.name, result.expected_diagnostic
            )
        })
        .collect::<Vec<_>>();
    lines.push(format!(
        "[mutation] PASS: killed {}/{} mutants",
        results.len(),
        MUTATIONS.len()
    ));
    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_covers_required_control_classes() {
        assert_eq!(
            MUTATIONS
                .iter()
                .map(|mutation| mutation.name)
                .collect::<Vec<_>>(),
            ["writeback", "flags", "exception", "bus"]
        );
        for mutation in MUTATIONS {
            assert!(!mutation.expected_diagnostic.is_empty());
            assert!(mutation.command.join(" ").contains("-DARM7TDMIS_MUTATE_"));
        }
    }

    #[test]
    fn intended_diagnostics_kill_every_mutant() {
        let results = run_mutations(MUTATIONS, |command| {
            let mutation = MUTATIONS
                .iter()
                .find(|mutation| mutation.command == command)
                .unwrap();
            Ok(Observation {
                returncode: 1,
                output: mutation.expected_diagnostic.into(),
            })
        })
        .unwrap();
        assert_eq!(results.len(), MUTATIONS.len());
        assert!(results.iter().all(|result| result.returncode != 0));
    }

    #[test]
    fn survivor_and_wrong_detector_fail_closed() {
        let survivor = run_mutations(&MUTATIONS[..1], |_| {
            Ok(Observation {
                returncode: 0,
                output: "PASS".into(),
            })
        })
        .unwrap_err();
        assert!(survivor.contains("survived"));
        let wrong = run_mutations(&MUTATIONS[..1], |_| {
            Ok(Observation {
                returncode: 2,
                output: "syntax error".into(),
            })
        })
        .unwrap_err();
        assert!(wrong.contains("wrong detector"));
    }
}
