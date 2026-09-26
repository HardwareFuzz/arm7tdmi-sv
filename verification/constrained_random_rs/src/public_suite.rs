//! VAL-003 pinned public ARM/Thumb suite acquisition and execution.
//! The upstream ROM is never redistributed in Git; every downloaded byte and
//! every permitted patch is checked against the reviewed manifest.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Digest;
use wait_timeout::ChildExt;

use crate::campaign::{file_entry, repo_root, sha256, utc_now, write_json};

const SCHEMA: &str = "arm7tdmis-public-suite-v1";
const MANIFEST_SCHEMA: &str = "arm7tdmis-public-suites-v1";

#[derive(Clone, Deserialize, Serialize)]
struct Upstream {
    url: String,
    commit: String,
    tree: String,
    license_path: String,
    license_spdx: String,
    license_sha256: String,
}

#[derive(Clone, Deserialize, Serialize)]
struct WordPatch {
    offset: String,
    expected_word: String,
    replacement_word: String,
}

#[derive(Clone, Deserialize, Serialize)]
struct Suite {
    path: String,
    upstream_sha256: String,
    patched_sha256: String,
    idle_pc: String,
    result_register: u32,
    expected_vram_signature: String,
    minimum_retirements: u64,
    #[serde(default)]
    word_patches: Vec<WordPatch>,
}

#[derive(Deserialize)]
struct Manifest {
    schema: String,
    upstream: Upstream,
    patches: Value,
    suites: BTreeMap<String, Suite>,
}

#[derive(Debug)]
struct RunError {
    message: String,
    command: Option<Vec<String>>,
    output: Option<String>,
}

impl RunError {
    fn other(message: impl Into<String>) -> Self {
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

impl std::fmt::Display for RunError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(formatter)
    }
}

impl From<std::io::Error> for RunError {
    fn from(error: std::io::Error) -> Self {
        Self::other(error.to_string())
    }
}

fn err(message: impl Into<String>) -> RunError {
    RunError::other(message)
}

fn run_command(command: &[String], cwd: &Path) -> Result<String, RunError> {
    let scratch = tempfile::tempfile()?;
    let mut child = Command::new(&command[0])
        .args(&command[1..])
        .current_dir(cwd)
        .stdout(Stdio::from(scratch.try_clone()?))
        .stderr(Stdio::from(scratch.try_clone()?))
        .spawn()
        .map_err(|error| RunError::command(command, 127, error.to_string()))?;
    let status = child.wait_timeout(Duration::from_secs(180))?;
    let code = if let Some(status) = status {
        status.code().unwrap_or(1)
    } else {
        child.kill()?;
        child.wait()?;
        124
    };
    let mut scratch = scratch;
    scratch.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    scratch.read_to_end(&mut bytes)?;
    let output = String::from_utf8_lossy(&bytes).into_owned();
    if code != 0 {
        Err(RunError::command(command, code, output))
    } else {
        Ok(output)
    }
}

fn git_text(arguments: &[&str], cwd: &Path) -> Result<String, RunError> {
    let mut command = vec!["git".to_owned()];
    command.extend(arguments.iter().map(|item| (*item).to_owned()));
    Ok(run_command(&command, cwd)?.trim().to_owned())
}

fn absolute(path: PathBuf) -> Result<PathBuf, RunError> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn optional(arguments: &[String], name: &str, default: &str) -> Result<String, RunError> {
    if arguments.iter().any(|argument| argument == name) {
        crate::option(arguments, name).map_err(err)
    } else {
        Ok(default.to_owned())
    }
}

fn artifact_entry(path: &Path, root: &Path) -> Result<Value, RunError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|error| err(error.to_string()))?;
    Ok(json!({
        "path": relative.to_string_lossy(),
        "bytes": fs::metadata(path)?.len(),
        "sha256": sha256(path).map_err(|error| err(error.to_string()))?,
    }))
}

fn verify_checkout(checkout: &Path, manifest: &Manifest) -> Result<(), RunError> {
    let upstream = &manifest.upstream;
    if git_text(&["rev-parse", "HEAD"], checkout)? != upstream.commit {
        return Err(err("public-suite checkout has the wrong commit"));
    }
    if git_text(&["rev-parse", "HEAD^{tree}"], checkout)? != upstream.tree {
        return Err(err("public-suite checkout has the wrong tree"));
    }
    if !git_text(&["status", "--porcelain=v1"], checkout)?.is_empty() {
        return Err(err("public-suite checkout is dirty"));
    }
    if git_text(&["remote", "get-url", "origin"], checkout)? != upstream.url {
        return Err(err("public-suite checkout has the wrong origin"));
    }
    let license = checkout.join(&upstream.license_path);
    if sha256(&license).map_err(|error| err(error.to_string()))? != upstream.license_sha256 {
        return Err(err("public-suite license_sha256 mismatch"));
    }
    if !fs::read_to_string(&license)?.contains("MIT License") {
        return Err(err("public-suite license is not the recorded MIT text"));
    }
    for suite in manifest.suites.values() {
        if sha256(&checkout.join(&suite.path)).map_err(|error| err(error.to_string()))?
            != suite.upstream_sha256
        {
            return Err(err("public-suite upstream ROM hash mismatch"));
        }
    }
    Ok(())
}

fn acquire_checkout(
    cache_root: &Path,
    manifest: &Manifest,
) -> Result<(PathBuf, Vec<Vec<String>>), RunError> {
    let upstream = &manifest.upstream;
    let checkout = cache_root.join(format!("gba-suite-{}", upstream.commit));
    if checkout.is_dir() {
        if verify_checkout(&checkout, manifest).is_ok() {
            return Ok((checkout, Vec::new()));
        }
        fs::remove_dir_all(&checkout)?;
    }
    fs::create_dir_all(cache_root)?;
    let commands = vec![
        vec![
            "git".to_owned(),
            "init".to_owned(),
            checkout.to_string_lossy().into_owned(),
        ],
        vec![
            "git".to_owned(),
            "-C".to_owned(),
            checkout.to_string_lossy().into_owned(),
            "remote".to_owned(),
            "add".to_owned(),
            "origin".to_owned(),
            upstream.url.clone(),
        ],
        vec![
            "git".to_owned(),
            "-C".to_owned(),
            checkout.to_string_lossy().into_owned(),
            "fetch".to_owned(),
            "--depth=1".to_owned(),
            "origin".to_owned(),
            upstream.commit.clone(),
        ],
        vec![
            "git".to_owned(),
            "-C".to_owned(),
            checkout.to_string_lossy().into_owned(),
            "checkout".to_owned(),
            "--detach".to_owned(),
            "FETCH_HEAD".to_owned(),
        ],
    ];
    for command in &commands {
        run_command(command, &repo_root())?;
    }
    verify_checkout(&checkout, manifest)?;
    Ok((checkout, commands))
}

fn hex_u32(text: &str) -> Result<u32, RunError> {
    u32::from_str_radix(text.trim_start_matches("0x"), 16)
        .map_err(|_| err(format!("invalid 32-bit hexadecimal value: {text}")))
}

fn patched_rom(source: &Path, suite: &Suite) -> Result<Vec<u8>, RunError> {
    let mut contents = fs::read(source)?;
    if contents.len() < 0xc0 {
        return Err(err("public-suite ROM is smaller than its GBA header"));
    }
    let original = contents.clone();
    contents[4..0xc0].fill(0);
    let mut allowed = BTreeSet::new();
    for patch in &suite.word_patches {
        let offset =
            usize::try_from(hex_u32(&patch.offset)?).map_err(|error| err(error.to_string()))?;
        if offset < 0xc0
            || offset.checked_add(4).is_none_or(|end| end > contents.len())
            || offset % 4 != 0
        {
            return Err(err("public-suite word patch has an invalid offset"));
        }
        let expected = hex_u32(&patch.expected_word)?;
        let replacement = hex_u32(&patch.replacement_word)?;
        if u32::from_le_bytes(contents[offset..offset + 4].try_into().unwrap()) != expected {
            return Err(err("public-suite word patch source mismatch"));
        }
        contents[offset..offset + 4].copy_from_slice(&replacement.to_le_bytes());
        allowed.extend(offset..offset + 4);
    }
    for offset in 0xc0..contents.len() {
        if !allowed.contains(&offset) && contents[offset] != original[offset] {
            return Err(err("unlisted public-suite payload byte changed"));
        }
    }
    if format!("{:x}", sha2::Sha256::digest(&contents)) != suite.patched_sha256 {
        return Err(err("public-suite patched_sha256 mismatch"));
    }
    Ok(contents)
}

fn write_word_hex(contents: &[u8], path: &Path) -> Result<usize, RunError> {
    let mut text = String::with_capacity(contents.len() * 3);
    for word in contents.chunks(4) {
        let mut padded = [0_u8; 4];
        padded[..word.len()].copy_from_slice(word);
        text.push_str(&format!("{:08x}\n", u32::from_le_bytes(padded)));
    }
    fs::write(path, text)?;
    Ok(contents.len().div_ceil(4))
}

fn run_suite(
    name: &str,
    suite: &Suite,
    checkout: &Path,
    artifact_root: &Path,
    binary: &Path,
    discover_signatures: bool,
) -> Result<Value, RunError> {
    let suite_root = artifact_root.join(name);
    fs::create_dir_all(&suite_root)?;
    let rom_path = suite_root.join(format!("{name}.patched.gba"));
    let hex_path = suite_root.join(format!("{name}.hex"));
    let log_path = suite_root.join("simulation.log");
    let contents = patched_rom(&checkout.join(&suite.path), suite)?;
    fs::write(&rom_path, &contents)?;
    let rom_words = write_word_hex(&contents, &hex_path)?;
    let expected_signature = hex_u32(&suite.expected_vram_signature)?;
    if expected_signature == 0 && !discover_signatures {
        return Err(err(format!(
            "public-suite {name} has no frozen expected_vram_signature"
        )));
    }
    let idle_pc = hex_u32(&suite.idle_pc)?;
    let command = vec![
        binary.to_string_lossy().into_owned(),
        format!("+ROM_HEX={}", hex_path.display()),
        format!("+ROM_WORDS={rom_words}"),
        format!("+IDLE_PC={idle_pc:08x}"),
        format!("+RESULT_REGISTER={}", suite.result_register),
        format!("+EXPECTED_VRAM_SIGNATURE={expected_signature:08x}"),
        format!("+MINIMUM_RETIREMENTS={}", suite.minimum_retirements),
    ];
    let output = run_command(&command, &repo_root())?;
    fs::write(&log_path, &output)?;
    let pattern = Regex::new(r"\[public_suite\] PASS retirements=(\d+) arm=(\d+) thumb=(\d+) vram_signature=([0-9a-fA-F]{8}) idle_pc=([0-9a-fA-F]{8})").unwrap();
    let marker = pattern
        .captures(&output)
        .ok_or_else(|| err(format!("public-suite {name} omitted its PASS marker")))?;
    let retirements: u64 = marker[1]
        .parse()
        .map_err(|_| err("invalid retirement count"))?;
    let arm: u64 = marker[2]
        .parse()
        .map_err(|_| err("invalid ARM retirement count"))?;
    let thumb: u64 = marker[3]
        .parse()
        .map_err(|_| err("invalid Thumb retirement count"))?;
    let signature = hex_u32(&marker[4])?;
    if hex_u32(&marker[5])? != idle_pc {
        return Err(err(format!(
            "public-suite {name} reached the wrong idle PC"
        )));
    }
    if expected_signature != 0 && signature != expected_signature {
        return Err(err(format!("public-suite {name} VRAM signature mismatch")));
    }
    if retirements < suite.minimum_retirements {
        return Err(err(format!("public-suite {name} trace is too short")));
    }
    let artifacts: BTreeMap<_, _> = [&rom_path, &hex_path, &log_path]
        .into_iter()
        .map(|path| {
            Ok((
                path.file_name().unwrap().to_string_lossy().into_owned(),
                artifact_entry(path, artifact_root)?,
            ))
        })
        .collect::<Result<_, RunError>>()?;
    Ok(json!({
        "status": "passed",
        "upstream_path": suite.path,
        "upstream_sha256": suite.upstream_sha256,
        "patched_sha256": suite.patched_sha256,
        "idle_pc": suite.idle_pc,
        "result_register": suite.result_register,
        "expected_vram_signature": format!("0x{signature:08x}"),
        "word_patches": suite.word_patches,
        "metrics": {
            "retirements": retirements,
            "arm_retirements": arm,
            "thumb_retirements": thumb,
            "rom_words": rom_words,
        },
        "command": command,
        "reproducer": command,
        "artifacts": artifacts,
    }))
}

pub fn public_suite(arguments: &[String]) -> Result<(), String> {
    run(arguments).map_err(|error| error.to_string())
}

fn run(arguments: &[String]) -> Result<(), RunError> {
    let root = repo_root();
    let binary = fs::canonicalize(crate::option(arguments, "--binary").map_err(err)?)?;
    if !binary.is_file() {
        return Err(err("public-suite simulator binary is missing"));
    }
    let output = absolute(PathBuf::from(
        crate::option(arguments, "--output").map_err(err)?,
    ))?;
    let manifest_path = fs::canonicalize(optional(
        arguments,
        "--manifest",
        &root
            .join("verification/public_suites.json")
            .to_string_lossy(),
    )?)?;
    let manifest: Manifest = serde_json::from_slice(&fs::read(&manifest_path)?)
        .map_err(|error| err(error.to_string()))?;
    if manifest.schema != MANIFEST_SCHEMA {
        return Err(err("public-suite manifest has the wrong schema"));
    }
    if manifest
        .suites
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>()
        != ["arm", "thumb"]
    {
        return Err(err(
            "public-suite manifest must contain ARM and Thumb suites",
        ));
    }
    let cache_root = absolute(PathBuf::from(optional(
        arguments,
        "--cache-root",
        &root.join(".tools/public-suites").to_string_lossy(),
    )?))?;
    let discover_signatures = arguments
        .iter()
        .any(|argument| argument == "--discover-signatures");
    let artifact_root = output
        .parent()
        .ok_or_else(|| err("output has no parent"))?
        .join("public_suite");
    if artifact_root.is_dir() {
        fs::remove_dir_all(&artifact_root)?;
    }
    fs::create_dir_all(&artifact_root)?;
    let status = git_text(
        &["status", "--porcelain=v1", "--untracked-files=all"],
        &root,
    )?;
    let source = root.join("verification/constrained_random_rs/src/public_suite.rs");
    let mut inputs = serde_json::Map::new();
    for path in [
        &source,
        &manifest_path,
        &root.join("tb/integration/arm7tdmis_public_suite_tb.sv"),
    ] {
        let entry = file_entry(path).map_err(|error| err(error.to_string()))?;
        inputs.insert(entry["path"].as_str().unwrap().to_owned(), entry);
    }
    let mut report = json!({
        "schema": SCHEMA,
        "status": "running",
        "started_utc": utc_now(),
        "git": {
            "commit": git_text(&["rev-parse", "HEAD"], &root)?,
            "dirty": !status.is_empty(),
            "status": status.lines().collect::<Vec<_>>(),
        },
        "tools": {
            "rust": env!("CARGO_PKG_VERSION"),
            "git": git_text(&["--version"], &root)?,
            "simulator_binary_sha256": sha256(&binary).map_err(|error| err(error.to_string()))?,
        },
        "inputs": inputs,
        "upstream": manifest.upstream,
        "patches": manifest.patches,
        "suites": {},
    });
    write_json(&output, &report).map_err(|error| err(error.to_string()))?;
    let execution = (|| -> Result<(), RunError> {
        let (checkout, commands) = acquire_checkout(&cache_root, &manifest)?;
        let license_copy = artifact_root.join("upstream-LICENSE");
        fs::copy(
            checkout.join(&manifest.upstream.license_path),
            &license_copy,
        )?;
        report["upstream"]["acquisition_commands"] = json!(commands);
        report["upstream"]["license_artifact"] = artifact_entry(&license_copy, &artifact_root)?;
        for (name, suite) in &manifest.suites {
            println!("[public-suite] running {name}");
            report["suites"][name] = run_suite(
                name,
                suite,
                &checkout,
                &artifact_root,
                &binary,
                discover_signatures,
            )?;
            write_json(&output, &report).map_err(|error| err(error.to_string()))?;
        }
        Ok(())
    })();
    if let Err(error) = execution {
        report["status"] = json!("failed");
        report["finished_utc"] = json!(utc_now());
        report["failure"] = json!(error.to_string());
        if let Some(command) = &error.command {
            report["failure_command"] = json!(command);
        }
        if let Some(output_text) = &error.output {
            let start = output_text.floor_char_boundary(output_text.len().saturating_sub(20_000));
            report["failure_output"] = json!(&output_text[start..]);
        }
        write_json(&output, &report).map_err(|error| err(error.to_string()))?;
        eprintln!("[public-suite] FAIL: {error}");
        return Err(error);
    }
    report["status"] = json!("passed");
    report["finished_utc"] = json!(utc_now());
    report["suite_count"] = json!(manifest.suites.len());
    let retirements: u64 = report["suites"]
        .as_object()
        .unwrap()
        .values()
        .filter_map(|suite| suite["metrics"]["retirements"].as_u64())
        .sum();
    report["total_retirements"] = json!(retirements);
    write_json(&output, &report).map_err(|error| err(error.to_string()))?;
    println!(
        "[public-suite] PASS ({} suites, {retirements} retirements)",
        manifest.suites.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patching_uses_only_manifested_words_and_frozen_hash() {
        let root = repo_root();
        let manifest: Manifest = serde_json::from_slice(
            &fs::read(root.join("verification/public_suites.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest.schema, MANIFEST_SCHEMA);
        assert_eq!(manifest.suites.len(), 2);
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("tiny.gba");
        let mut rom = vec![0x5a; 0xc4];
        rom[0xc0..0xc4].copy_from_slice(&0x1234_5678_u32.to_le_bytes());
        fs::write(&source, &rom).unwrap();
        let mut suite = manifest.suites["arm"].clone();
        suite.word_patches = vec![WordPatch {
            offset: "0x000000c0".into(),
            expected_word: "0x12345678".into(),
            replacement_word: "0xea000000".into(),
        }];
        let mut expected = rom.clone();
        expected[4..0xc0].fill(0);
        expected[0xc0..0xc4].copy_from_slice(&0xea00_0000_u32.to_le_bytes());
        suite.patched_sha256 = format!("{:x}", sha2::Sha256::digest(&expected));
        assert_eq!(patched_rom(&source, &suite).unwrap(), expected);
        suite.word_patches[0].expected_word = "0xffffffff".into();
        assert!(patched_rom(&source, &suite).is_err());
    }
}
