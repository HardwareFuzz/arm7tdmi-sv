//! Independent QEMU ARM926 reference for the checked ARMv4T program.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::campaign::{repo_root, run_command, sha256, symbols, version, write_json, Failure};
use crate::qemu::{parse_states, CpuState};

const LINK_ADDRESS: u32 = 0x0001_0000;
const DATA_ADDRESS: u32 = 0x0002_0000;
const PSR_COMPARE_MASK: u32 = 0xf000_00ff;

fn optional(arguments: &[String], name: &str, default: String) -> Result<String, Failure> {
    if arguments.iter().any(|argument| argument == name) {
        crate::option(arguments, name).map_err(Failure::other)
    } else {
        Ok(default)
    }
}

fn differential_events(
    states: &[CpuState],
    begin: u32,
    end: u32,
) -> Result<Vec<(CpuState, CpuState)>, Failure> {
    let start = states
        .iter()
        .position(|state| state.pc() == begin)
        .ok_or_else(|| Failure::other(format!("QEMU never reached measure_begin {begin:#x}")))?;
    let mut events = Vec::new();
    for index in start..states.len().saturating_sub(1) {
        if states[index].pc() == end {
            break;
        }
        events.push((states[index].clone(), states[index + 1].clone()));
    }
    if states.get(start + events.len()).map(CpuState::pc) != Some(end) {
        return Err(Failure::other(format!(
            "QEMU never reached measure_end {end:#x}"
        )));
    }
    if events.len() < 50 {
        return Err(Failure::other(format!(
            "differential trace is unexpectedly short: {}",
            events.len()
        )));
    }
    Ok(events)
}

fn normalize_thumb_bl(
    events: Vec<(CpuState, CpuState)>,
    binary: &[u8],
) -> Vec<(CpuState, CpuState)> {
    let mut normalized = Vec::with_capacity(events.len());
    for (before, after) in events {
        if before.psr & (1 << 5) != 0 {
            if let Some(offset) = before
                .pc()
                .checked_sub(LINK_ADDRESS)
                .map(|value| value as usize)
            {
                if let Some(bytes) = binary.get(offset..offset.saturating_add(4)) {
                    if bytes.len() == 4 {
                        let prefix = u16::from_le_bytes([bytes[0], bytes[1]]);
                        let suffix = u16::from_le_bytes([bytes[2], bytes[3]]);
                        if prefix >> 11 == 0b11110 && suffix >> 11 == 0b11111 {
                            let high = (prefix & 0x7ff) as i32;
                            let signed_high = if high < 0x400 { high } else { high - 0x800 };
                            let mut intermediate = before.clone();
                            intermediate.registers[14] = before
                                .pc()
                                .wrapping_add(4)
                                .wrapping_add((signed_high << 12) as u32);
                            intermediate.registers[15] = before.pc().wrapping_add(2);
                            normalized.push((before, intermediate.clone()));
                            normalized.push((intermediate, after));
                            continue;
                        }
                    }
                }
            }
        }
        normalized.push((before, after));
    }
    normalized
}

fn packed_expected_line(before: &CpuState, after: &CpuState) -> String {
    let words = [
        before.pc(),
        u32::from(before.psr & (1 << 5) != 0),
        after.psr & PSR_COMPARE_MASK,
    ];
    words
        .into_iter()
        .chain(after.registers[..15].iter().copied())
        .map(|word| format!("{word:08x}"))
        .collect()
}

fn write_program_hex(binary: &[u8], path: &Path) -> Result<(), Failure> {
    let reset_branch = 0xea00_0000 | (((LINK_ADDRESS - 8) >> 2) & 0x00ff_ffff);
    let mut lines = vec![
        "@00000000".to_owned(),
        format!("{reset_branch:08x}"),
        format!("@{:08x}", LINK_ADDRESS / 4),
    ];
    for chunk in binary.chunks(4) {
        let mut bytes = [0_u8; 4];
        bytes[..chunk.len()].copy_from_slice(chunk);
        lines.push(format!("{:08x}", u32::from_le_bytes(bytes)));
    }
    fs::write(path, lines.join("\n") + "\n")?;
    Ok(())
}

pub fn reference(arguments: &[String]) -> Result<(), String> {
    let root = repo_root();
    let source = PathBuf::from(
        optional(
            arguments,
            "--source",
            root.join("verification/programs/qemu_diff.S")
                .to_string_lossy()
                .into_owned(),
        )
        .map_err(|error| error.to_string())?,
    );
    let source = fs::canonicalize(source).map_err(|error| error.to_string())?;
    let output = PathBuf::from(crate::option(arguments, "--output")?);
    let output = if output.is_absolute() {
        output
    } else {
        std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(output)
    };
    fs::create_dir_all(&output).map_err(|error| error.to_string())?;
    let clang =
        optional(arguments, "--clang", "clang".to_owned()).map_err(|error| error.to_string())?;
    let objcopy = optional(arguments, "--objcopy", "llvm-objcopy".to_owned())
        .map_err(|error| error.to_string())?;
    let nm =
        optional(arguments, "--nm", "llvm-nm".to_owned()).map_err(|error| error.to_string())?;
    let qemu = optional(arguments, "--qemu", "qemu-system-arm".to_owned())
        .map_err(|error| error.to_string())?;
    let fail = |error: Failure| error.to_string();

    let clang_version = version(&clang).map_err(fail)?;
    let qemu_version = version(&qemu).map_err(fail)?;
    if !qemu_version.starts_with("QEMU emulator version") {
        return Err(format!(
            "unexpected qemu-system-arm identity: {qemu_version}"
        ));
    }
    let elf = output.join("program.elf");
    let binary = output.join("program.bin");
    let trace = output.join("qemu.log");
    let expected = output.join("expected.hex");
    let program = output.join("program.hex");
    let generated = output.join("qemu_diff_generated.svh");
    let metadata_path = output.join("metadata.json");
    let compile_command = vec![
        clang.clone(),
        "--target=arm-none-eabi".into(),
        "-march=armv4t".into(),
        "-marm".into(),
        "-nostdlib".into(),
        "-fuse-ld=lld".into(),
        format!("-Wl,-Ttext={LINK_ADDRESS:#x}"),
        "-Wl,--entry=_start".into(),
        "-Wl,--build-id=none".into(),
        source.to_string_lossy().into_owned(),
        "-o".into(),
        elf.to_string_lossy().into_owned(),
    ];
    run_command(&compile_command, None).map_err(fail)?;
    run_command(
        &[
            objcopy.clone(),
            "-O".into(),
            "binary".into(),
            elf.to_string_lossy().into_owned(),
            binary.to_string_lossy().into_owned(),
        ],
        None,
    )
    .map_err(fail)?;
    let nm_output = run_command(
        &[
            nm.clone(),
            "--defined-only".into(),
            elf.to_string_lossy().into_owned(),
        ],
        None,
    )
    .map_err(fail)?;
    let symbol_map = symbols(&nm_output).map_err(fail)?;
    let qemu_command = vec![
        qemu,
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
        "-kernel".into(),
        binary.to_string_lossy().into_owned(),
        "-semihosting-config".into(),
        "enable=on,target=native".into(),
        "-d".into(),
        "cpu,nochain".into(),
        "-D".into(),
        trace.to_string_lossy().into_owned(),
    ];
    run_command(&qemu_command, Some(("QEMU_AUDIO_DRV", "none"))).map_err(fail)?;
    let states = parse_states(&fs::read_to_string(&trace).map_err(|error| error.to_string())?)?;
    let binary_contents = fs::read(&binary).map_err(|error| error.to_string())?;
    let events = differential_events(
        &states,
        symbol_map["measure_begin"] & !1,
        symbol_map["measure_end"] & !1,
    )
    .map_err(fail)?;
    let events = normalize_thumb_bl(events, &binary_contents);
    fs::write(
        &expected,
        events
            .iter()
            .map(|(before, after)| packed_expected_line(before, after))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )
    .map_err(|error| error.to_string())?;
    write_program_hex(&binary_contents, &program).map_err(fail)?;
    fs::write(&generated, format!("// Generated by arm7-constrained-random reference; do not edit.\nlocalparam int EXPECTED_EVENTS = {};\n", events.len())).map_err(|error| error.to_string())?;
    let symbol_strings = ["_start", "measure_begin", "measure_end"]
        .into_iter()
        .map(|name| (name.to_owned(), format!("0x{:08x}", symbol_map[name] & !1)))
        .collect::<std::collections::BTreeMap<_, _>>();
    let metadata = json!({
        "schema": "arm7tdmis-qemu-differential-v1",
        "reference": "QEMU ARM926, restricted to shared ARMv4T behavior",
        "normalization": {"psr_mask": format!("0x{PSR_COMPARE_MASK:08x}"), "excluded": [
            "ARMv5 instructions", "reserved and architecturally UNPREDICTABLE inputs",
            "implementation-defined reset register contents",
            "QEMU CPSR A bit and other non-ARM7 PSR extensions",
            "QEMU atomic Thumb BL log split into ARMv4T prefix/suffix",
        ]},
        "tools": {"clang": clang_version, "qemu": qemu_version, "objcopy": objcopy, "nm": nm},
        "symbols": symbol_strings, "events": events.len(),
        "data_address": format!("0x{DATA_ADDRESS:08x}"),
        "sha256": {"source": sha256(&source).map_err(fail)?, "elf": sha256(&elf).map_err(fail)?,
            "binary": sha256(&binary).map_err(fail)?, "program_hex": sha256(&program).map_err(fail)?,
            "expected_hex": sha256(&expected).map_err(fail)?, "qemu_log": sha256(&trace).map_err(fail)?},
        "commands": {"compile": compile_command, "qemu": qemu_command},
    });
    write_json(&metadata_path, &metadata).map_err(fail)?;
    println!(
        "[qemu-reference] PASS ({} events, {qemu_version})",
        events.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{normalize_thumb_bl, LINK_ADDRESS};
    use crate::qemu::CpuState;

    #[test]
    fn splits_allocated_thumb_bl_into_two_retirements() {
        let mut before = CpuState {
            registers: [0; 16],
            psr: 1 << 5,
        };
        before.registers[15] = LINK_ADDRESS;
        let mut after = before.clone();
        after.registers[15] = LINK_ADDRESS + 8;
        let events = normalize_thumb_bl(
            vec![(before.clone(), after.clone())],
            &[0x00, 0xf0, 0x00, 0xf8],
        );
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].1.pc(), LINK_ADDRESS + 2);
        assert_eq!(events[0].1.registers[14], LINK_ADDRESS + 4);
        assert_eq!(events[1].1.pc(), after.pc());
    }
}
