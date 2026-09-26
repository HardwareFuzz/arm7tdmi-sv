//! Build the pinned ARM/Thumb compiler regression and record its provenance.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::campaign::{repo_root, run_command, sha256};

const SCHEMA: &str = "arm7tdmis-compiler-program-v1";

fn tool(command: &[String]) -> Result<String, String> {
    run_command(command, None).map_err(|error| error.to_string())
}

fn digest(path: &Path) -> Result<String, String> {
    sha256(path).map_err(|error| error.to_string())
}

fn program_hex(binary: &Path, output: &Path) -> Result<(), String> {
    let bytes = fs::read(binary).map_err(|error| error.to_string())?;
    let mut text = String::new();
    for chunk in bytes.chunks(4) {
        let mut word = [0_u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        text.push_str(&format!("{:08x}\n", u32::from_le_bytes(word)));
    }
    if bytes.is_empty() {
        text.push('\n');
    }
    fs::write(output, text).map_err(|error| error.to_string())
}

fn build(toolchain: &Path, output: &Path) -> Result<(Value, String, u64), String> {
    let root = repo_root();
    let source_root = root.join("verification/compiler");
    let toolchain = fs::canonicalize(toolchain).map_err(|error| error.to_string())?;
    fs::create_dir_all(output).map_err(|error| error.to_string())?;
    let output = fs::canonicalize(output).map_err(|error| error.to_string())?;
    let prefix = toolchain.join("bin");
    let executable = |name: &str| {
        prefix
            .join(format!("arm-none-eabi-{name}"))
            .display()
            .to_string()
    };
    let gcc = executable("gcc");
    let objcopy = executable("objcopy");
    let objdump = executable("objdump");
    let readelf = executable("readelf");

    let startup = output.join("startup.o");
    let arm = output.join("arm_program.o");
    let thumb = output.join("thumb_program.o");
    let elf = output.join("compiler_program.elf");
    let binary = output.join("compiler_program.bin");
    let hex = output.join("program.hex");
    let disassembly = output.join("disassembly.txt");
    let attributes = output.join("attributes.txt");
    let map = output.join("compiler_program.map");

    let common = [
        "-march=armv4t",
        "-mthumb-interwork",
        "-ffreestanding",
        "-fno-builtin",
        "-fno-pic",
        "-fno-pie",
        "-fno-stack-protector",
        "-fno-unwind-tables",
        "-fno-asynchronous-unwind-tables",
        "-O2",
        "-Wall",
        "-Wextra",
        "-Werror",
    ];
    let mut commands = Vec::<Vec<String>>::new();
    for (mode, language, source, object) in [
        ("-marm", ["-x", "assembler-with-cpp"], "startup.S", &startup),
        ("-marm", ["-std=c11", ""], "arm_program.c", &arm),
        ("-mthumb", ["-std=c11", ""], "thumb_program.c", &thumb),
    ] {
        let mut command = vec![gcc.clone()];
        command.extend(common.iter().map(|flag| (*flag).to_owned()));
        command.push(mode.into());
        command.extend(
            language
                .into_iter()
                .filter(|flag| !flag.is_empty())
                .map(str::to_owned),
        );
        command.extend([
            "-c".into(),
            source_root.join(source).display().to_string(),
            "-o".into(),
            object.display().to_string(),
        ]);
        commands.push(command);
    }
    commands.push(vec![
        gcc.clone(),
        "-march=armv4t".into(),
        "-marm".into(),
        "-mthumb-interwork".into(),
        "-nostdlib".into(),
        "-no-pie".into(),
        format!("-Wl,-T,{}", source_root.join("linker.ld").display()),
        format!("-Wl,-Map,{}", map.display()),
        "-Wl,--build-id=none".into(),
        startup.display().to_string(),
        arm.display().to_string(),
        thumb.display().to_string(),
        "-o".into(),
        elf.display().to_string(),
    ]);
    commands.push(vec![
        objcopy,
        "-O".into(),
        "binary".into(),
        elf.display().to_string(),
        binary.display().to_string(),
    ]);
    for command in &commands {
        tool(command)?;
    }

    let disassembly_text = tool(&[objdump, "-d".into(), elf.display().to_string()])?;
    let attribute_text = tool(&[readelf, "-A".into(), elf.display().to_string()])?;
    for symbol in ["arm_main", "arm_mix", "thumb_accumulate", "thumb_store"] {
        if !disassembly_text.contains(&format!("<{symbol}>")) {
            return Err(format!("linked program is missing {symbol}"));
        }
    }
    if disassembly_text.to_ascii_lowercase().contains("blx") {
        return Err("ARMv5 BLX leaked into the ARMv4T program".into());
    }
    if !attribute_text.contains("Tag_CPU_arch: v4T") {
        return Err("ELF attributes do not identify ARMv4T".into());
    }
    fs::write(&disassembly, disassembly_text).map_err(|error| error.to_string())?;
    fs::write(&attributes, attribute_text).map_err(|error| error.to_string())?;
    program_hex(&binary, &hex)?;

    let version = tool(&[gcc, "--version".into()])?
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned();
    let mut sources = BTreeMap::new();
    for source in ["startup.S", "arm_program.c", "thumb_program.c", "linker.ld"] {
        sources.insert(source.to_owned(), digest(&source_root.join(source))?);
    }
    let metadata = json!({
        "schema": SCHEMA,
        "toolchain": {
            "version": version,
            "release": "14.3.Rel1",
            "install_metadata_sha256": digest(&toolchain.join("metadata.json"))?,
        },
        "flags": {
            "architecture": "-march=armv4t",
            "arm": "-marm",
            "thumb": "-mthumb",
            "interworking": "-mthumb-interwork",
            "environment": "-ffreestanding -nostdlib",
        },
        "commands": commands,
        "sha256": {
            "sources": sources,
            "elf": digest(&elf)?,
            "binary": digest(&binary)?,
            "program_hex": digest(&hex)?,
            "disassembly": digest(&disassembly)?,
            "attributes": digest(&attributes)?,
            "map": digest(&map)?,
        },
    });
    let mut serialized =
        serde_json::to_string_pretty(&metadata).map_err(|error| error.to_string())?;
    serialized.push('\n');
    fs::write(output.join("metadata.json"), serialized).map_err(|error| error.to_string())?;
    let size = fs::metadata(&binary)
        .map_err(|error| error.to_string())?
        .len();
    Ok((metadata, version, size))
}

pub fn command(arguments: &[String]) -> Result<String, String> {
    let mut toolchain = repo_root().join(".tools/arm-gnu-toolchain-14.3.rel1-x86_64-arm-none-eabi");
    let mut output = None;
    let mut args = arguments.iter();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--toolchain" => {
                toolchain = PathBuf::from(args.next().ok_or("missing --toolchain value")?)
            }
            "--output" => {
                output = Some(PathBuf::from(args.next().ok_or("missing --output value")?))
            }
            other => return Err(format!("unknown compiler-program-build argument: {other}")),
        }
    }
    let output = output.ok_or("missing --output")?;
    let (_, version, size) = build(&toolchain, &output)?;
    Ok(format!("[compiler-build] PASS ({version}, {size} bytes)"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_hex_uses_little_endian_words_and_pads_last_word() {
        let temporary = tempfile::tempdir().unwrap();
        let binary = temporary.path().join("program.bin");
        let hex = temporary.path().join("program.hex");
        fs::write(&binary, [0x11, 0x22, 0x33, 0x44, 0x55]).unwrap();
        program_hex(&binary, &hex).unwrap();
        assert_eq!(fs::read_to_string(hex).unwrap(), "44332211\n00000055\n");
    }
}
