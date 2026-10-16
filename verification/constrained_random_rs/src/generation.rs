use std::collections::BTreeSet;

use serde::Serialize;

use crate::random::PythonRandom;

const MODE_USER: u8 = 0x10;
const MODE_FIQ: u8 = 0x11;
const MODE_IRQ: u8 = 0x12;
const MODE_SUPERVISOR: u8 = 0x13;
const MODE_ABORT: u8 = 0x17;
const MODE_UNDEFINED: u8 = 0x1b;
const MODE_SYSTEM: u8 = 0x1f;

#[derive(Serialize)]
pub struct Differential {
    pub source: String,
    pub coverage: Vec<String>,
}

#[derive(Serialize)]
pub struct Policy {
    pub source: String,
    pub expected: Vec<u32>,
    pub result_words: usize,
    pub coverage: Vec<String>,
}

fn mark(coverage: &mut BTreeSet<&'static str>, bins: &[&'static str]) {
    coverage.extend(bins.iter().copied());
}

fn arm_instruction(
    rng: &mut PythonRandom,
    index: usize,
    previous: usize,
    lines: &mut Vec<String>,
    coverage: &mut BTreeSet<&'static str>,
) -> usize {
    const REGISTERS: [usize; 10] = [0, 1, 2, 3, 4, 5, 8, 9, 11, 12];
    let destination = rng.choice(&REGISTERS);
    let source = if index.is_multiple_of(3) {
        previous
    } else {
        rng.choice(&REGISTERS)
    };
    let other = rng.choice(&REGISTERS);
    let immediate = rng.range(256);
    let operation = rng.range(8);
    mark(
        coverage,
        &[
            "arm",
            if source == previous {
                "dependency"
            } else {
                "register"
            },
        ],
    );
    match operation {
        0 => {
            lines.push(format!("    adds r{destination}, r{source}, #{immediate}"));
            mark(coverage, &["data_processing", "flags"]);
        }
        1 => {
            lines.push(format!("    subs r{destination}, r{source}, #{immediate}"));
            mark(coverage, &["data_processing", "flags"]);
        }
        2 => {
            lines.push(format!("    eor r{destination}, r{source}, r{other}"));
            mark(coverage, &["data_processing"]);
        }
        3 => {
            lines.push(format!("    orr r{destination}, r{source}, r{other}"));
            mark(coverage, &["data_processing"]);
        }
        4 => {
            let shift = 1 + rng.range(31);
            lines.push(format!("    movs r{destination}, r{source}, lsl #{shift}"));
            mark(coverage, &["shift", "flags"]);
        }
        5 => {
            let alternatives: Vec<_> = REGISTERS
                .iter()
                .copied()
                .filter(|r| *r != destination)
                .collect();
            let multiplier = rng.choice(&alternatives);
            lines.push(format!("    mul r{destination}, r{multiplier}, r{other}"));
            mark(coverage, &["multiply"]);
        }
        6 => {
            let offset = 4 * rng.range(5);
            if rng.range(2) != 0 {
                lines.push(format!("    str r{source}, [r10, #{offset}]"));
                mark(coverage, &["memory", "store", "aligned"]);
                return source;
            }
            lines.push(format!("    ldr r{destination}, [r10, #{offset}]"));
            mark(coverage, &["memory", "load", "aligned"]);
        }
        _ => {
            let condition = if rng.range(2) != 0 { "eq" } else { "ne" };
            lines.push(format!("    cmp r{source}, r{other}"));
            lines.push(format!(
                "    add{condition} r{destination}, r{source}, #{immediate}"
            ));
            lines.push(format!("arm_condition_{index}:"));
            mark(coverage, &["condition", "flags"]);
        }
    }
    destination
}

fn thumb_instruction(
    rng: &mut PythonRandom,
    index: usize,
    previous: usize,
    lines: &mut Vec<String>,
    coverage: &mut BTreeSet<&'static str>,
) -> usize {
    const REGISTERS: [usize; 6] = [0, 1, 2, 3, 4, 5];
    let destination = rng.choice(&REGISTERS);
    let source = if previous < 6 && index.is_multiple_of(3) {
        previous
    } else {
        rng.choice(&REGISTERS)
    };
    let other = rng.choice(&REGISTERS);
    let immediate = rng.range(8);
    let operation = rng.range(9);
    mark(
        coverage,
        &[
            "thumb",
            if source == previous {
                "dependency"
            } else {
                "register"
            },
        ],
    );
    match operation {
        0 => {
            lines.push(format!("    adds r{destination}, r{source}, #{immediate}"));
            mark(coverage, &["data_processing", "flags"]);
        }
        1 => {
            lines.push(format!("    subs r{destination}, r{source}, #{immediate}"));
            mark(coverage, &["data_processing", "flags"]);
        }
        2 => {
            lines.push(format!("    eors r{destination}, r{other}"));
            mark(coverage, &["data_processing", "flags"]);
        }
        3 => {
            lines.push(format!("    orrs r{destination}, r{other}"));
            mark(coverage, &["data_processing", "flags"]);
        }
        4 => {
            let shift = 1 + rng.range(31);
            lines.push(format!("    lsls r{destination}, r{source}, #{shift}"));
            mark(coverage, &["shift", "flags"]);
        }
        5 => {
            lines.push(format!("    muls r{destination}, r{other}"));
            mark(coverage, &["multiply", "flags"]);
        }
        6 => {
            let offset = 4 * rng.range(5);
            if rng.range(2) != 0 {
                lines.push(format!("    str r{source}, [r6, #{offset}]"));
                mark(coverage, &["memory", "store", "aligned"]);
                return source;
            }
            lines.push(format!("    ldr r{destination}, [r6, #{offset}]"));
            mark(coverage, &["memory", "load", "aligned"]);
        }
        7 => {
            let offset = rng.range(20);
            if rng.range(2) != 0 {
                lines.push(format!("    strb r{source}, [r6, #{offset}]"));
                mark(coverage, &["memory", "store", "byte"]);
                return source;
            }
            lines.push(format!("    ldrb r{destination}, [r6, #{offset}]"));
            mark(coverage, &["memory", "load", "byte"]);
        }
        _ => {
            lines.push(format!("    cmp r{source}, r{other}"));
            lines.push(format!("    bne thumb_condition_{index}"));
            lines.push(format!("    adds r{destination}, #{immediate}"));
            lines.push(format!("thumb_condition_{index}:"));
            mark(coverage, &["condition", "flags"]);
        }
    }
    destination
}

pub fn differential(seed: i64, instruction_count: usize) -> Differential {
    let mut rng = PythonRandom::new(seed);
    let mut coverage: BTreeSet<_> = [
        "instruction",
        "register",
        "flags",
        "memory",
        "alignment",
        "little",
    ]
    .into_iter()
    .collect();
    let arm_count = instruction_count / 2;
    let thumb_count = instruction_count - arm_count;
    let mut arm_lines = Vec::new();
    let mut thumb_lines = Vec::new();
    let mut previous = 0;
    for index in 0..arm_count {
        previous = arm_instruction(&mut rng, index, previous, &mut arm_lines, &mut coverage);
    }
    previous = 0;
    for index in 0..thumb_count {
        previous = thumb_instruction(&mut rng, index, previous, &mut thumb_lines, &mut coverage);
    }

    let mut modes = [
        ("fiq", MODE_FIQ),
        ("irq", MODE_IRQ),
        ("abort", MODE_ABORT),
        ("undefined", MODE_UNDEFINED),
        ("system", MODE_SYSTEM),
    ];
    rng.shuffle(&mut modes);
    mark(&mut coverage, &["mode", "banked_register"]);
    let mut mode_lines = Vec::new();
    for (sequence, (name, mode)) in modes.iter().enumerate() {
        mode_lines.extend([
            format!("    /* enter MODE_{} */", name.to_ascii_uppercase()),
            format!("    mov r7, #0x{mode:02x}"),
            "    msr cpsr_c, r7".to_owned(),
        ]);
        if *mode == MODE_FIQ {
            for register in 8..13 {
                mode_lines.push(format!(
                    "    mov r{register}, #{}",
                    (sequence + register) & 0xff
                ));
            }
        }
        mode_lines.push(format!("    mov r13, #{}", 0x40 + sequence));
        mode_lines.push(format!("    mov r14, #{}", 0x80 + sequence));
    }
    mode_lines.extend([
        "    /* return to MODE_SUPERVISOR */".to_owned(),
        format!("    mov r7, #0x{MODE_SUPERVISOR:02x}"),
        "    msr cpsr_c, r7".to_owned(),
    ]);
    let words: Vec<_> = (0..5).map(|_| rng.next_u32()).collect();
    let mut init_memory = vec!["    ldr r10, =0x00010000".to_owned()];
    for (index, word) in words.iter().enumerate() {
        init_memory.push(format!("    ldr r0, =0x{word:08x}"));
        init_memory.push(format!("    str r0, [r10, #{}]", 4 * index));
    }

    let mut lines = Vec::new();
    lines.extend(DIFFERENTIAL_START.lines().map(str::to_owned));
    lines.extend(init_memory);
    lines.extend([
        "    cmp r0, r0".to_owned(),
        "    b measure_begin".to_owned(),
        "    .ltorg /* keep initialization constants in ARM literal range */".to_owned(),
        "measure_begin:".to_owned(),
    ]);
    lines.extend(mode_lines);
    lines.extend([
        "    /* enter MODE_USER after every privileged bank is covered */".to_owned(),
        format!("    mov r7, #0x{MODE_USER:02x}"),
        "    msr cpsr_c, r7".to_owned(),
    ]);
    lines.extend(arm_lines);
    lines.extend(DIFFERENTIAL_THUMB_START.lines().map(str::to_owned));
    lines.extend(thumb_lines);
    lines.extend(DIFFERENTIAL_END.lines().map(str::to_owned));
    // The old Python list ended in an empty string and joined with newlines.
    lines.push(String::new());
    Differential {
        source: lines.join("\n"),
        coverage: coverage.into_iter().map(str::to_owned).collect(),
    }
}

const DIFFERENTIAL_START: &str = r"/* Generated constrained-random shared ARMv4T program. */
    .syntax unified
    .cpu arm7tdmi
    .section .text
    .global _start
    .global measure_begin
    .global measure_end
    .arm
_start:
    b initialize
    b undefined_handler
    b svc_handler
    b prefetch_abort_handler
    b data_abort_handler
    b reserved_handler
    b irq_handler
    b fiq_handler
initialize:
    mov r0, #0
    mov r1, #0
    mov r2, #0
    mov r3, #0
    mov r4, #0
    mov r5, #0
    mov r6, #0
    mov r7, #0
    mov r8, #0
    mov r9, #0
    mov r10, #0
    mov r11, #0
    mov r12, #0
    ldr r13, =0x0001f000
    mov r14, #0";

const DIFFERENTIAL_THUMB_START: &str = r"    ldr r5, =thumb_random_entry
    bx r5
    .ltorg /* mode-switch address must not span the Thumb body */
    .thumb
    .thumb_func
thumb_random_entry:
    ldr r6, =0x00010000
    b thumb_literals_end
    .ltorg /* Thumb-1 literal loads have only a 1020-byte reach */
thumb_literals_end:";

const DIFFERENTIAL_END: &str = r"    ldr r5, =arm_random_return
    bx r5
    .align 2
    .arm
arm_random_return:
    ldr r10, =0x00010000
    ldmia r10, {r8-r12}
    b measure_end
measure_end:
    ldr r0, =0x51a7e11a
    ldr r1, =0xae581ee5
    svc #0x7f
    b .
semihost_call:
    mov r0, #0x20
    adr r1, semihost_exit
    svc #0x123456
undefined_handler:
    add r6, r6, #1
    movs pc, lr
svc_handler:
    ldr r12, =0x51a7e11a
    cmp r0, r12
    bne ordinary_svc
    mvn r12, r12
    cmp r1, r12
    beq semihost_call
ordinary_svc:
    add r6, r6, #2
    movs pc, lr
prefetch_abort_handler:
data_abort_handler:
reserved_handler:
irq_handler:
fiq_handler:
    b .
    .align 2
semihost_exit:
    .word 0x00020026
    .word 0
    .ltorg";

fn load_memory(words: &[u32], address: usize, kind: &str, big_endian: bool) -> u32 {
    let word = words[address / 4];
    let low = address & 3;
    match kind {
        "word" => word.rotate_right((8 * low) as u32),
        "byte" | "signed_byte" => {
            let lane = if big_endian { 3 - low } else { low };
            let value = (word >> (8 * lane)) & 0xff;
            if kind == "signed_byte" {
                (value as i8 as i32) as u32
            } else {
                value
            }
        }
        "half" | "signed_half" => {
            assert!(low == 0 || low == 2);
            let high = if big_endian { low == 0 } else { low == 2 };
            let value = (word >> if high { 16 } else { 0 }) & 0xffff;
            if kind == "signed_half" {
                (value as i16 as i32) as u32
            } else {
                value
            }
        }
        _ => unreachable!(),
    }
}

fn store_memory(words: &mut [u32], address: usize, kind: &str, value: u32, big_endian: bool) {
    let index = address / 4;
    let low = address & 3;
    match kind {
        "word" => {
            assert_eq!(low, 0);
            words[index] = value;
        }
        "byte" => {
            let lane = if big_endian { 3 - low } else { low };
            let shift = 8 * lane;
            words[index] = (words[index] & !(0xff << shift)) | ((value & 0xff) << shift);
        }
        "half" => {
            assert!(low == 0 || low == 2);
            let high = if big_endian { low == 0 } else { low == 2 };
            let shift = if high { 16 } else { 0 };
            words[index] = (words[index] & !(0xffff << shift)) | ((value & 0xffff) << shift);
        }
        _ => unreachable!(),
    }
}

struct PolicyBuilder {
    lines: Vec<String>,
    words: Vec<u32>,
    results: Vec<u32>,
    big_endian: bool,
}

impl PolicyBuilder {
    fn emit_load(&mut self, kind: &str, address: usize) {
        let mnemonic = match kind {
            "word" => "ldr",
            "byte" => "ldrb",
            "signed_byte" => "ldrsb",
            "half" => "ldrh",
            "signed_half" => "ldrsh",
            _ => unreachable!(),
        };
        let result = load_memory(&self.words, address, kind, self.big_endian);
        let output_offset = 4 * self.results.len();
        self.lines
            .push(format!("    {mnemonic} r0, [r10, #{address}]"));
        self.lines
            .push(format!("    str r0, [r11, #{output_offset}]"));
        self.results.push(result);
    }

    fn emit_store(&mut self, kind: &str, address: usize, value: u32) {
        let mnemonic = match kind {
            "word" => "str",
            "byte" => "strb",
            "half" => "strh",
            _ => unreachable!(),
        };
        self.lines.push(format!("    ldr r0, =0x{value:08x}"));
        self.lines
            .push(format!("    {mnemonic} r0, [r10, #{address}]"));
        store_memory(&mut self.words, address, kind, value, self.big_endian);
    }
}

pub fn policy(seed: i64, instruction_count: usize, big_endian: bool) -> Policy {
    let seeded = seed ^ if big_endian { 0xb16e_0001 } else { 0x1177_e001 };
    let mut rng = PythonRandom::new(seeded);
    let words: Vec<_> = (0..8).map(|_| rng.next_u32()).collect();
    let mut builder = PolicyBuilder {
        lines: POLICY_START.lines().map(str::to_owned).collect(),
        words,
        results: Vec::new(),
        big_endian,
    };
    for (index, value) in builder.words.iter().enumerate() {
        builder.lines.push(format!("    ldr r0, =0x{value:08x}"));
        builder
            .lines
            .push(format!("    str r0, [r10, #{}]", 4 * index));
    }
    for offset in 0..4 {
        builder.emit_load("word", offset);
        builder.emit_load("byte", offset);
    }
    for offset in [0, 2] {
        builder.emit_load("half", offset);
        builder.emit_load("signed_half", offset);
    }
    builder.emit_load("signed_byte", 1);
    const LOAD_KINDS: [&str; 5] = ["word", "byte", "signed_byte", "half", "signed_half"];
    const STORE_KINDS: [&str; 3] = ["word", "byte", "half"];
    let operation_count = usize::max(32, instruction_count / 4);
    for _ in 0..operation_count {
        if rng.range(100) < 65 {
            let kind = rng.choice(&LOAD_KINDS);
            let address = if kind == "half" || kind == "signed_half" {
                2 * rng.range(builder.words.len() * 2)
            } else {
                rng.range(builder.words.len() * 4)
            };
            builder.emit_load(kind, address);
        } else {
            let kind = rng.choice(&STORE_KINDS);
            let address = match kind {
                "word" => 4 * rng.range(builder.words.len()),
                "half" => 2 * rng.range(builder.words.len() * 2),
                _ => rng.range(builder.words.len() * 4),
            };
            builder.emit_store(kind, address, rng.next_u32());
        }
    }
    for index in 0..builder.words.len() {
        builder.emit_load("word", 4 * index);
    }
    let result_words = builder.results.len() + 1;
    let completion_offset = 4 * builder.results.len();
    builder.lines.extend([
        "    ldr r0, =0xc0decafe".to_owned(),
        format!("    str r0, [r11, #{completion_offset}]"),
        "measure_end:".to_owned(),
        "    b .".to_owned(),
        "    .ltorg".to_owned(),
        String::new(),
    ]);
    builder.results.push(0xc0de_cafe);
    builder.results.extend(&builder.words);
    let coverage = [
        "alignment",
        "aligned",
        "unaligned",
        "memory",
        "permitted_memory",
        if big_endian { "big" } else { "little" },
        "byte",
        "halfword",
        "word",
        "signed",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    Policy {
        source: builder.lines.join("\n"),
        expected: builder.results,
        result_words,
        coverage,
    }
}

const POLICY_START: &str = r"/* Generated ARM7TDMI endian/unaligned permitted-memory program. */
    .syntax unified
    .cpu arm7tdmi
    .section .text
    .global _start
    .global measure_begin
    .global measure_end
    .arm
_start:
    b initialize
initialize:
    ldr r10, =0x00018000
    ldr r11, =0x0001a000
measure_begin:";

#[cfg(test)]
mod tests {
    use super::{differential, load_memory, policy, store_memory};
    use sha2::{Digest, Sha256};

    #[test]
    fn seeded_stream_has_bounded_literal_pools() {
        let hash = |source: &str| format!("{:x}", Sha256::digest(source.as_bytes()));
        let source = differential(1, 64).source;
        assert!(source.contains("b measure_begin\n    .ltorg"));
        assert!(source.contains("bx r5\n    .ltorg"));
        assert!(source.contains("b thumb_literals_end\n    .ltorg"));
        assert_eq!(
            hash(&source),
            "74a454343bb2889ca70c784eb36d477775739cc708700585cc43311140fa8fce"
        );
        assert_eq!(
            hash(&policy(1, 64, false).source),
            "a354598c18d144f630a4f05c049809a7f611f6913e42642e8049301f6c40421f"
        );
        assert_eq!(
            hash(&policy(1, 64, true).source),
            "f02a71d8988bb3eca1c468ff1e6f11471b3001babc28f9c1cb159acd91fc0593"
        );
    }

    #[test]
    fn endian_and_alignment_model() {
        let mut words = vec![0x12_34_56_78];
        assert_eq!(load_memory(&words, 1, "word", false), 0x78_12_34_56);
        assert_eq!(load_memory(&words, 0, "byte", false), 0x78);
        assert_eq!(load_memory(&words, 0, "byte", true), 0x12);
        store_memory(&mut words, 0, "byte", 0xaa, true);
        assert_eq!(words, [0xaa_34_56_78]);
    }
}
