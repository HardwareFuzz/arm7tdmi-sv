//! Build the generic SoC's ARMv4T ROM and prove every RTL word equals the
//! independently assembled binary, including the unused zero-filled tail.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;
use sha2::{Digest, Sha256};

use crate::campaign::{repo_root, run_command};

const ROM_BYTES: usize = 1024;
const EXPECTED_BYTES: usize = 812;
const EXPECTED_WORDS: usize = 203;
const EXPECTED_SHA256: &str = "ee9c06bc1720ce9e4f9f57da3e162acdfc0e42df403766f6d98bbeb9249c4900";
const REQUIRED_SYMBOLS: &[&str] = &[
    "_start",
    "reset",
    "pass",
    "fail",
    "irq_handler",
    "thumb_test",
];

fn program_source(root: &Path) -> PathBuf {
    root.join("examples/generic_soc/program.S")
}

fn linker_script(root: &Path) -> PathBuf {
    root.join("examples/generic_soc/linker.ld")
}

fn rtl_source(root: &Path) -> PathBuf {
    root.join("examples/generic_soc/arm7tdmi_generic_soc.sv")
}

fn tool(command: Vec<String>) -> Result<String, String> {
    run_command(&command, None).map_err(|error| error.to_string())
}

pub fn parse_rtl_words(text: &str) -> Result<BTreeMap<usize, u32>, String> {
    let function = Regex::new(r"(?s)function automatic logic \[31:0\] rom_word.*?endfunction")
        .unwrap()
        .find(text)
        .ok_or("RTL rom_word function is missing")?
        .as_str();
    let pattern =
        Regex::new(r"8'h(?P<index>[0-9a-fA-F]{2})\s*:\s*return\s+32'h(?P<word>[0-9a-fA-F_]+)\s*;")
            .unwrap();
    let mut words = BTreeMap::new();
    for capture in pattern.captures_iter(function) {
        let index =
            usize::from_str_radix(&capture["index"], 16).map_err(|error| error.to_string())?;
        let digits = capture["word"].replace('_', "");
        let word = u64::from_str_radix(&digits, 16).map_err(|error| error.to_string())?;
        if words.contains_key(&index) {
            return Err(format!("RTL ROM index 0x{index:02x} is duplicated"));
        }
        let word = u32::try_from(word)
            .map_err(|_| format!("RTL ROM index 0x{index:02x} exceeds 32 bits"))?;
        words.insert(index, word);
    }
    if words.is_empty() {
        return Err("RTL rom_word function contains no words".into());
    }
    if !function.contains("default: return 32'h0000_0000;") {
        return Err("RTL ROM default is not an explicit zero word".into());
    }
    Ok(words)
}

pub fn compare_rom(binary: &[u8], rtl_words: &BTreeMap<usize, u32>) -> Result<(), String> {
    if binary.len() > ROM_BYTES {
        return Err(format!(
            "compiled ROM is {} bytes; limit is {ROM_BYTES}",
            binary.len()
        ));
    }
    if !binary.len().is_multiple_of(4) {
        return Err("compiled ROM length is not word-aligned".into());
    }
    let mut mismatches = Vec::new();
    for index in 0..ROM_BYTES / 4 {
        let offset = index * 4;
        let compiled = if offset < binary.len() {
            u32::from_le_bytes(binary[offset..offset + 4].try_into().unwrap())
        } else {
            0
        };
        let rtl = rtl_words.get(&index).copied().unwrap_or(0);
        if compiled != rtl {
            mismatches.push(format!(
                "word 0x{index:02x}: compiled=0x{compiled:08x}, rtl=0x{rtl:08x}"
            ));
        }
    }
    let out_of_range = rtl_words
        .keys()
        .filter(|index| **index >= ROM_BYTES / 4)
        .map(|index| format!("0x{index:02x}"))
        .collect::<Vec<_>>();
    if !out_of_range.is_empty() {
        mismatches.push(format!(
            "RTL contains out-of-range word indices: {}",
            out_of_range.join(", ")
        ));
    }
    if mismatches.is_empty() {
        return Ok(());
    }
    let visible = mismatches
        .iter()
        .take(16)
        .cloned()
        .collect::<Vec<_>>()
        .join("\n  ");
    let remaining = mismatches.len().saturating_sub(16);
    let suffix = if remaining > 0 {
        format!("\n  ... {remaining} more")
    } else {
        String::new()
    };
    Err(format!("compiled/RTL ROM mismatch:\n  {visible}{suffix}"))
}

fn require_tools(toolchain: &Path) -> Result<Vec<PathBuf>, String> {
    let prefix = toolchain.join("bin");
    let tools = ["gcc", "objcopy", "objdump", "readelf", "nm"]
        .map(|name| prefix.join(format!("arm-none-eabi-{name}")))
        .to_vec();
    for executable in &tools {
        if !executable.is_file() {
            return Err(format!(
                "required tool is missing: {}",
                executable.display()
            ));
        }
    }
    Ok(tools)
}

fn has_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|candidate| candidate == needle)
}

pub struct Report {
    pub bytes: usize,
    pub words: usize,
    pub sha256: String,
}

pub fn build_and_check(toolchain: &Path, output: Option<&Path>) -> Result<Report, String> {
    let root = repo_root();
    let [gcc, objcopy, objdump, readelf, nm]: [PathBuf; 5] =
        require_tools(toolchain)?
            .try_into()
            .map_err(|_| "internal toolchain list error")?;
    let version = tool(vec![gcc.display().to_string(), "--version".into()])?
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned();
    if !version.contains("14.3.1") {
        return Err(format!("unexpected Arm GNU compiler version: {version}"));
    }
    let source = program_source(&root);
    let linker = linker_script(&root);
    let source_text = fs::read_to_string(&source).map_err(|error| error.to_string())?;
    if !source_text.contains(".arch armv4t") {
        return Err("program.S does not declare ARMv4T".into());
    }
    if Regex::new(r"(?i)\bblx\b").unwrap().is_match(&source_text) {
        return Err("ARMv5 BLX is forbidden in the ARMv4T ROM".into());
    }

    let temporary = tempfile::Builder::new()
        .prefix("arm7tdmis-rom-")
        .tempdir()
        .map_err(|error| error.to_string())?;
    let work = temporary.path();
    let object = work.join("program.o");
    let elf = work.join("program.elf");
    let binary_path = work.join("program.bin");
    let map = work.join("program.map");
    let disassembly_path = work.join("disassembly.txt");
    let attributes_path = work.join("attributes.txt");
    let display = |path: &Path| path.display().to_string();
    tool(vec![
        display(&gcc),
        "-march=armv4t".into(),
        "-marm".into(),
        "-mthumb-interwork".into(),
        "-x".into(),
        "assembler-with-cpp".into(),
        "-ffreestanding".into(),
        "-fno-pic".into(),
        "-fno-pie".into(),
        "-Wall".into(),
        "-Werror".into(),
        "-c".into(),
        display(&source),
        "-o".into(),
        display(&object),
    ])?;
    tool(vec![
        display(&gcc),
        "-march=armv4t".into(),
        "-marm".into(),
        "-mthumb-interwork".into(),
        "-nostdlib".into(),
        "-no-pie".into(),
        format!("-Wl,-T,{}", display(&linker)),
        format!("-Wl,-Map,{}", display(&map)),
        "-Wl,--build-id=none".into(),
        display(&object),
        "-o".into(),
        display(&elf),
    ])?;
    tool(vec![
        display(&objcopy),
        "-O".into(),
        "binary".into(),
        display(&elf),
        display(&binary_path),
    ])?;

    let attributes = tool(vec![display(&readelf), "-A".into(), display(&object)])?;
    if !attributes.contains("Tag_CPU_arch: v4T") {
        return Err("object attributes do not identify ARMv4T".into());
    }
    if !attributes.contains("Tag_THUMB_ISA_use: Thumb-1") {
        return Err("object attributes do not identify Thumb-1".into());
    }
    fs::write(&attributes_path, attributes).map_err(|error| error.to_string())?;

    let symbols = tool(vec![display(&nm), "-n".into(), display(&elf)])?;
    for symbol in REQUIRED_SYMBOLS {
        let pattern = Regex::new(&format!(r"(?m)\b{}$", regex::escape(symbol))).unwrap();
        if !pattern.is_match(&symbols) {
            return Err(format!("linked ROM is missing symbol {symbol}"));
        }
    }
    let disassembly = tool(vec![display(&objdump), "-d".into(), display(&elf)])?;
    for instruction in ["umull", "swp", "subs"] {
        let pattern = Regex::new(&format!(r"\b{instruction}\b")).unwrap();
        if !pattern.is_match(&disassembly) {
            return Err(format!(
                "linked ROM is missing required instruction {instruction}"
            ));
        }
    }
    let binary = fs::read(&binary_path).map_err(|error| error.to_string())?;
    if !has_bytes(&binary, &[0x11, 0xff, 0x2f, 0xe1]) {
        return Err("linked ROM is missing ARM-state BX r1".into());
    }
    if !has_bytes(&binary, &[0x70, 0x47]) {
        return Err("linked ROM is missing Thumb-state BX lr".into());
    }
    fs::write(&disassembly_path, disassembly).map_err(|error| error.to_string())?;

    let digest = format!("{:x}", Sha256::digest(&binary));
    if binary.len() != EXPECTED_BYTES {
        return Err(format!(
            "compiled ROM is {} bytes; expected {EXPECTED_BYTES}",
            binary.len()
        ));
    }
    if binary.len() / 4 != EXPECTED_WORDS {
        return Err(format!(
            "compiled ROM is {} words; expected {EXPECTED_WORDS}",
            binary.len() / 4
        ));
    }
    if digest != EXPECTED_SHA256 {
        return Err(format!(
            "compiled ROM SHA-256 is {digest}; expected {EXPECTED_SHA256}"
        ));
    }
    let rtl = fs::read_to_string(rtl_source(&root)).map_err(|error| error.to_string())?;
    compare_rom(&binary, &parse_rtl_words(&rtl)?)?;

    if let Some(output) = output {
        fs::create_dir_all(output).map_err(|error| error.to_string())?;
        for path in [
            &elf,
            &binary_path,
            &map,
            &disassembly_path,
            &attributes_path,
        ] {
            fs::copy(path, output.join(path.file_name().unwrap()))
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(Report {
        bytes: EXPECTED_BYTES,
        words: EXPECTED_WORDS,
        sha256: digest,
    })
}

pub fn check(arguments: &[String]) -> Result<String, String> {
    let root = repo_root();
    let mut toolchain = root.join(".tools/arm-gnu-toolchain-14.3.rel1-x86_64-arm-none-eabi");
    let mut output = None;
    let mut args = arguments.iter();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--toolchain" => {
                toolchain = PathBuf::from(args.next().ok_or("missing --toolchain value")?);
            }
            "--output" => {
                output = Some(PathBuf::from(args.next().ok_or("missing --output value")?));
            }
            other => return Err(format!("unknown generic-soc-rom-check argument: {other}")),
        }
    }
    let toolchain = if toolchain.is_absolute() {
        toolchain
    } else {
        std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(toolchain)
    };
    let report = build_and_check(&toolchain, output.as_deref())?;
    Ok(format!(
        "[generic-soc-rom] PASS ({} bytes, {} words, sha256={})",
        report.bytes, report.words, report.sha256
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtl_rom_indices_and_frozen_image_identity_are_declared() {
        let root = repo_root();
        let rtl = fs::read_to_string(rtl_source(&root)).unwrap();
        let words = parse_rtl_words(&rtl).unwrap();
        assert_eq!(words.get(&0), Some(&0xea00_000e));
        assert_eq!(words.get(&0xca), Some(&0x4641_494c));
        assert_eq!(EXPECTED_BYTES, 812);
        assert_eq!(EXPECTED_WORDS, 203);
        assert_eq!(
            EXPECTED_SHA256,
            "ee9c06bc1720ce9e4f9f57da3e162acdfc0e42df403766f6d98bbeb9249c4900"
        );
    }

    #[test]
    fn wrong_binary_cannot_match_rtl_rom() {
        let root = repo_root();
        let rtl = fs::read_to_string(rtl_source(&root)).unwrap();
        let words = parse_rtl_words(&rtl).unwrap();
        let error = compare_rom(&[0, 0, 0, 0], &words).unwrap_err();
        assert!(error.contains("compiled/RTL ROM mismatch"));
    }
}
