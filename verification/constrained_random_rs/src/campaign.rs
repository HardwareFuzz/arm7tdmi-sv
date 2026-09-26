use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

use chrono::{SecondsFormat, Utc};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use wait_timeout::ChildExt;

use crate::{generation, qemu};

const SCHEMA: &str = "arm7tdmis-constrained-random-v1";
const MIN_RELEASE_SEEDS: usize = 32;
const MIN_RELEASE_INSTRUCTIONS: usize = 256;
const MIN_RELEASE_EVENTS: usize = 8_192;

#[derive(Debug)]
pub(crate) struct Failure {
    message: String,
    command: Option<Vec<String>>,
    output: Option<String>,
}

impl Failure {
    pub(crate) fn other(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            command: None,
            output: None,
        }
    }

    fn command(command: &[String], code: i32, output: String) -> Self {
        Self {
            message: format!("command exited {code}: {}\n{output}", command.join(" ")),
            command: Some(command.to_vec()),
            output: Some(output),
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(formatter)
    }
}

impl From<std::io::Error> for Failure {
    fn from(error: std::io::Error) -> Self {
        Self::other(error.to_string())
    }
}

#[derive(Clone)]
struct Args {
    differential_binary: PathBuf,
    policy_binary: PathBuf,
    output: PathBuf,
    seeds: usize,
    seed_base: i64,
    instructions: usize,
    clang: String,
    objcopy: String,
    nm: String,
    qemu: String,
}

impl Args {
    fn parse(arguments: &[String]) -> Result<Self, Failure> {
        fn required(arguments: &[String], name: &str) -> Result<String, Failure> {
            crate::option(arguments, name).map_err(Failure::other)
        }
        fn optional(arguments: &[String], name: &str, default: &str) -> Result<String, Failure> {
            if arguments.iter().any(|argument| argument == name) {
                required(arguments, name)
            } else {
                Ok(default.to_owned())
            }
        }
        let seeds = optional(arguments, "--seeds", "32")?
            .parse::<usize>()
            .map_err(|_| Failure::other("seeds must be positive"))?;
        let instructions = optional(arguments, "--instructions", "256")?
            .parse::<usize>()
            .map_err(|_| Failure::other("instructions must be at least 32"))?;
        let seed_base = optional(arguments, "--seed-base", "1")?
            .parse::<i64>()
            .map_err(|_| Failure::other("invalid seed base"))?;
        if seeds == 0 || instructions < 32 {
            return Err(Failure::other(
                "seeds must be positive and instructions at least 32",
            ));
        }
        let differential_binary = PathBuf::from(required(arguments, "--differential-binary")?);
        let policy_binary = PathBuf::from(required(arguments, "--policy-binary")?);
        if !differential_binary.is_file() {
            return Err(Failure::other("differential simulator binary is missing"));
        }
        if !policy_binary.is_file() {
            return Err(Failure::other("policy simulator binary is missing"));
        }
        Ok(Self {
            differential_binary: fs::canonicalize(differential_binary)?,
            policy_binary: fs::canonicalize(policy_binary)?,
            output: absolute(&PathBuf::from(required(arguments, "--output")?))?,
            seeds,
            seed_base,
            instructions,
            clang: optional(arguments, "--clang", "clang")?,
            objcopy: optional(arguments, "--objcopy", "llvm-objcopy")?,
            nm: optional(arguments, "--nm", "llvm-nm")?,
            qemu: optional(arguments, "--qemu", "qemu-system-arm")?,
        })
    }
}

fn absolute(path: &Path) -> Result<PathBuf, Failure> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

pub(crate) fn repo_root() -> PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        for start in [std::env::current_dir().ok(), std::env::current_exe().ok()]
            .into_iter()
            .flatten()
        {
            for ancestor in start.ancestors() {
                if ancestor
                    .join("verification/constrained_random_rs/Cargo.toml")
                    .is_file()
                {
                    return ancestor.to_path_buf();
                }
            }
        }
        // A binary copied outside the checkout still knows where it was built.
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf()
    })
    .clone()
}

pub(crate) fn utc_now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, false)
}

pub(crate) fn sha256(path: &Path) -> Result<String, Failure> {
    let bytes = fs::read(path)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

pub(crate) fn run_command(
    command: &[String],
    env: Option<(&str, &str)>,
) -> Result<String, Failure> {
    let scratch = tempfile::tempfile()?;
    let stdout = scratch.try_clone()?;
    let stderr = scratch.try_clone()?;
    let mut process = Command::new(&command[0]);
    process
        .args(&command[1..])
        .current_dir(repo_root())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    if let Some((name, value)) = env {
        process.env(name, value);
    }
    let mut child = process
        .spawn()
        .map_err(|error| Failure::command(command, 127, error.to_string()))?;
    let status = child.wait_timeout(Duration::from_secs(120))?;
    let (code, timed_out) = if let Some(status) = status {
        (status.code().unwrap_or(1), false)
    } else {
        child.kill()?;
        child.wait()?;
        (124, true)
    };
    let mut scratch = scratch;
    scratch.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    scratch.read_to_end(&mut bytes)?;
    let output = String::from_utf8_lossy(&bytes).into_owned();
    if timed_out || code != 0 {
        return Err(Failure::command(command, code, output));
    }
    Ok(output)
}

fn command(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_owned()).collect()
}

pub(crate) fn version(program: &str) -> Result<String, Failure> {
    Ok(run_command(&command(&[program, "--version"]), None)?
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned())
}

pub(crate) fn git_text(arguments: &[&str]) -> Result<String, Failure> {
    let mut command = vec!["git".to_owned()];
    command.extend(arguments.iter().map(|argument| (*argument).to_owned()));
    Ok(run_command(&command, None)?.trim().to_owned())
}

pub(crate) fn file_entry(path: &Path) -> Result<Value, Failure> {
    let root = repo_root();
    let relative = path
        .strip_prefix(&root)
        .map_err(|error| Failure::other(error.to_string()))?;
    Ok(
        json!({"path": relative.to_string_lossy(), "bytes": fs::metadata(path)?.len(), "sha256": sha256(path)?}),
    )
}

fn artifact_entry(path: &Path, root: &Path) -> Result<Value, Failure> {
    let relative = path
        .strip_prefix(root)
        .map_err(|error| Failure::other(error.to_string()))?;
    Ok(
        json!({"path": relative.to_string_lossy(), "bytes": fs::metadata(path)?.len(), "sha256": sha256(path)?}),
    )
}

pub(crate) fn write_json(path: &Path, value: &Value) -> Result<(), Failure> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!(
        "{}tmp",
        path.extension()
            .and_then(|ext| ext.to_str())
            .map_or(String::new(), |ext| format!("{ext}."))
    ));
    let mut file = File::create(&temporary)?;
    serde_json::to_writer_pretty(&mut file, value)
        .map_err(|error| Failure::other(error.to_string()))?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    fs::rename(temporary, path)?;
    Ok(())
}

pub(crate) fn symbols(output: &str) -> Result<BTreeMap<String, u32>, Failure> {
    let mut symbols = BTreeMap::new();
    for line in output.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() == 3 && fields[1].len() == 1 {
            if let Ok(address) = u32::from_str_radix(fields[0], 16) {
                symbols.insert(fields[2].to_owned(), address);
            }
        }
    }
    for required in ["_start", "measure_begin", "measure_end"] {
        if !symbols.contains_key(required) {
            return Err(Failure::other(format!(
                "linked image has no {required} symbol"
            )));
        }
    }
    Ok(symbols)
}

struct CompiledProgram {
    elf: PathBuf,
    binary: PathBuf,
    symbols: BTreeMap<String, u32>,
    command: Vec<String>,
}

fn compile_program(
    source: &Path,
    directory: &Path,
    args: &Args,
) -> Result<CompiledProgram, Failure> {
    let elf = directory.join("program.elf");
    let binary = directory.join("program.bin");
    let command = vec![
        args.clang.clone(),
        "--target=arm-none-eabi".into(),
        "-march=armv4t".into(),
        "-marm".into(),
        "-nostdlib".into(),
        "-fuse-ld=lld".into(),
        "-Wl,--image-base=0".into(),
        "-Wl,-Ttext=0".into(),
        "-Wl,--entry=_start".into(),
        "-Wl,--build-id=none".into(),
        source.to_string_lossy().into_owned(),
        "-o".into(),
        elf.to_string_lossy().into_owned(),
    ];
    run_command(&command, None)?;
    run_command(
        &[
            args.objcopy.clone(),
            "-O".into(),
            "binary".into(),
            elf.to_string_lossy().into_owned(),
            binary.to_string_lossy().into_owned(),
        ],
        None,
    )?;
    let nm_output = run_command(
        &[
            args.nm.clone(),
            "--defined-only".into(),
            elf.to_string_lossy().into_owned(),
        ],
        None,
    )?;
    Ok(CompiledProgram {
        elf,
        binary,
        symbols: symbols(&nm_output)?,
        command,
    })
}

fn artifact_map(paths: &[PathBuf], root: &Path) -> Result<Value, Failure> {
    let mut artifacts = serde_json::Map::new();
    for path in paths {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        artifacts.insert(name, artifact_entry(path, root)?);
    }
    Ok(Value::Object(artifacts))
}

fn qemu_expected(source: &Path, directory: &Path, args: &Args) -> Result<(usize, Value), Failure> {
    let CompiledProgram {
        elf,
        binary,
        symbols,
        command: compile_command,
    } = compile_program(source, directory, args)?;
    let trace = directory.join("qemu.log");
    let expected = directory.join("expected.hex");
    let program = directory.join("program.hex");
    let permitted_memory = directory.join("permitted_memory.hex");
    let qemu_command = vec![
        args.qemu.clone(),
        "-M".into(),
        "versatilepb".into(),
        "-cpu".into(),
        "arm926".into(),
        "-accel".into(),
        "tcg,one-insn-per-tb=on".into(),
        "-m".into(),
        "128M".into(),
        "-nographic".into(),
        "-monitor".into(),
        "none".into(),
        "-serial".into(),
        "none".into(),
        "-device".into(),
        format!(
            "loader,file={},addr=0,force-raw=on,cpu-num=0",
            binary.display()
        ),
        "-semihosting-config".into(),
        "enable=on,target=native".into(),
        "-d".into(),
        "cpu,nochain".into(),
        "-D".into(),
        trace.to_string_lossy().into_owned(),
    ];
    run_command(&qemu_command, Some(("QEMU_AUDIO_DRV", "none")))?;
    let begin = symbols["measure_begin"] & !1;
    let end = symbols["measure_end"] & !1;
    let event_count = qemu::expected(
        &trace,
        &binary,
        begin,
        end,
        &expected,
        &permitted_memory,
        &program,
    )
    .map_err(Failure::other)?
    .events;
    let artifacts = artifact_map(
        &[
            source.to_path_buf(),
            elf,
            binary,
            trace,
            expected,
            program,
            permitted_memory,
        ],
        directory,
    )?;
    Ok((
        event_count,
        json!({"compile_command": compile_command, "qemu_command": qemu_command, "artifacts": artifacts}),
    ))
}

fn build_policy_case(
    seed: i64,
    instructions: usize,
    big_endian: bool,
    directory: &Path,
    args: &Args,
) -> Result<Value, Failure> {
    fs::create_dir_all(directory)?;
    let source = directory.join("policy.S");
    let expected_path = directory.join("expected_memory.hex");
    let program_path = directory.join("program.hex");
    let generated = generation::policy(seed, instructions, big_endian);
    fs::write(&source, generated.source)?;
    fs::write(
        &expected_path,
        generated
            .expected
            .iter()
            .map(|word| format!("{word:08x}"))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )?;
    let CompiledProgram {
        elf,
        binary,
        command: compile_command,
        ..
    } = compile_program(&source, directory, args)?;
    qemu::write_program_hex(&binary, &program_path).map_err(Failure::other)?;
    let simulation_command = vec![
        args.policy_binary.to_string_lossy().into_owned(),
        format!("+PROGRAM_HEX={}", program_path.display()),
        format!("+EXPECTED_MEMORY_HEX={}", expected_path.display()),
        format!("+EXPECTED_WORDS={}", generated.result_words),
        "+DATA_WORDS=8".into(),
        format!("+BIG_ENDIAN={}", u8::from(big_endian)),
        format!("+SEED={seed}"),
    ];
    let output = run_command(&simulation_command, None)?;
    if !output.contains("[random_policy] PASS") {
        return Err(Failure::other("policy simulator omitted its PASS marker"));
    }
    let simulation_log = directory.join("simulation.log");
    fs::write(&simulation_log, output)?;
    Ok(json!({
        "status": "passed", "endian": if big_endian { "big" } else { "little" },
        "coverage": generated.coverage, "result_words": generated.result_words,
        "compile_command": compile_command, "simulation_command": simulation_command,
        "artifacts": artifact_map(&[source, elf, binary, program_path, expected_path, simulation_log], directory)?,
    }))
}

fn build_seed_case(
    seed: i64,
    instructions: usize,
    directory: &Path,
    args: &Args,
) -> Result<Value, Failure> {
    fs::create_dir_all(directory)?;
    let differential_directory = directory.join("differential");
    fs::create_dir(&differential_directory)?;
    let source = differential_directory.join("random.S");
    let generated = generation::differential(seed, instructions);
    fs::write(&source, generated.source)?;
    let (event_count, mut qemu_metadata) = qemu_expected(&source, &differential_directory, args)?;
    let simulation_command = vec![
        args.differential_binary.to_string_lossy().into_owned(),
        format!(
            "+PROGRAM_HEX={}",
            differential_directory.join("program.hex").display()
        ),
        format!(
            "+EXPECTED_HEX={}",
            differential_directory.join("expected.hex").display()
        ),
        format!("+EXPECTED_EVENTS={event_count}"),
        format!(
            "+PERMITTED_MEMORY_HEX={}",
            differential_directory
                .join("permitted_memory.hex")
                .display()
        ),
        format!("+SEED={seed}"),
    ];
    let output = run_command(&simulation_command, None)?;
    if !output.contains("[random_diff] PASS") {
        return Err(Failure::other(
            "differential simulator omitted its PASS marker",
        ));
    }
    let simulation_log = differential_directory.join("simulation.log");
    fs::write(&simulation_log, output)?;
    qemu_metadata["simulation_command"] = json!(simulation_command);
    qemu_metadata["artifacts"]["simulation.log"] =
        artifact_entry(&simulation_log, &differential_directory)?;
    let little = build_policy_case(
        seed,
        instructions,
        false,
        &directory.join("policy_little"),
        args,
    )?;
    let big = build_policy_case(
        seed,
        instructions,
        true,
        &directory.join("policy_big"),
        args,
    )?;
    let coverage: BTreeSet<String> = generated
        .coverage
        .into_iter()
        .chain(
            little["coverage"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned),
        )
        .chain(
            big["coverage"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned),
        )
        .collect();
    let reproducer = vec![
        "cargo".to_owned(),
        "run".into(),
        "--release".into(),
        "--locked".into(),
        "--manifest-path".into(),
        repo_root()
            .join("verification/constrained_random_rs/Cargo.toml")
            .to_string_lossy()
            .into_owned(),
        "--".into(),
        "campaign".into(),
        "--differential-binary".into(),
        args.differential_binary.to_string_lossy().into_owned(),
        "--policy-binary".into(),
        args.policy_binary.to_string_lossy().into_owned(),
        "--output".into(),
        args.output.to_string_lossy().into_owned(),
        "--seeds".into(),
        "1".into(),
        "--seed-base".into(),
        seed.to_string(),
        "--instructions".into(),
        instructions.to_string(),
        "--clang".into(),
        args.clang.clone(),
        "--objcopy".into(),
        args.objcopy.clone(),
        "--nm".into(),
        args.nm.clone(),
        "--qemu".into(),
        args.qemu.clone(),
    ];
    Ok(json!({
        "seed": seed, "status": "passed", "qemu_events": event_count,
        "coverage": coverage, "differential": qemu_metadata,
        "policy_profiles": {"little": little, "big": big}, "reproducer": reproducer,
    }))
}

fn required_coverage() -> BTreeSet<&'static str> {
    [
        "instruction",
        "arm",
        "thumb",
        "register",
        "flags",
        "mode",
        "banked_register",
        "memory",
        "load",
        "store",
        "exception",
        "svc",
        "undefined",
        "alignment",
        "aligned",
        "unaligned",
        "little",
        "big",
        "dependency",
        "permitted_memory",
    ]
    .into_iter()
    .collect()
}

fn initialize_report(args: &Args) -> Result<Value, Failure> {
    let root = repo_root();
    let status = git_text(&["status", "--porcelain=v1", "--untracked-files=all"])?;
    let release_grade =
        args.seeds >= MIN_RELEASE_SEEDS && args.instructions >= MIN_RELEASE_INSTRUCTIONS;
    let mut inputs = serde_json::Map::new();
    let crate_root = root.join("verification/constrained_random_rs");
    let mut paths = vec![crate_root.join("Cargo.toml"), crate_root.join("Cargo.lock")];
    for entry in fs::read_dir(crate_root.join("src"))? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "rs") {
            paths.push(path);
        }
    }
    paths.extend([
        root.join("tb/integration/arm7tdmis_random_diff_tb.sv"),
        root.join("tb/integration/arm7tdmis_random_policy_tb.sv"),
    ]);
    for path in paths {
        let entry = file_entry(&path)?;
        inputs.insert(entry["path"].as_str().unwrap().to_owned(), entry);
    }
    Ok(json!({
        "schema": SCHEMA, "status": "running", "started_utc": utc_now(),
        "git": {"commit": git_text(&["rev-parse", "HEAD"])?, "dirty": !status.is_empty(), "status": status.lines().collect::<Vec<_>>()},
        "configuration": {"seed_count": args.seeds, "seed_base": args.seed_base,
            "instructions_per_seed": args.instructions, "minimum_release_qemu_events": MIN_RELEASE_EVENTS,
            "release_grade": release_grade, "profiles": ["qemu-shared-armv4t", "little", "big"]},
        "tools": {"rust": version("rustc")?, "rust_engine_sha256": sha256(&std::env::current_exe()?)?,
            "clang": version(&args.clang)?, "qemu": version(&args.qemu)?,
            "differential_binary_sha256": sha256(&args.differential_binary)?,
            "policy_binary_sha256": sha256(&args.policy_binary)?},
        "inputs": inputs, "required_coverage": required_coverage(), "seeds": [],
    }))
}

fn seed_directory(seed: i64) -> String {
    if seed < 0 {
        format!("-{:07x}", seed.unsigned_abs())
    } else {
        format!("{seed:08x}")
    }
}

fn finish_failure(report: &mut Value, failure: &Failure) {
    report["status"] = json!("failed");
    report["finished_utc"] = json!(utc_now());
    report["failure"] = json!(failure.message);
    if let Some(command) = &failure.command {
        report["failure_command"] = json!(command);
    }
    if let Some(output) = &failure.output {
        report["failure_output"] = json!(output
            .chars()
            .rev()
            .take(20_000)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>());
    }
}

pub fn campaign(arguments: &[String]) -> Result<(), String> {
    let args = Args::parse(arguments).map_err(|error| error.to_string())?;
    let artifact_root = args.output.parent().unwrap().join("constrained_random");
    if artifact_root.is_dir() {
        fs::remove_dir_all(&artifact_root).map_err(|error| error.to_string())?;
    }
    fs::create_dir_all(&artifact_root).map_err(|error| error.to_string())?;
    let mut report = initialize_report(&args).map_err(|error| error.to_string())?;
    write_json(&args.output, &report).map_err(|error| error.to_string())?;
    for offset in 0..args.seeds {
        let seed = args
            .seed_base
            .checked_add(i64::try_from(offset).map_err(|error| error.to_string())?)
            .ok_or_else(|| "seed overflow".to_owned())?;
        println!(
            "[constrained-random] seed={seed} instructions={}",
            args.instructions
        );
        let directory = artifact_root.join(format!("seed-{}", seed_directory(seed)));
        match build_seed_case(seed, args.instructions, &directory, &args) {
            Ok(seed_result) => {
                report["seeds"].as_array_mut().unwrap().push(seed_result);
                write_json(&args.output, &report).map_err(|error| error.to_string())?;
            }
            Err(failure) => {
                finish_failure(&mut report, &failure);
                write_json(&args.output, &report).map_err(|error| error.to_string())?;
                return Err(format!("[constrained-random] FAIL: {failure}"));
            }
        }
    }
    let covered: BTreeSet<String> = report["seeds"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|seed| {
            seed["coverage"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
        })
        .collect();
    let missing: Vec<_> = required_coverage()
        .iter()
        .filter(|bin| !covered.contains(**bin))
        .copied()
        .collect();
    let total_events: usize = report["seeds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|seed| seed["qemu_events"].as_u64().unwrap() as usize)
        .sum();
    let release_grade =
        args.seeds >= MIN_RELEASE_SEEDS && args.instructions >= MIN_RELEASE_INSTRUCTIONS;
    let failure = if !missing.is_empty() {
        Some(format!("missing required coverage: {missing:?}"))
    } else if release_grade && total_events < MIN_RELEASE_EVENTS {
        Some(format!(
            "release campaign has only {total_events} QEMU events; requires {MIN_RELEASE_EVENTS}"
        ))
    } else {
        None
    };
    report["status"] = json!(if failure.is_some() {
        "failed"
    } else {
        "passed"
    });
    if let Some(ref failure) = failure {
        report["failure"] = json!(failure);
    }
    report["finished_utc"] = json!(utc_now());
    report["completed_seed_count"] = json!(args.seeds);
    report["total_qemu_events"] = json!(total_events);
    report["covered"] = json!(covered);
    write_json(&args.output, &report).map_err(|error| error.to_string())?;
    if let Some(failure) = failure {
        return Err(format!("[constrained-random] FAIL: {failure}"));
    }
    println!(
        "[constrained-random] PASS ({} seeds, {total_events} QEMU retirements)",
        args.seeds
    );
    Ok(())
}
