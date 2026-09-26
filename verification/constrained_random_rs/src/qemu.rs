use std::fs;
use std::path::Path;

use serde::Serialize;

const PSR_COMPARE_MASK: u32 = 0xf000_00ff;
const EXPECTED_EXCEPTION_NONE: u32 = 0xffff_ffff;

#[derive(Clone, Debug)]
pub(crate) struct CpuState {
    pub(crate) registers: [u32; 16],
    pub(crate) psr: u32,
}

impl CpuState {
    pub(crate) fn pc(&self) -> u32 {
        self.registers[15]
    }
}

fn register_line(line: &str) -> Option<[u32; 4]> {
    let mut values = [0_u32; 4];
    let mut parts = line.split(' ');
    for value in &mut values {
        let part = parts.next()?;
        // Match the original QEMU-log contract: four Rdd=XXXXXXXX tokens.
        let (name, hex) = part.split_once('=')?;
        if name.len() != 3
            || !name.starts_with('R')
            || !name[1..].bytes().all(|b| b.is_ascii_digit())
            || hex.len() != 8
        {
            return None;
        }
        *value = u32::from_str_radix(hex, 16).ok()?;
    }
    if parts.next().is_some() {
        return None;
    }
    Some(values)
}

pub(crate) fn parse_states(text: &str) -> Result<Vec<CpuState>, String> {
    let lines: Vec<_> = text.lines().collect();
    let mut states = Vec::new();
    let mut index = 0;
    while index + 4 < lines.len() {
        let Some(first) = register_line(lines[index]) else {
            index += 1;
            continue;
        };
        let mut registers = [0_u32; 16];
        registers[0..4].copy_from_slice(&first);
        for group in 1..4 {
            let Some(values) = register_line(lines[index + group]) else {
                return Err(format!(
                    "truncated QEMU register record at line {}",
                    index + 1
                ));
            };
            registers[(group * 4)..(group * 4 + 4)].copy_from_slice(&values);
        }
        let Some(psr_hex) = lines[index + 4]
            .strip_prefix("PSR=")
            .and_then(|part| part.get(..8))
        else {
            return Err(format!("missing QEMU PSR at line {}", index + 5));
        };
        let psr = u32::from_str_radix(psr_hex, 16)
            .map_err(|_| format!("missing QEMU PSR at line {}", index + 5))?;
        states.push(CpuState { registers, psr });
        index += 5;
    }
    if states.is_empty() {
        return Err("QEMU log contains no CPU states".to_owned());
    }
    Ok(states)
}

fn expected_exception(before_pc: u32, after_pc: u32) -> u32 {
    if before_pc >= 0x20 {
        match after_pc {
            4 => 1,
            8 => 2,
            _ => EXPECTED_EXCEPTION_NONE,
        }
    } else {
        EXPECTED_EXCEPTION_NONE
    }
}

fn write_word_hex(binary: &[u8]) -> String {
    let mut lines = vec!["@00000000".to_owned()];
    for chunk in binary.chunks(4) {
        let mut bytes = [0_u8; 4];
        bytes[..chunk.len()].copy_from_slice(chunk);
        lines.push(format!("{:08x}", u32::from_le_bytes(bytes)));
    }
    lines.join("\n") + "\n"
}

pub fn write_program_hex(binary: &Path, program: &Path) -> Result<(), String> {
    let binary_bytes = fs::read(binary).map_err(|error| error.to_string())?;
    fs::write(program, write_word_hex(&binary_bytes)).map_err(|error| error.to_string())
}

#[derive(Serialize)]
pub struct QemuResult {
    pub events: usize,
}

pub fn expected(
    trace: &Path,
    binary: &Path,
    begin: u32,
    end: u32,
    expected: &Path,
    permitted_memory: &Path,
    program: &Path,
) -> Result<QemuResult, String> {
    let log = fs::read_to_string(trace).map_err(|error| error.to_string())?;
    let states = parse_states(&log)?;
    let start = states
        .iter()
        .position(|state| state.pc() == begin)
        .ok_or_else(|| format!("QEMU never reached measure_begin {begin:#x}"))?;
    let mut expected_lines = Vec::new();
    let mut final_state = None;
    for index in start..states.len().saturating_sub(1) {
        let before = &states[index];
        if before.pc() == end {
            break;
        }
        let after = &states[index + 1];
        let mut words = vec![
            before.pc(),
            u32::from(before.psr & (1 << 5) != 0),
            after.psr & PSR_COMPARE_MASK,
            expected_exception(before.pc(), after.pc()),
        ];
        words.extend(&after.registers[..15]);
        expected_lines.push(
            words
                .iter()
                .map(|word| format!("{word:08x}"))
                .collect::<String>(),
        );
        final_state = Some(after);
    }
    if states.get(start + expected_lines.len()).map(CpuState::pc) != Some(end) {
        return Err(format!("QEMU never reached measure_end {end:#x}"));
    }
    if expected_lines.len() < 50 {
        return Err(format!(
            "differential trace is unexpectedly short: {}",
            expected_lines.len()
        ));
    }
    let final_state = final_state.expect("at least 50 events");
    fs::write(expected, expected_lines.join("\n") + "\n").map_err(|error| error.to_string())?;
    write_program_hex(binary, program)?;
    let memory = final_state.registers[8..13]
        .iter()
        .map(|value| format!("{value:08x}"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(permitted_memory, memory).map_err(|error| error.to_string())?;
    Ok(QemuResult {
        events: expected_lines.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::{expected_exception, parse_states};

    #[test]
    fn exception_vector_mapping() {
        assert_eq!(expected_exception(0x100, 4), 1);
        assert_eq!(expected_exception(0x100, 8), 2);
        assert_eq!(expected_exception(0x10, 8), u32::MAX);
    }

    #[test]
    fn rejects_missing_psr() {
        let sample = "R00=00000000 R01=00000000 R02=00000000 R03=00000000\nR04=00000000 R05=00000000 R06=00000000 R07=00000000\nR08=00000000 R09=00000000 R10=00000000 R11=00000000\nR12=00000000 R13=00000000 R14=00000000 R15=00000020\nwrong\n";
        assert!(parse_states(sample)
            .unwrap_err()
            .contains("missing QEMU PSR"));
    }
}
