//! Directed end-to-end contract for the supplied-program RTL runner.
//!
//! These cases use hand-encoded ARM/Thumb programs and expected observations;
//! they are runner tests, not an independent architectural oracle.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::time::Duration;

use serde_json::Value;
use wait_timeout::ChildExt;

use crate::campaign::Failure;

const RUN_TIMEOUT: Duration = Duration::from_secs(15);

macro_rules! require {
    ($condition:expr, $($argument:tt)+) => {
        if !($condition) {
            return Err(Failure::other(format!($($argument)+)));
        }
    };
}

#[derive(Clone, Copy)]
struct Options<'a> {
    stop_pc: Option<u32>,
    memory_base: u32,
    memory_length: u32,
    max_cycles: i64,
    thumb: &'a [u16],
    program_contents: Option<&'a str>,
}

impl Default for Options<'_> {
    fn default() -> Self {
        Self {
            stop_pc: None,
            memory_base: 0x200,
            memory_length: 16,
            max_cycles: 100_000,
            thumb: &[],
            program_contents: None,
        }
    }
}

struct Execution {
    status: ExitStatus,
    diagnostics: String,
    rows: Vec<Value>,
}

struct Completed {
    result: Value,
    events: Vec<Value>,
}

type ContractCase = (&'static str, fn(&Path) -> Result<(), Failure>);

fn branch(pc: u32, target: u32) -> u32 {
    0xea00_0000 | (((i64::from(target) - i64::from(pc) - 8) >> 2) as u32 & 0x00ff_ffff)
}

fn put_word(image: &mut [u8], offset: usize, word: u32) {
    image[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
}

fn program_image(instructions: &[u32], thumb: &[u16]) -> Vec<u8> {
    let mut image = vec![0_u8; 0x100 + instructions.len() * 4];
    for (vector, handler) in [(0, 0x100), (4, 0x80), (8, 0x84), (12, 0x8c), (16, 0x88)] {
        put_word(&mut image, vector, branch(vector as u32, handler));
    }
    for (address, word) in [
        (0x80, 0xe1b0_f00e),
        (0x84, 0xe1b0_f00e),
        (0x88, 0xe25e_f004),
        (0x8c, 0xe1a0_0000),
    ] {
        put_word(&mut image, address, word);
    }
    for (index, word) in instructions.iter().enumerate() {
        put_word(&mut image, 0x100 + index * 4, *word);
    }
    if !thumb.is_empty() {
        image.resize(image.len().max(0x200 + thumb.len() * 2), 0);
        for (index, halfword) in thumb.iter().enumerate() {
            let offset = 0x200 + index * 2;
            image[offset..offset + 2].copy_from_slice(&halfword.to_le_bytes());
        }
    }
    image.resize((image.len() + 3) & !3, 0);
    image
}

fn read_output(mut file: File) -> Result<String, Failure> {
    file.seek(SeekFrom::Start(0))?;
    let mut output = String::new();
    file.read_to_string(&mut output)?;
    Ok(output)
}

fn execute(
    binary: &Path,
    instructions: &[u32],
    options: Options<'_>,
) -> Result<Execution, Failure> {
    require!(
        !instructions.is_empty(),
        "standalone contract needs at least one instruction"
    );
    let temporary = tempfile::Builder::new()
        .prefix("arm7-standalone-test-")
        .tempdir()?;
    let program = temporary.path().join("program.hex");
    let result = temporary.path().join("result.jsonl");
    let image = program_image(instructions, options.thumb);
    let mut contents = String::new();
    for word in image.chunks_exact(4) {
        contents.push_str(&format!(
            "{:08x}\n",
            u32::from_le_bytes(word.try_into().unwrap())
        ));
    }
    fs::write(&program, options.program_contents.unwrap_or(&contents))?;

    let stop_pc = options
        .stop_pc
        .unwrap_or(0x100 + (instructions.len() as u32 - 1) * 4);
    let stdout_file = tempfile::tempfile()?;
    let stderr_file = tempfile::tempfile()?;
    let mut command = Command::new(binary);
    command
        .arg(format!("+PROGRAM_HEX={}", program.display()))
        .arg(format!("+RESULT_JSONL={}", result.display()))
        .arg(format!("+STOP_PC={stop_pc:x}"))
        .arg(format!("+MEMORY_BASE={:x}", options.memory_base))
        .arg(format!("+MEMORY_LENGTH={}", options.memory_length))
        .arg(format!("+MAX_CYCLES={}", options.max_cycles))
        .current_dir(temporary.path())
        .env("PATH", temporary.path().join("no-tools"))
        .stdout(Stdio::from(stdout_file.try_clone()?))
        .stderr(Stdio::from(stderr_file.try_clone()?));
    // The deliberately invalid inputs can trigger Verilator's abort path.
    // Disable core files without adding a shell or reference tool to PATH.
    unsafe {
        command.pre_exec(|| {
            let limit = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            if libc::setrlimit(libc::RLIMIT_CORE, &limit) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
    let mut child = command.spawn()?;
    let Some(status) = child.wait_timeout(RUN_TIMEOUT)? else {
        child.kill()?;
        child.wait()?;
        return Err(Failure::other("standalone RTL process exceeded 15 seconds"));
    };
    let diagnostics = format!("{}{}", read_output(stdout_file)?, read_output(stderr_file)?);
    let rows = if result.is_file() {
        fs::read_to_string(result)?
            .lines()
            .enumerate()
            .map(|(index, line)| {
                serde_json::from_str(line).map_err(|error| {
                    Failure::other(format!("invalid JSONL row {}: {error}", index + 1))
                })
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        Vec::new()
    };
    Ok(Execution {
        status,
        diagnostics,
        rows,
    })
}

fn completed(
    binary: &Path,
    instructions: &[u32],
    options: Options<'_>,
) -> Result<Completed, Failure> {
    let stop_pc = options
        .stop_pc
        .unwrap_or(0x100 + (instructions.len() as u32 - 1) * 4);
    let stop_is_thumb = !options.thumb.is_empty()
        && (0x200..0x200 + options.thumb.len() as u32 * 2).contains(&stop_pc);
    let expected_next_pc = stop_pc + if stop_is_thumb { 2 } else { 4 };
    let execution = execute(binary, instructions, options)?;
    require!(
        execution.status.success(),
        "standalone RTL failed unexpectedly: {}",
        execution.diagnostics
    );
    require!(
        execution.rows.len() >= 2,
        "completed run has no header and result"
    );
    let header = &execution.rows[0];
    let result = execution.rows.last().unwrap();
    let events = &execution.rows[1..execution.rows.len() - 1];
    require!(
        header["kind"] == "header",
        "first JSONL row is not a header"
    );
    require!(
        header["schema"] == "arm7tdmis-standalone-v1",
        "wrong standalone schema"
    );
    require!(
        header["backend"] == "arm7tdmi-sv",
        "wrong standalone backend"
    );
    require!(
        header["execution_backend"] == "arm7tdmi-sv",
        "wrong execution backend"
    );
    require!(
        header["reference_only"] == false,
        "runner is reference-only"
    );
    require!(result["kind"] == "result", "last JSONL row is not a result");
    require!(
        result["complete"] == true,
        "successful process has incomplete result"
    );
    require!(
        result["status"] == "completed",
        "wrong successful result status"
    );
    require!(
        result["last_retired_pc"] == format!("0x{stop_pc:08x}"),
        "successful result does not identify the STOP_PC retirement"
    );
    require!(
        result["next_pc"] == format!("0x{expected_next_pc:08x}")
            && result["next_pc_source"] == "sequential_completion_retirement",
        "successful result does not identify the sequential next-PC boundary"
    );
    require!(
        events.iter().all(|event| event["kind"] == "exception"),
        "non-exception row in event stream"
    );
    require!(
        result["exception_count"].as_u64() == Some(events.len() as u64),
        "exception count does not match event stream"
    );
    require!(
        result["exception_log_count"].as_u64() == Some(events.len() as u64),
        "exception log count does not match event stream"
    );
    require!(
        result["exception_dropped_count"] == 0,
        "exception events were dropped"
    );
    require!(
        events
            .iter()
            .enumerate()
            .all(|(index, event)| event["sequence"].as_u64() == Some(index as u64)),
        "event sequence is not contiguous from zero"
    );
    require!(result["registers"]["r15"].is_null(), "r15 was fabricated");
    require!(
        result["r15_availability"] == "unavailable",
        "wrong r15 availability"
    );
    let memory_hex = result["memory"]["bytes"]
        .as_str()
        .ok_or_else(|| Failure::other("memory bytes are missing"))?;
    let memory_length = result["memory"]["length"]
        .as_u64()
        .ok_or_else(|| Failure::other("memory length is missing"))?;
    require!(
        memory_hex.len() == memory_length as usize * 2
            && memory_hex.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "memory byte string length or encoding is wrong"
    );
    Ok(Completed {
        result: result.clone(),
        events: events.to_vec(),
    })
}

fn register(result: &Value, name: &str) -> Result<u32, Failure> {
    let value = result["registers"][name]
        .as_str()
        .ok_or_else(|| Failure::other(format!("missing {name}")))?;
    u32::from_str_radix(value.strip_prefix("0x").unwrap_or(value), 16)
        .map_err(|error| Failure::other(format!("invalid {name}: {error}")))
}

fn event_strings<'a>(events: &'a [Value], key: &str) -> Vec<Option<&'a str>> {
    events.iter().map(|event| event[key].as_str()).collect()
}

fn arithmetic_and_memory(binary: &Path) -> Result<(), Failure> {
    let completed = completed(
        binary,
        &[
            0xe3a0_0005,
            0xe3a0_1007,
            0xe080_2001,
            0xe3a0_3c02,
            0xe583_2000,
            0xe1a0_0000,
        ],
        Options::default(),
    )?;
    require!(
        completed.events.is_empty(),
        "arithmetic program caused an exception"
    );
    require!(
        register(&completed.result, "r2")? == 12,
        "r2 did not contain 5 + 7"
    );
    require!(
        completed.result["memory"]["bytes"] == format!("0c000000{}", "00".repeat(12)),
        "stored memory result is wrong"
    );
    Ok(())
}

fn synchronous_fault_recovery(binary: &Path) -> Result<(), Failure> {
    let completed = completed(
        binary,
        &[
            0xe3a0_0005,
            0xef00_0042,
            0xe7f0_00f0,
            0xee00_0010,
            0xe350_0000,
            0x07f0_00f0,
            0xe280_0001,
            0xe1a0_0000,
        ],
        Options::default(),
    )?;
    require!(
        event_strings(&completed.events, "pc")
            == [Some("0x00000104"), Some("0x00000108"), Some("0x0000010c")],
        "wrong synchronous fault PCs"
    );
    require!(
        completed
            .events
            .iter()
            .map(|event| event["raw_reason_code"].as_u64())
            .collect::<Vec<_>>()
            == [Some(2), Some(1), Some(1)],
        "wrong synchronous raw reason codes"
    );
    require!(
        event_strings(&completed.events, "cause")
            == [Some("svc"), Some("undefined"), Some("undefined")],
        "wrong synchronous causes"
    );
    require!(
        completed.events.iter().all(|event| {
            event["faulting_instruction"] == true
                && event["instruction_width"] == 4
                && event["thumb"] == false
        }),
        "synchronous fault lacks an explicit A32 faulting-instruction identity"
    );
    require!(
        event_strings(&completed.events, "instruction_bytes")
            == [Some("420000ef"), Some("f000f0e7"), Some("100000ee")],
        "wrong synchronous fault instruction bytes"
    );
    require!(
        register(&completed.result, "r0")? == 6,
        "execution did not resume after faults"
    );
    Ok(())
}

fn thumb_faults(binary: &Path) -> Result<(), Failure> {
    let completed = completed(
        binary,
        &[0xe59f_0000, 0xe12f_ff10, 0x0000_0201],
        Options {
            thumb: &[0x2009, 0xdf42, 0xde00, 0x3001, 0x46c0],
            stop_pc: Some(0x208),
            memory_base: 0x300,
            ..Options::default()
        },
    )?;
    require!(
        event_strings(&completed.events, "pc") == [Some("0x00000202"), Some("0x00000204")],
        "wrong Thumb fault PCs"
    );
    require!(
        completed
            .events
            .iter()
            .map(|event| event["instruction_width"].as_u64())
            .collect::<Vec<_>>()
            == [Some(2), Some(2)],
        "wrong Thumb instruction widths"
    );
    require!(
        event_strings(&completed.events, "instruction_bytes") == [Some("42df"), Some("00de")],
        "wrong Thumb instruction bytes"
    );
    require!(
        completed.events.iter().all(|event| event["thumb"] == true),
        "Thumb exceptions lost execution state"
    );
    require!(
        register(&completed.result, "r0")? == 10,
        "Thumb execution did not resume"
    );
    Ok(())
}

fn unmapped_store(binary: &Path) -> Result<(), Failure> {
    let completed = completed(
        binary,
        &[
            0xe3a0_0055,
            0xe3a0_3201,
            0xe583_0000,
            0xe280_0001,
            0xe1a0_0000,
        ],
        Options {
            memory_base: 0,
            memory_length: 4,
            ..Options::default()
        },
    )?;
    require!(
        event_strings(&completed.events, "pc") == [Some("0x00000108")]
            && event_strings(&completed.events, "cause") == [Some("data_abort")],
        "unmapped store did not cause the expected data abort"
    );
    require!(
        completed.events[0]["raw_reason_code"] == 4,
        "wrong data abort code"
    );
    require!(
        completed.events[0]["faulting_instruction"] == true
            && completed.events[0]["instruction_width"] == 4
            && completed.events[0]["thumb"] == false
            && completed.events[0]["instruction_bytes"] == "000083e5",
        "data abort lacks the observed A32 store identity"
    );
    let vector_bytes: String = branch(0, 0x100)
        .to_le_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    require!(
        completed.result["memory"]["bytes"] == vector_bytes,
        "unmapped store aliased low RAM"
    );
    require!(
        register(&completed.result, "r0")? == 0x56,
        "execution did not resume after abort"
    );
    Ok(())
}

fn unmapped_fetch(binary: &Path) -> Result<(), Failure> {
    let completed = completed(
        binary,
        &[0xe59f_f000, 0xe1a0_0000, 0x0004_0000],
        Options {
            stop_pc: Some(0x8c),
            ..Options::default()
        },
    )?;
    require!(
        event_strings(&completed.events, "pc") == [Some("0x00040000")]
            && event_strings(&completed.events, "cause") == [Some("prefetch_abort")],
        "unmapped fetch did not cause the expected prefetch abort"
    );
    require!(
        completed.events[0]["raw_reason_code"] == 3
            && completed.events[0]["faulting_instruction"] == true,
        "prefetch abort lacks its raw code or failed-fetch disposition"
    );
    require!(
        completed.events[0]["instruction_bytes"].is_null()
            && completed.events[0]["instruction_width"].is_null(),
        "unmapped fetch fabricated an instruction"
    );
    Ok(())
}

fn unbounded_event_stream(binary: &Path) -> Result<(), Failure> {
    let mut instructions = vec![0xef00_0042; 3_000];
    instructions.push(0xe1a0_0000);
    let completed = completed(binary, &instructions, Options::default())?;
    require!(
        completed.events.len() == 3_000,
        "event stream was truncated below 3000"
    );
    Ok(())
}

fn timeout_preserves_events(binary: &Path) -> Result<(), Failure> {
    let execution = execute(
        binary,
        &[0xef00_0042, branch(0x104, 0x100), 0xe1a0_0000],
        Options {
            max_cycles: 150,
            ..Options::default()
        },
    )?;
    require!(
        !execution.status.success(),
        "cycle-bound timeout exited successfully"
    );
    require!(
        execution.rows.len() >= 2,
        "timeout omitted JSONL header/result"
    );
    let result = execution.rows.last().unwrap();
    require!(
        result["complete"] == false,
        "timeout claimed complete execution"
    );
    require!(result["status"] == "timeout", "wrong timeout status");
    require!(
        result["exception_count"]
            .as_u64()
            .is_some_and(|count| count > 0),
        "timeout discarded observed exceptions"
    );
    require!(
        result["exception_count"].as_u64() == Some((execution.rows.len() - 2) as u64),
        "timeout event count does not match JSONL stream"
    );
    Ok(())
}

fn non_nop_stop_does_not_claim_completion(binary: &Path) -> Result<(), Failure> {
    let execution = execute(
        binary,
        &[branch(0x100, 0x100)],
        Options {
            stop_pc: Some(0x100),
            max_cycles: 200,
            ..Options::default()
        },
    )?;
    require!(
        !execution.status.success(),
        "branch at STOP_PC was treated as completion"
    );
    let result = execution
        .rows
        .last()
        .ok_or_else(|| Failure::other("branch stop case omitted its result"))?;
    require!(
        result["kind"] == "result" && result["complete"] == false,
        "branch at STOP_PC emitted a complete result"
    );
    require!(
        result["next_pc"].is_null() && result["next_pc_source"] == "unavailable",
        "incomplete branch stop case fabricated a next PC"
    );
    Ok(())
}

fn invalid_bounds_and_limits(binary: &Path) -> Result<(), Failure> {
    let cases = [
        Options {
            memory_base: 0x3ffff,
            memory_length: 2,
            ..Options::default()
        },
        Options {
            max_cycles: 0,
            ..Options::default()
        },
        Options {
            max_cycles: -1,
            ..Options::default()
        },
        Options {
            stop_pc: Some(0x101),
            ..Options::default()
        },
    ];
    for (index, options) in cases.into_iter().enumerate() {
        let execution = execute(binary, &[0xe1a0_0000], options)?;
        require!(
            !execution.status.success(),
            "invalid bounds/limit case {index} exited successfully"
        );
        require!(
            !execution.rows.iter().any(|row| row["complete"] == true),
            "invalid bounds/limit case {index} emitted a complete result"
        );
    }
    Ok(())
}

fn invalid_program_images(binary: &Path) -> Result<(), Failure> {
    for (index, contents) in ["", "not-hex\n", "@00010000\ne1a00000\n"]
        .into_iter()
        .enumerate()
    {
        let execution = execute(
            binary,
            &[0xe1a0_0000],
            Options {
                program_contents: Some(contents),
                ..Options::default()
            },
        )?;
        require!(
            !execution.status.success(),
            "invalid image case {index} exited successfully"
        );
        require!(
            !execution.rows.iter().any(|row| row["complete"] == true),
            "invalid image case {index} emitted a complete result"
        );
    }
    Ok(())
}

pub fn command(arguments: &[String]) -> Result<(), String> {
    run(arguments).map_err(|error| error.to_string())
}

fn run(arguments: &[String]) -> Result<(), Failure> {
    let binary =
        Path::new(&crate::option(arguments, "--binary").map_err(Failure::other)?).canonicalize()?;
    require!(binary.is_file(), "standalone RTL binary is missing");
    let cases: [ContractCase; 10] = [
        ("arithmetic_and_memory", arithmetic_and_memory),
        ("synchronous_fault_recovery", synchronous_fault_recovery),
        ("thumb_faults", thumb_faults),
        ("unmapped_store", unmapped_store),
        ("unmapped_fetch", unmapped_fetch),
        ("unbounded_event_stream", unbounded_event_stream),
        ("timeout_preserves_events", timeout_preserves_events),
        (
            "non_nop_stop_does_not_claim_completion",
            non_nop_stop_does_not_claim_completion,
        ),
        ("invalid_bounds_and_limits", invalid_bounds_and_limits),
        ("invalid_program_images", invalid_program_images),
    ];
    for (name, test) in cases {
        test(&binary).map_err(|error| Failure::other(format!("{name}: {error}")))?;
        println!("[standalone-test/{name}] PASS");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_image_places_vectors_handlers_arm_and_thumb_words_at_byte_addresses() {
        let image = program_image(&[0xe1a0_0000], &[0xdf42]);
        assert_eq!(&image[0..4], &branch(0, 0x100).to_le_bytes());
        assert_eq!(&image[0x80..0x84], &0xe1b0_f00e_u32.to_le_bytes());
        assert_eq!(&image[0x100..0x104], &0xe1a0_0000_u32.to_le_bytes());
        assert_eq!(&image[0x200..0x202], &0xdf42_u16.to_le_bytes());
        assert_eq!(image.len() % 4, 0);
    }
}
