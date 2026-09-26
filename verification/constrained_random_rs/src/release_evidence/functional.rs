//! Independent release checks for the exhaustive functional-coverage map.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};

use super::string_set;

const GROUPS: [&str; 19] = [
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

const DOMAINS: [(&str, u64); 5] = [
    ("arm_decode_rows", 4096),
    ("arm_nv_rows", 4096),
    ("thumb_words", 65536),
    ("arm_static_policy_rows", 1066),
    ("thumb_static_policy_rows", 448),
];

fn arm_citation(value: &Value) -> bool {
    value.as_str().is_some_and(|text| {
        ["ARM DDI 0100", "ARM DDI 0234", "ARMv4T"]
            .iter()
            .any(|citation| text.contains(citation))
    })
}

fn required_bins(manifest: &Value) -> Result<(Vec<String>, BTreeSet<String>), String> {
    if manifest["schema"] != "arm7tdmis-functional-coverage-map-v1" {
        return Err("functional-coverage manifest has wrong schema".into());
    }
    let domains = DOMAINS.into_iter().collect::<BTreeMap<_, _>>();
    if manifest["domains"] != json!(domains) {
        return Err("functional-coverage domains were weakened".into());
    }

    let groups = manifest["required_bin_groups"]
        .as_array()
        .ok_or("functional-coverage groups are incomplete")?;
    let group_ids = groups
        .iter()
        .map(|group| group["id"].as_str())
        .collect::<Option<BTreeSet<_>>>()
        .ok_or("functional-coverage groups are incomplete")?;
    if groups.len() != GROUPS.len() || group_ids != GROUPS.into_iter().collect() {
        return Err("functional-coverage groups are incomplete".into());
    }

    let evidence_entries = manifest["evidence"]
        .as_array()
        .ok_or("functional-coverage evidence map is malformed")?;
    if evidence_entries.iter().any(|entry| !entry.is_object()) {
        return Err("functional-coverage evidence map is malformed".into());
    }
    let evidence_phases = evidence_entries
        .iter()
        .map(|entry| entry["phase"].as_str().map(str::to_owned))
        .collect::<Option<BTreeSet<_>>>()
        .ok_or("functional-coverage evidence phases are ambiguous")?;
    if evidence_phases.len() != evidence_entries.len() {
        return Err("functional-coverage evidence phases are ambiguous".into());
    }

    let mut required = Vec::new();
    let mut by_group = BTreeMap::new();
    for group in groups {
        let id = group["id"].as_str().unwrap();
        let members = group["members"].as_array();
        let member_set = string_set(&group["members"]);
        let evidence = group["evidence"].as_array();
        let allowed_kind = ["encoding-family", "exceptional-reserved"]
            .contains(&group["kind"].as_str().unwrap_or(""));
        let allowed_coverage =
            ["enumerated", "evidence"].contains(&group["coverage"].as_str().unwrap_or(""));
        if !allowed_kind
            || !allowed_coverage
            || members.is_none_or(Vec::is_empty)
            || member_set
                .as_ref()
                .is_none_or(|set| set.len() != members.unwrap().len())
            || !arm_citation(&group["citation"])
            || evidence.is_none_or(Vec::is_empty)
            || evidence.unwrap().iter().any(|phase| {
                phase
                    .as_str()
                    .is_none_or(|name| !evidence_phases.contains(name))
            })
        {
            return Err(format!("functional-coverage group is malformed: {id}"));
        }
        for member in members.unwrap() {
            required.push(format!("{id}.{}", member.as_str().unwrap()));
        }
        by_group.insert(id, member_set.unwrap());
    }
    required.sort();
    if required.len() != 234 || required.iter().collect::<BTreeSet<_>>().len() != required.len() {
        return Err("functional-coverage required-bin count changed".into());
    }

    let foundations = [
        (
            "arm.class",
            [
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
            ]
            .into_iter()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>(),
        ),
        (
            "arm.dp_opcode",
            [
                "and", "eor", "sub", "rsb", "add", "adc", "sbc", "rsc", "tst", "teq", "cmp", "cmn",
                "orr", "mov", "bic", "mvn",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        ),
        (
            "thumb.format",
            (1..20).map(|number| format!("{number:02}")).collect(),
        ),
    ];
    if foundations
        .iter()
        .any(|(group, expected)| by_group.get(group) != Some(expected))
    {
        return Err("functional-coverage foundational bins changed".into());
    }

    let exclusions = manifest["exclusions"]
        .as_array()
        .filter(|rows| !rows.is_empty())
        .ok_or("functional-coverage exclusions are missing")?;
    for exclusion in exclusions {
        if ["id", "pattern", "rationale"]
            .iter()
            .any(|field| exclusion[*field].as_str().is_none_or(str::is_empty))
            || !arm_citation(&exclusion["citation"])
        {
            return Err("functional-coverage exclusion is uncited".into());
        }
    }
    Ok((required, evidence_phases))
}

pub(super) fn validate(
    report: &Value,
    manifest: &Value,
    expected_commit: &str,
) -> Result<(), String> {
    let (required, evidence_phases) = required_bins(manifest)?;
    if report["schema"] != "arm7tdmis-functional-coverage-v1" {
        return Err("functional-coverage report has wrong schema".into());
    }
    if report["status"] != "passed" {
        return Err("functional-coverage report is not passed".into());
    }
    if report["git"]["dirty"] != false {
        return Err("functional-coverage report describes a dirty source tree".into());
    }
    if report["git"]["commit"] != expected_commit {
        return Err("functional-coverage report commit does not match regression".into());
    }

    let expected_domains = DOMAINS
        .into_iter()
        .map(|(name, count)| (name, json!({"expected": count, "observed": count})))
        .collect::<BTreeMap<_, _>>();
    if report["domains"] != json!(expected_domains) {
        return Err("functional-coverage exhaustive domains are incomplete".into());
    }
    if report["required_bin_count"] != required.len()
        || report["required_bins"] != json!(required)
        || report["covered_bin_count"] != required.len()
        || report["covered_bins"] != json!(required)
        || report["uncovered_bins"] != json!([])
    {
        return Err("functional-coverage required bins are incomplete".into());
    }
    if report["exclusions"] != manifest["exclusions"] {
        return Err("functional-coverage exclusions do not match".into());
    }
    let evidence = report["evidence"].as_object();
    if evidence.is_none_or(|entries| {
        entries.keys().cloned().collect::<BTreeSet<_>>() != evidence_phases
            || entries.values().any(|entry| entry["status"] != "passed")
    }) {
        return Err("functional-coverage evidence is incomplete".into());
    }
    Ok(())
}
