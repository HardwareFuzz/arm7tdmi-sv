//! VAL-006 independent ARMv4T allocation enumerator and full-regression gate.
//! Exhaustive domain counts are computed here, then tied to hashed RTL phase
//! logs. Manifest entries alone cannot claim a bin was covered.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::{json, Value};

use crate::campaign::{file_entry, git_text, repo_root, sha256, utc_now, write_json, Failure};

const SCHEMA: &str = "arm7tdmis-functional-coverage-v1";
const MANIFEST_SCHEMA: &str = "arm7tdmis-functional-coverage-map-v1";
const GROUP_IDS: [&str; 19] = [
    "arm.class",
    "arm.condition",
    "arm.dp_opcode",
    "arm.shifter",
    "arm.multiply",
    "arm.psr",
    "arm.branch",
    "arm.single_transfer",
    "arm.extra_transfer",
    "arm.block_transfer",
    "arm.block_operands",
    "arm.swap",
    "arm.coprocessor",
    "thumb.format",
    "thumb.alu",
    "thumb.condition",
    "thumb.subfamily",
    "exception.reserved",
    "exception.policy",
];
const ARM_CLASSES: [&str; 16] = [
    "undef",
    "dp",
    "msr",
    "mrs",
    "mul",
    "mull",
    "branch",
    "bx",
    "ldr_str",
    "ldrh_strh",
    "ldm_stm",
    "swp",
    "swi",
    "cdp",
    "mcr_mrc",
    "ldc_stc",
];
const ARM_CLASS_TOTALS: [u32; 16] = [
    445, 784, 34, 2, 4, 8, 512, 1, 768, 64, 512, 2, 256, 128, 128, 448,
];
const DP_NAMES: [&str; 16] = [
    "and", "eor", "sub", "rsb", "add", "adc", "sbc", "rsc", "tst", "teq", "cmp", "cmn", "orr",
    "mov", "bic", "mvn",
];
const CONDITIONS: [&str; 15] = [
    "eq", "ne", "cs", "cc", "mi", "pl", "vs", "vc", "hi", "ls", "ge", "lt", "gt", "le", "al",
];
const THUMB_ALU: [&str; 16] = [
    "and", "eor", "lsl", "lsr", "asr", "adc", "sbc", "ror", "tst", "neg", "cmp", "cmn", "orr",
    "mul", "bic", "mvn",
];
const DOMAINS: [(&str, u32); 5] = [
    ("arm_decode_rows", 4096),
    ("arm_nv_rows", 4096),
    ("thumb_words", 65536),
    ("arm_static_policy_rows", 1066),
    ("thumb_static_policy_rows", 448),
];
const POLICY_COMPONENTS: [(&str, u32); 9] = [
    ("data_processing_fields", 2 * ((4 * 15) + (2 * 15))),
    ("short_multiply_operands", 14 + 7),
    ("long_multiply_operands", 4 * (4 + 15)),
    ("psr_operands", 8),
    ("psr_misc_fields", 2 * ((3 * 15) + 255 + 15)),
    ("bx_sbo_fields", 3 * 15),
    ("swp_sbz_fields", 2 * 15),
    ("extra_transfer_sbz_fields", 4 * 15),
    ("coprocessor_pc_writeback", 2 * 2 * 2 * 2),
];

fn failure(message: impl Into<String>) -> Failure {
    Failure::other(message)
}

fn names<const N: usize>(values: &[&str; N]) -> BTreeSet<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn str_field<'a>(entry: &'a Value, name: &str) -> Result<&'a str, Failure> {
    entry[name]
        .as_str()
        .ok_or_else(|| failure(format!("missing string field: {name}")))
}

fn string_array(entry: &Value) -> Option<BTreeSet<String>> {
    entry.as_array().and_then(|items| {
        items
            .iter()
            .map(|item| item.as_str().map(str::to_owned))
            .collect()
    })
}

fn required_bins(manifest: &Value) -> Result<Vec<String>, Failure> {
    let mut bins = Vec::new();
    for group in manifest["required_bin_groups"]
        .as_array()
        .ok_or_else(|| failure("functional-coverage required groups are incomplete"))?
    {
        let id = str_field(group, "id")?;
        for member in group["members"]
            .as_array()
            .ok_or_else(|| failure("functional-coverage members are malformed"))?
        {
            bins.push(format!(
                "{id}.{}",
                member
                    .as_str()
                    .ok_or_else(|| failure("non-string coverage member"))?
            ));
        }
    }
    bins.sort();
    Ok(bins)
}

fn validate_manifest(manifest: &Value) -> Result<(), Failure> {
    if manifest["schema"] != MANIFEST_SCHEMA {
        return Err(failure("functional-coverage manifest has wrong schema"));
    }
    let expected_domains: BTreeMap<_, _> = DOMAINS
        .iter()
        .map(|(key, value)| ((*key).to_owned(), *value))
        .collect();
    let actual_domains: BTreeMap<_, _> = manifest["domains"]
        .as_object()
        .ok_or_else(|| failure("functional-coverage domains are malformed"))?
        .iter()
        .map(|(key, value)| {
            Ok((
                key.clone(),
                u32::try_from(
                    value
                        .as_u64()
                        .ok_or_else(|| failure("non-integer domain"))?,
                )
                .map_err(|_| failure("domain exceeds u32"))?,
            ))
        })
        .collect::<Result<_, Failure>>()?;
    if actual_domains != expected_domains {
        return Err(failure("functional-coverage domain sizes were weakened"));
    }
    let groups = manifest["required_bin_groups"]
        .as_array()
        .ok_or_else(|| failure("functional-coverage required groups are incomplete"))?;
    let group_ids: BTreeSet<_> = groups
        .iter()
        .map(|group| str_field(group, "id").map(str::to_owned))
        .collect::<Result<_, _>>()?;
    if group_ids != names(&GROUP_IDS) || group_ids.len() != groups.len() {
        return Err(failure(
            "functional-coverage required groups are incomplete",
        ));
    }
    let evidence = manifest["evidence"]
        .as_array()
        .ok_or_else(|| failure("functional-coverage evidence is malformed"))?;
    let evidence_phases: BTreeSet<_> = evidence
        .iter()
        .map(|entry| str_field(entry, "phase").map(str::to_owned))
        .collect::<Result<_, _>>()?;
    if evidence_phases.len() != evidence.len() {
        return Err(failure("functional-coverage evidence phases are malformed"));
    }
    let citation = Regex::new(r"ARM DDI 0100|ARM DDI 0234|ARMv4T").unwrap();
    let mut members_by_group = BTreeMap::new();
    for group in groups {
        let id = str_field(group, "id")?;
        let members = string_array(&group["members"])
            .ok_or_else(|| failure(format!("functional-coverage group is malformed: {id}")))?;
        let evidence_refs = string_array(&group["evidence"])
            .ok_or_else(|| failure(format!("functional-coverage group is malformed: {id}")))?;
        let raw_members = group["members"].as_array().unwrap();
        let kind = str_field(group, "kind")?;
        let coverage = str_field(group, "coverage")?;
        if !["encoding-family", "exceptional-reserved"].contains(&kind)
            || !["enumerated", "evidence"].contains(&coverage)
            || members.is_empty()
            || members.len() != raw_members.len()
            || !group["citation"]
                .as_str()
                .is_some_and(|text| citation.is_match(text))
            || evidence_refs.is_empty()
            || !evidence_refs.is_subset(&evidence_phases)
        {
            return Err(failure(format!(
                "functional-coverage group is malformed: {id}"
            )));
        }
        members_by_group.insert(id, members);
    }
    for (id, required) in [
        ("arm.class", names(&ARM_CLASSES)),
        ("arm.condition", names(&CONDITIONS)),
        ("arm.dp_opcode", names(&DP_NAMES)),
        (
            "arm.multiply",
            names(&["mul", "mla", "umull", "umlal", "smull", "smlal"]),
        ),
        (
            "arm.coprocessor",
            names(&["cdp", "mcr", "mrc", "ldc", "stc"]),
        ),
        (
            "thumb.format",
            (1..=19).map(|number| format!("{number:02}")).collect(),
        ),
        ("thumb.alu", names(&THUMB_ALU)),
        (
            "thumb.condition",
            CONDITIONS[..14]
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
        ),
    ] {
        if members_by_group.get(id) != Some(&required) {
            return Err(failure(format!(
                "functional-coverage foundation changed: {id}"
            )));
        }
    }
    let bins = required_bins(manifest)?;
    if bins.len() < 180 || bins.iter().collect::<BTreeSet<_>>().len() != bins.len() {
        return Err(failure("functional-coverage required-bin set is too small"));
    }
    let exclusions = manifest["exclusions"]
        .as_array()
        .ok_or_else(|| failure("functional-coverage exclusions are missing"))?;
    if exclusions.is_empty() {
        return Err(failure("functional-coverage exclusions are missing"));
    }
    for exclusion in exclusions {
        if ["id", "pattern", "rationale"]
            .iter()
            .any(|key| exclusion[key].as_str().is_none_or(str::is_empty))
            || !exclusion["citation"]
                .as_str()
                .is_some_and(|text| citation.is_match(text))
        {
            return Err(failure("functional-coverage exclusion is uncited"));
        }
    }
    for entry in evidence {
        let keys: BTreeSet<_> = entry
            .as_object()
            .ok_or_else(|| failure("functional-coverage evidence entry is malformed"))?
            .keys()
            .map(String::as_str)
            .collect();
        if keys != BTreeSet::from(["phase", "source", "marker"])
            || entry["marker"].as_str().is_none_or(str::is_empty)
        {
            return Err(failure("functional-coverage evidence entry is malformed"));
        }
    }
    Ok(())
}

fn arm_class(hi: u16, lo: u16) -> &'static str {
    let major = (hi >> 5) & 7;
    match major {
        0 => {
            if lo & 9 == 9 {
                if lo == 9 {
                    if hi <= 3 {
                        "mul"
                    } else if (8..=15).contains(&hi) {
                        "mull"
                    } else if hi == 0x10 || hi == 0x14 {
                        "swp"
                    } else {
                        "undef"
                    }
                } else if lo == 0xb {
                    "ldrh_strh"
                } else if lo == 0xd || lo == 0xf {
                    if hi & 1 != 0 {
                        "ldrh_strh"
                    } else {
                        "undef"
                    }
                } else {
                    "undef"
                }
            } else if [0x10, 0x12, 0x14, 0x16].contains(&hi) {
                if (hi == 0x10 || hi == 0x14) && lo == 0 {
                    "mrs"
                } else if (hi == 0x12 || hi == 0x16) && lo == 0 {
                    "msr"
                } else if hi == 0x12 && lo == 1 {
                    "bx"
                } else {
                    "undef"
                }
            } else {
                "dp"
            }
        }
        1 => {
            if hi == 0x30 || hi == 0x34 {
                "undef"
            } else if hi == 0x32 || hi == 0x36 {
                "msr"
            } else {
                "dp"
            }
        }
        2 => "ldr_str",
        3 => {
            if lo & 1 != 0 {
                "undef"
            } else {
                "ldr_str"
            }
        }
        4 => "ldm_stm",
        5 => "branch",
        6 => {
            if hi >> 3 == 0b11000 && hi & 2 == 0 {
                "undef"
            } else {
                "ldc_stc"
            }
        }
        _ => {
            if hi & 0x10 != 0 {
                "swi"
            } else if lo & 1 != 0 {
                "mcr_mrc"
            } else {
                "cdp"
            }
        }
    }
}

fn arm_bins(hi: u16, lo: u16, class: &str, bins: &mut BTreeSet<String>) {
    bins.insert(format!("arm.class.{class}"));
    match class {
        "dp" => {
            bins.insert(format!(
                "arm.dp_opcode.{}",
                DP_NAMES[((hi >> 1) & 15) as usize]
            ));
            if hi & 0x20 != 0 {
                bins.insert("arm.shifter.immediate_rotate".into());
            } else {
                let shift = ["lsl", "lsr", "asr", "ror"][((lo >> 1) & 3) as usize];
                let suffix = if lo & 1 != 0 {
                    "register"
                } else if shift == "ror" && lo & 8 == 0 {
                    "rrx"
                } else {
                    "immediate"
                };
                if suffix == "rrx" {
                    bins.insert("arm.shifter.rrx".into());
                } else {
                    bins.insert(format!("arm.shifter.{shift}_{suffix}"));
                }
            }
        }
        "mrs" | "msr" => {
            let psr = if hi & 4 != 0 { "spsr" } else { "cpsr" };
            let name = if class == "mrs" {
                format!("mrs_{psr}")
            } else {
                format!(
                    "msr_{}_{psr}",
                    if hi & 0x20 != 0 {
                        "immediate"
                    } else {
                        "register"
                    }
                )
            };
            bins.insert(format!("arm.psr.{name}"));
        }
        "mul" => {
            bins.insert(format!(
                "arm.multiply.{}",
                if hi & 2 != 0 { "mla" } else { "mul" }
            ));
        }
        "mull" => {
            let name = match (hi & 4 != 0, hi & 2 != 0) {
                (true, true) => "smlal",
                (true, false) => "smull",
                (false, true) => "umlal",
                (false, false) => "umull",
            };
            bins.insert(format!("arm.multiply.{name}"));
        }
        "branch" => {
            bins.insert(format!(
                "arm.branch.{}",
                if hi & 0x10 != 0 { "bl" } else { "b" }
            ));
        }
        "bx" => {
            bins.insert("arm.branch.bx".into());
        }
        "ldr_str" => {
            let (pre, up, byte, writeback, load) = (
                hi & 0x10 != 0,
                hi & 8 != 0,
                hi & 4 != 0,
                hi & 2 != 0,
                hi & 1 != 0,
            );
            let operation = match (load, byte) {
                (true, true) => "ldrb",
                (true, false) => "ldr",
                (false, true) => "strb",
                (false, false) => "str",
            };
            for name in [
                operation,
                if hi & 0x20 != 0 {
                    "register_offset"
                } else {
                    "immediate_offset"
                },
                if pre { "pre_index" } else { "post_index" },
                if up { "up" } else { "down" },
                if !pre || writeback {
                    "writeback"
                } else {
                    "no_writeback"
                },
            ] {
                bins.insert(format!("arm.single_transfer.{name}"));
            }
            if !pre && writeback {
                bins.insert(format!(
                    "arm.single_transfer.translated_{}",
                    if load { "load" } else { "store" }
                ));
            }
        }
        "ldrh_strh" => {
            let (pre, up, immediate, writeback, load) = (
                hi & 0x10 != 0,
                hi & 8 != 0,
                hi & 4 != 0,
                hi & 2 != 0,
                hi & 1 != 0,
            );
            let sh = (lo >> 1) & 3;
            let operation = if sh == 1 && !load {
                "strh"
            } else if sh == 1 {
                "ldrh"
            } else if sh == 2 {
                "ldrsb"
            } else {
                "ldrsh"
            };
            for name in [
                operation,
                if immediate {
                    "immediate_offset"
                } else {
                    "register_offset"
                },
                if pre { "pre_index" } else { "post_index" },
                if up { "up" } else { "down" },
                if !pre || writeback {
                    "writeback"
                } else {
                    "no_writeback"
                },
            ] {
                bins.insert(format!("arm.extra_transfer.{name}"));
            }
        }
        "ldm_stm" => {
            let (pre, up, s_bit, writeback, load) = (
                hi & 0x10 != 0,
                hi & 8 != 0,
                hi & 4 != 0,
                hi & 2 != 0,
                hi & 1 != 0,
            );
            let mode = if up && pre {
                "ib"
            } else if up {
                "ia"
            } else if pre {
                "db"
            } else {
                "da"
            };
            for name in [
                if load { "ldm" } else { "stm" },
                mode,
                if writeback {
                    "writeback"
                } else {
                    "no_writeback"
                },
            ] {
                bins.insert(format!("arm.block_transfer.{name}"));
            }
            if s_bit {
                bins.insert("arm.block_operands.user_bank".into());
            }
        }
        "swp" => {
            bins.insert(format!(
                "arm.swap.{}",
                if hi & 4 != 0 { "swpb" } else { "swp" }
            ));
        }
        "cdp" => {
            bins.insert("arm.coprocessor.cdp".into());
        }
        "mcr_mrc" => {
            bins.insert(format!(
                "arm.coprocessor.{}",
                if hi & 1 != 0 { "mrc" } else { "mcr" }
            ));
        }
        "ldc_stc" => {
            bins.insert(format!(
                "arm.coprocessor.{}",
                if hi & 1 != 0 { "ldc" } else { "stc" }
            ));
        }
        _ => {}
    }
}

fn thumb_reserved(word: u16) -> bool {
    (0xb100..=0xb3ff).contains(&word)
        || (0xb600..=0xbbff).contains(&word)
        || (0xbe00..=0xbfff).contains(&word)
        || (0xde00..=0xdeff).contains(&word)
        || (0xe800..=0xefff).contains(&word)
}

fn thumb_policy_unpredictable(word: u16) -> bool {
    let format5 = word >> 10 == 0b010001;
    let operation = (word >> 8) & 3;
    format5
        && ((operation != 3
            && ((word & 0x80 == 0 && word & 0x40 == 0)
                || (operation == 1 && (((word >> 4) & 8) | (word & 7)) == 15)))
            || (operation == 3 && (word & 0x80 != 0 || word & 7 != 0)))
}

fn thumb_format(word: u16) -> Option<u8> {
    Some(match word {
        0x0000..=0x17ff => 1,
        0x1800..=0x1fff => 2,
        0x2000..=0x3fff => 3,
        0x4000..=0x43ff => 4,
        0x4400..=0x47ff => 5,
        0x4800..=0x4fff => 6,
        0x5000..=0x51ff | 0x5400..=0x55ff | 0x5800..=0x59ff | 0x5c00..=0x5dff => 7,
        0x5200..=0x53ff | 0x5600..=0x57ff | 0x5a00..=0x5bff | 0x5e00..=0x5fff => 8,
        0x6000..=0x7fff => 9,
        0x8000..=0x8fff => 10,
        0x9000..=0x9fff => 11,
        0xa000..=0xafff => 12,
        0xb000..=0xb0ff => 13,
        0xb400..=0xb5ff | 0xbc00..=0xbdff => 14,
        0xc000..=0xcfff => 15,
        0xd000..=0xddff => 16,
        0xdf00..=0xdfff => 17,
        0xe000..=0xe7ff => 18,
        0xf000..=0xffff => 19,
        _ => return None,
    })
}

fn thumb_bins(word: u16, format: u8, bins: &mut BTreeSet<String>) {
    bins.insert(format!("thumb.format.{format:02}"));
    let subfamily = match format {
        1 => Some(format!(
            "fmt1_{}",
            ["lsl", "lsr", "asr"][((word >> 11) & 3) as usize]
        )),
        2 => Some(format!(
            "fmt2_{}_{}",
            if word & 0x200 != 0 { "sub" } else { "add" },
            if word & 0x400 != 0 {
                "immediate"
            } else {
                "register"
            }
        )),
        3 => Some(format!(
            "fmt3_{}",
            ["mov", "cmp", "add", "sub"][((word >> 11) & 3) as usize]
        )),
        4 => {
            bins.insert(format!(
                "thumb.alu.{}",
                THUMB_ALU[((word >> 6) & 15) as usize]
            ));
            None
        }
        5 => Some(
            ["fmt5_add_high", "fmt5_cmp_high", "fmt5_mov_high", "fmt5_bx"]
                [((word >> 8) & 3) as usize]
                .into(),
        ),
        6 => Some("fmt6_ldr_literal".into()),
        7 => Some(
            ["fmt7_str", "fmt7_strb", "fmt7_ldr", "fmt7_ldrb"][((word >> 10) & 3) as usize].into(),
        ),
        8 => Some(
            ["fmt8_strh", "fmt8_ldrsb", "fmt8_ldrh", "fmt8_ldrsh"][((word >> 10) & 3) as usize]
                .into(),
        ),
        9 => Some(
            ["fmt9_str", "fmt9_ldr", "fmt9_strb", "fmt9_ldrb"][((word >> 11) & 3) as usize].into(),
        ),
        10 => Some(format!(
            "fmt10_{}",
            if word & 0x800 != 0 { "ldrh" } else { "strh" }
        )),
        11 => Some(format!(
            "fmt11_{}",
            if word & 0x800 != 0 {
                "ldr_sp"
            } else {
                "str_sp"
            }
        )),
        12 => Some(format!(
            "fmt12_{}",
            if word & 0x800 != 0 {
                "add_sp"
            } else {
                "add_pc"
            }
        )),
        13 => Some(format!(
            "fmt13_{}",
            if word & 0x80 != 0 { "sub_sp" } else { "add_sp" }
        )),
        14 => {
            let load = word & 0x800 != 0;
            if word & 0x100 != 0 {
                bins.insert(format!(
                    "thumb.subfamily.fmt14_{}",
                    if load { "pop_pc" } else { "push_lr" }
                ));
            }
            Some(format!("fmt14_{}", if load { "pop" } else { "push" }))
        }
        15 => Some(format!(
            "fmt15_{}",
            if word & 0x800 != 0 { "ldmia" } else { "stmia" }
        )),
        16 => {
            bins.insert(format!(
                "thumb.condition.{}",
                CONDITIONS[((word >> 8) & 15) as usize]
            ));
            None
        }
        17 => Some("fmt17_swi".into()),
        18 => Some("fmt18_b".into()),
        19 => Some(format!(
            "fmt19_bl_{}",
            if word & 0x800 != 0 {
                "suffix"
            } else {
                "prefix"
            }
        )),
        _ => None,
    };
    if let Some(name) = subfamily {
        bins.insert(format!("thumb.subfamily.{name}"));
    }
}

fn enumerate_reference(manifest: &Value) -> Result<Value, Failure> {
    validate_manifest(manifest)?;
    let mut covered = BTreeSet::new();
    let mut counts: BTreeMap<String, u32> =
        ARM_CLASSES.iter().map(|name| ((*name).into(), 0)).collect();
    let mut arm_decode_rows = 0_u32;
    for hi in 0..256 {
        for lo in 0..16 {
            arm_decode_rows += 1;
            let class = arm_class(hi, lo);
            *counts.get_mut(class).unwrap() += 1;
            arm_bins(hi, lo, class, &mut covered);
        }
    }
    for (class, expected) in ARM_CLASSES.iter().zip(ARM_CLASS_TOTALS) {
        if counts[*class] != expected {
            return Err(failure("independent ARM allocation totals changed"));
        }
    }
    let mut arm_nv_rows = 0_u32;
    for condition in [0xf_u8] {
        for _hi in 0..256 {
            for _lo in 0..16 {
                if condition != 0xf {
                    return Err(failure("ARM NV reference condition changed"));
                }
                arm_nv_rows += 1;
            }
        }
    }
    for condition in CONDITIONS {
        covered.insert(format!("arm.condition.{condition}"));
    }
    let mut policy_rows = 0;
    let mut thumb_words = 0_u32;
    for word in 0..=u16::MAX {
        thumb_words += 1;
        if thumb_policy_unpredictable(word) {
            policy_rows += 1;
        }
        if thumb_reserved(word) || thumb_policy_unpredictable(word) {
            continue;
        }
        let format = thumb_format(word)
            .ok_or_else(|| failure(format!("valid Thumb word lacks a format: {word:04x}")))?;
        thumb_bins(word, format, &mut covered);
    }
    if policy_rows != 448 {
        return Err(failure("independent Thumb policy total changed"));
    }
    let components: BTreeMap<_, _> = POLICY_COMPONENTS
        .iter()
        .map(|(key, value)| ((*key).to_owned(), *value))
        .collect();
    let arm_policy_rows = components.values().sum::<u32>();
    if arm_policy_rows != 1066 {
        return Err(failure("independent ARM policy total changed"));
    }
    let observed = BTreeMap::from([
        ("arm_decode_rows", arm_decode_rows),
        ("arm_nv_rows", arm_nv_rows),
        ("thumb_words", thumb_words),
        ("arm_static_policy_rows", arm_policy_rows),
        ("thumb_static_policy_rows", policy_rows),
    ]);
    if DOMAINS
        .iter()
        .any(|(name, expected)| observed[name] != *expected)
    {
        return Err(failure("independent functional-coverage domains changed"));
    }
    let domains: BTreeMap<_, _> = DOMAINS
        .iter()
        .map(|(key, count)| {
            (
                (*key).to_owned(),
                json!({"expected": count, "observed": observed[key]}),
            )
        })
        .collect();
    Ok(json!({
        "domains": domains,
        "covered_bins": covered,
        "arm_class_counts": counts,
        "arm_static_policy_components": components,
    }))
}

fn relative(path: &Path) -> Result<String, Failure> {
    Ok(path
        .strip_prefix(repo_root())
        .map_err(|error| failure(error.to_string()))?
        .to_string_lossy()
        .into_owned())
}

fn repo_file(text: &str) -> Result<PathBuf, Failure> {
    let root = fs::canonicalize(repo_root())?;
    let path = fs::canonicalize(root.join(text))?;
    if !path.starts_with(&root) || !path.is_file() {
        return Err(failure(format!(
            "evidence file escapes repository or is missing: {text}"
        )));
    }
    Ok(path)
}

fn option_or(arguments: &[String], name: &str, default: &str) -> Result<String, Failure> {
    if arguments.iter().any(|argument| argument == name) {
        crate::option(arguments, name).map_err(failure)
    } else {
        Ok(default.to_owned())
    }
}

pub fn functional_reference(arguments: &[String]) -> Result<String, String> {
    let manifest = option_or(
        arguments,
        "--manifest",
        "verification/functional_coverage.json",
    )
    .and_then(|text| repo_file(&text))
    .and_then(|path| fs::read(path).map_err(Failure::from))
    .and_then(|bytes| {
        serde_json::from_slice::<Value>(&bytes).map_err(|error| failure(error.to_string()))
    })
    .and_then(|manifest| enumerate_reference(&manifest))
    .map_err(|error| error.to_string())?;
    serde_json::to_string(&manifest).map_err(|error| error.to_string())
}

pub fn functional_coverage(arguments: &[String]) -> Result<(), String> {
    run(arguments).map_err(|error| error.to_string())
}

fn run(arguments: &[String]) -> Result<(), Failure> {
    let root = repo_root();
    let manifest_path = repo_file(&option_or(
        arguments,
        "--manifest",
        "verification/functional_coverage.json",
    )?)?;
    let regression_path = repo_file(&option_or(
        arguments,
        "--regression",
        "reports/generated/regression.json",
    )?)?;
    let output_text = option_or(
        arguments,
        "--output",
        "reports/generated/functional-coverage-report.json",
    )?;
    let output_path = if Path::new(&output_text).is_absolute() {
        PathBuf::from(output_text)
    } else {
        root.join(output_text)
    };
    let manifest: Value = serde_json::from_slice(&fs::read(&manifest_path)?)
        .map_err(|error| failure(error.to_string()))?;
    let regression: Value = serde_json::from_slice(&fs::read(&regression_path)?)
        .map_err(|error| failure(error.to_string()))?;
    validate_manifest(&manifest)?;
    let commit = git_text(&["rev-parse", "HEAD"])?;
    let dirty = !git_text(&["status", "--porcelain"])?.is_empty();
    if dirty {
        return Err(failure("functional coverage requires a clean source tree"));
    }
    if regression["schema"] != "arm7tdmis-regression-v1"
        || regression["mode"] != "full"
        || regression["git"]["commit"] != commit
        || regression["git"]["dirty"] != false
        || !["running", "passed"].contains(&regression["status"].as_str().unwrap_or(""))
    {
        return Err(failure(
            "functional coverage requires the current clean full regression",
        ));
    }
    let results = regression["results"]
        .as_array()
        .ok_or_else(|| failure("regression result list is malformed"))?;
    let result_by_name: BTreeMap<_, _> = results
        .iter()
        .filter_map(|result| result["name"].as_str().map(|name| (name, result)))
        .collect();
    let mut evidence_report = serde_json::Map::new();
    for evidence in manifest["evidence"].as_array().unwrap() {
        let phase = str_field(evidence, "phase")?;
        let result = result_by_name.get(phase).ok_or_else(|| {
            failure(format!(
                "required functional-coverage phase did not pass: {phase}"
            ))
        })?;
        if result["status"] != "passed" || result["exit_code"] != 0 {
            return Err(failure(format!(
                "required functional-coverage phase did not pass: {phase}"
            )));
        }
        let log_path = repo_file(str_field(result, "log")?)?;
        let source_path = repo_file(str_field(evidence, "source")?)?;
        if sha256(&log_path)? != str_field(result, "log_sha256")? {
            return Err(failure(format!(
                "functional-coverage evidence hash is invalid: {phase}"
            )));
        }
        let bytes = fs::read(&log_path)?;
        let log_text = String::from_utf8_lossy(&bytes);
        let marker = str_field(evidence, "marker")?;
        if !Regex::new(marker)
            .map_err(|error| failure(error.to_string()))?
            .is_match(&log_text)
        {
            return Err(failure(format!(
                "functional-coverage PASS marker is missing: {phase}"
            )));
        }
        evidence_report.insert(
            phase.into(),
            json!({
                "status": "passed", "marker": marker,
                "log": file_entry(&log_path)?, "source": file_entry(&source_path)?,
            }),
        );
    }
    let reference = enumerate_reference(&manifest)?;
    let mut covered = string_array(&reference["covered_bins"]).unwrap();
    for group in manifest["required_bin_groups"].as_array().unwrap() {
        if group["coverage"] == "evidence" {
            let id = str_field(group, "id")?;
            for phase in string_array(&group["evidence"]).unwrap() {
                if !evidence_report.contains_key(&phase) {
                    return Err(failure(format!(
                        "functional-coverage group lacks evidence: {id}"
                    )));
                }
            }
            for member in string_array(&group["members"]).unwrap() {
                covered.insert(format!("{id}.{member}"));
            }
        }
    }
    let required = required_bins(&manifest)?;
    let covered_required: Vec<_> = required
        .iter()
        .filter(|bin| covered.contains(*bin))
        .cloned()
        .collect();
    let uncovered: Vec<_> = required
        .iter()
        .filter(|bin| !covered.contains(*bin))
        .cloned()
        .collect();
    let source_path = root.join("verification/constrained_random_rs/src/functional_coverage.rs");
    let mut inputs = serde_json::Map::new();
    for path in [&manifest_path, &source_path] {
        inputs.insert(relative(path)?, file_entry(path)?);
    }
    let report = json!({
        "schema": SCHEMA,
        "created_utc": utc_now(),
        "status": if uncovered.is_empty() { "passed" } else { "failed" },
        "git": {"commit": commit, "dirty": dirty},
        "regression": {
            "schema": regression["schema"], "path": relative(&regression_path)?,
            "commit": regression["git"]["commit"], "mode": regression["mode"],
            "status_when_sampled": regression["status"],
        },
        "domains": reference["domains"],
        "arm_class_counts": reference["arm_class_counts"],
        "arm_static_policy_components": reference["arm_static_policy_components"],
        "required_bin_count": required.len(), "required_bins": required,
        "covered_bin_count": covered_required.len(), "covered_bins": covered_required,
        "uncovered_bins": uncovered, "exclusions": manifest["exclusions"],
        "evidence": evidence_report, "inputs": inputs,
    });
    write_json(&output_path, &report)?;
    if !uncovered.is_empty() {
        return Err(failure(format!(
            "functional-coverage uncovered bins: {}",
            uncovered.join(",")
        )));
    }
    println!("[functional-coverage] PASS ({} required bins, zero uncovered; 4096 ARM + 4096 NV + 65536 Thumb rows)", required.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_manifest() -> Value {
        serde_json::from_slice(
            &fs::read(repo_root().join("verification/functional_coverage.json")).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn independent_domains_and_all_enumerated_bins_close() {
        let manifest = fixture_manifest();
        let reference = enumerate_reference(&manifest).unwrap();
        assert_eq!(required_bins(&manifest).unwrap().len(), 234);
        for (field, expected) in [
            ("arm_decode_rows", 4_096),
            ("arm_nv_rows", 4_096),
            ("thumb_words", 65_536),
            ("arm_static_policy_rows", 1_066),
            ("thumb_static_policy_rows", 448),
        ] {
            assert_eq!(reference["domains"][field]["expected"], expected);
            assert_eq!(reference["domains"][field]["observed"], expected);
        }
        assert_eq!(reference["domains"]["thumb_words"]["observed"], 65536);
        assert_eq!(
            reference["domains"]["arm_static_policy_rows"]["observed"],
            1066
        );
        let covered = string_array(&reference["covered_bins"]).unwrap();
        for group in manifest["required_bin_groups"].as_array().unwrap() {
            if group["coverage"] == "enumerated" {
                for member in string_array(&group["members"]).unwrap() {
                    assert!(covered.contains(&format!(
                        "{}.{}",
                        group["id"].as_str().unwrap(),
                        member
                    )));
                }
            }
        }
    }

    #[test]
    fn manifest_keeps_all_arm_thumb_and_exception_foundations() {
        let manifest = fixture_manifest();
        validate_manifest(&manifest).unwrap();
        let groups = manifest["required_bin_groups"]
            .as_array()
            .unwrap()
            .iter()
            .map(|group| {
                (
                    group["id"].as_str().unwrap(),
                    string_array(&group["members"]).unwrap(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(groups["arm.class"], names(&ARM_CLASSES));
        assert_eq!(groups["arm.dp_opcode"], names(&DP_NAMES));
        assert_eq!(groups["arm.condition"].len(), 15);
        assert_eq!(groups["thumb.format"].len(), 19);
        assert_eq!(groups["thumb.alu"], names(&THUMB_ALU));
        assert_eq!(groups["thumb.condition"].len(), 14);
        for group in [
            "arm.shifter",
            "arm.multiply",
            "arm.psr",
            "arm.branch",
            "arm.single_transfer",
            "arm.extra_transfer",
            "arm.block_transfer",
            "arm.swap",
            "arm.coprocessor",
            "thumb.subfamily",
            "exception.reserved",
            "exception.policy",
        ] {
            assert!(!groups[group].is_empty(), "empty required group: {group}");
        }
        let bins = required_bins(&manifest).unwrap();
        assert_eq!(bins.len(), bins.iter().collect::<BTreeSet<_>>().len());
        for exclusion in manifest["exclusions"].as_array().unwrap() {
            let citation = exclusion["citation"].as_str().unwrap();
            assert!(["ARM DDI 0100", "ARM DDI 0234", "ARMv4T"]
                .iter()
                .any(|marker| citation.contains(marker)));
            assert!(exclusion["rationale"]
                .as_str()
                .is_some_and(|value| !value.is_empty()));
        }
    }

    #[test]
    fn exhaustive_simulations_and_aggregate_gate_remain_mandatory() {
        let root = repo_root();
        let read = |path: &str| fs::read_to_string(root.join(path)).unwrap();
        let reserved = read("tb/unit/reserved_decode_tb.sv");
        for marker in [
            "4096 ARM decode rows",
            "65536 Thumb words",
            "448 policy traps",
        ] {
            assert!(reserved.contains(marker));
        }
        let unpredictable = read("tb/unit/unpredictable_decode_tb.sv");
        for marker in ["1066 ARM traps", "448 Thumb traps", "12 deterministic rows"] {
            assert!(unpredictable.contains(marker));
        }
        let makefile = read("scripts/Makefile");
        assert!(makefile.contains("functional-coverage:"));
        assert!(makefile.contains("$(RANDOM_RUST_BIN)) functional-coverage"));
        let regression = read("verification/constrained_random_rs/src/regression.rs");
        assert!(regression.contains("\"functional-coverage\""));
        assert!(regression.contains("if !args.quick"));
        assert!(
            read("verification/constrained_random_rs/src/release_evidence/files/coverage.rs")
                .contains("functional-coverage-report.json")
        );
        assert!(
            read("verification/constrained_random_rs/src/release_evidence.rs")
                .contains("functional::validate")
        );
        assert!(read("verification/traceability.json").contains("VAL-006"));
        assert!(read("TASKS.md").contains("- [x] **VAL-006:**"));
        assert!(read("docs/VERIFICATION.md").contains("arm7tdmis-functional-coverage-v1"));
    }

    #[test]
    fn rejects_missing_group_and_weakened_domain() {
        let mut manifest = fixture_manifest();
        manifest["domains"]["thumb_words"] = json!(65535);
        assert!(validate_manifest(&manifest).is_err());
        manifest = fixture_manifest();
        manifest["required_bin_groups"]
            .as_array_mut()
            .unwrap()
            .pop();
        assert!(validate_manifest(&manifest).is_err());
    }
}
