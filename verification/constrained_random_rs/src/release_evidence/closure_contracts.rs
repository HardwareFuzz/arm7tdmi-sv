//! Release-closure mutation tests for the formal and functional-coverage gates.
//! The fixtures exercise the Rust validators directly without a Python CLI client.

use std::collections::BTreeSet;
use std::fs;

use serde_json::{json, Map, Value};

use super::{functional, validate_formal};
use crate::campaign::repo_root;

fn manifest(relative: &str) -> Value {
    serde_json::from_slice(&fs::read(repo_root().join(relative)).unwrap()).unwrap()
}

fn formal_report(manifest: &Value) -> Value {
    let mut proofs = manifest["proofs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["id"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    proofs.sort();
    let mut covers = Vec::new();
    for (field, prefix) in [
        ("fsm_states", "fsm.state"),
        ("fsm_transitions", "fsm.transition"),
        ("exceptions", "exception"),
        ("debug", "debug"),
    ] {
        covers.extend(
            manifest["covers"][field]
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| format!("{prefix}.{}", entry.as_str().unwrap())),
        );
    }
    covers.sort();

    let mut results = Map::new();
    for obligation in proofs.iter().chain(&covers) {
        let mut entry = json!({
            "status": "passed",
            "engine": "smtbmc boolector",
            "depth": 32,
            "log": {
                "path": format!("reports/generated/formal/{obligation}.log"),
                "bytes": 100,
                "sha256": "1".repeat(64),
            },
            "source": {
                "path": "tb/formal/arm7tdmis_core_formal.sv",
                "bytes": 100,
                "sha256": "2".repeat(64),
            },
        });
        if covers.binary_search(obligation).is_ok() {
            let index = covers.binary_search(obligation).unwrap();
            entry["witnesses"] = json!([
                {
                    "path": format!("reports/generated/formal/{index}/trace.vcd"),
                    "bytes": 100,
                    "sha256": "3".repeat(64),
                },
                {
                    "path": format!("reports/generated/formal/{index}/trace.yw"),
                    "bytes": 100,
                    "sha256": "4".repeat(64),
                },
            ]);
        }
        results.insert(obligation.clone(), entry);
    }
    json!({
        "schema": "arm7tdmis-formal-v1",
        "status": "passed",
        "failure": null,
        "git": {"commit": "a".repeat(40), "dirty": false},
        "toolchain": manifest["toolchain"],
        "required_proofs": proofs,
        "proven_proofs": proofs,
        "required_covers": covers,
        "covered_covers": covers,
        "uncovered_covers": [],
        "results": results,
    })
}

#[test]
fn formal_release_gate_rejects_every_proof_cover_and_provenance_weakening() {
    let manifest = manifest("verification/formal_requirements.json");
    let report = formal_report(&manifest);
    let commit = "a".repeat(40);
    validate_formal(&report, &manifest, &commit).unwrap();
    let proof0 = report["required_proofs"][0].as_str().unwrap().to_owned();
    let proof1 = report["required_proofs"][1].as_str().unwrap().to_owned();
    let cover0 = report["required_covers"][0].as_str().unwrap().to_owned();
    let cover1 = report["required_covers"][1].as_str().unwrap().to_owned();
    let cover2 = report["required_covers"][2].as_str().unwrap().to_owned();
    for mutation in [
        "failed",
        "dirty",
        "stale",
        "tool",
        "proof",
        "cover",
        "uncovered",
        "result",
        "log_hash",
        "source_hash",
        "witness",
        "witness_hash",
    ] {
        let mut weakened = report.clone();
        match mutation {
            "failed" => weakened["status"] = json!("failed"),
            "dirty" => weakened["git"]["dirty"] = json!(true),
            "stale" => weakened["git"]["commit"] = json!("b".repeat(40)),
            "tool" => weakened["toolchain"]["version"] = json!("latest"),
            "proof" => {
                weakened["proven_proofs"].as_array_mut().unwrap().pop();
            }
            "cover" => {
                weakened["covered_covers"].as_array_mut().unwrap().pop();
            }
            "uncovered" => {
                weakened["uncovered_covers"] = json!([cover0]);
            }
            "result" => weakened["results"][&proof0]["status"] = json!("failed"),
            "log_hash" => weakened["results"][&proof1]["log"]["sha256"] = json!(""),
            "source_hash" => weakened["results"][&cover0]["source"]["sha256"] = json!(""),
            "witness" => {
                weakened["results"][&cover1]["witnesses"]
                    .as_array_mut()
                    .unwrap()
                    .pop();
            }
            "witness_hash" => weakened["results"][&cover2]["witnesses"][0]["sha256"] = json!(""),
            _ => unreachable!(),
        }
        assert!(
            validate_formal(&weakened, &manifest, &commit).is_err(),
            "accepted formal mutation: {mutation}"
        );
    }
}

fn functional_report(manifest: &Value) -> Value {
    let mut required = manifest["required_bin_groups"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|group| {
            let group_id = group["id"].as_str().unwrap();
            group["members"]
                .as_array()
                .unwrap()
                .iter()
                .map(move |member| format!("{group_id}.{}", member.as_str().unwrap()))
        })
        .collect::<Vec<_>>();
    required.sort();
    let reference: Value =
        serde_json::from_str(&crate::functional_coverage::functional_reference(&[]).unwrap())
            .unwrap();
    let evidence = manifest["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            (
                entry["phase"].as_str().unwrap().to_owned(),
                json!({"status": "passed"}),
            )
        })
        .collect::<Map<_, _>>();
    json!({
        "schema": "arm7tdmis-functional-coverage-v1",
        "status": "passed",
        "git": {"commit": "abc123", "dirty": false},
        "domains": reference["domains"],
        "required_bin_count": required.len(),
        "required_bins": required,
        "covered_bin_count": required.len(),
        "covered_bins": required,
        "uncovered_bins": [],
        "exclusions": manifest["exclusions"],
        "evidence": evidence,
    })
}

#[test]
fn functional_release_gate_rejects_every_domain_bin_and_evidence_weakening() {
    let manifest = manifest("verification/functional_coverage.json");
    let report = functional_report(&manifest);
    functional::validate(&report, &manifest, "abc123").unwrap();
    let first_evidence = report["evidence"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    for mutation in [
        "dirty",
        "commit",
        "required",
        "covered",
        "uncovered",
        "count",
        "domain",
        "evidence",
        "citation",
        "group",
    ] {
        let mut weakened = report.clone();
        let mut weakened_manifest = manifest.clone();
        match mutation {
            "dirty" => weakened["git"]["dirty"] = json!(true),
            "commit" => weakened["git"]["commit"] = json!("def456"),
            "required" => {
                weakened["required_bins"].as_array_mut().unwrap().pop();
            }
            "covered" => {
                weakened["covered_bins"].as_array_mut().unwrap().pop();
            }
            "uncovered" => weakened["uncovered_bins"] = json!([weakened["required_bins"][0]]),
            "count" => {
                let count = weakened["covered_bin_count"].as_u64().unwrap();
                weakened["covered_bin_count"] = json!(count - 1);
            }
            "domain" => weakened["domains"]["thumb_words"]["observed"] = json!(65_535),
            "evidence" => {
                weakened["evidence"]
                    .as_object_mut()
                    .unwrap()
                    .remove(&first_evidence);
            }
            "citation" => weakened_manifest["exclusions"][0]["citation"] = json!(""),
            "group" => {
                weakened_manifest["required_bin_groups"]
                    .as_array_mut()
                    .unwrap()
                    .pop();
            }
            _ => unreachable!(),
        }
        assert!(
            functional::validate(&weakened, &weakened_manifest, "abc123").is_err(),
            "accepted functional-coverage mutation: {mutation}"
        );
    }
}

#[test]
fn release_closure_fixtures_cover_the_exact_manifest_sets() {
    let formal = manifest("verification/formal_requirements.json");
    let formal_report = formal_report(&formal);
    let proofs = formal_report["required_proofs"].as_array().unwrap();
    let covers = formal_report["required_covers"].as_array().unwrap();
    assert_eq!(proofs.len(), 14);
    assert_eq!(covers.len(), 77);
    assert_eq!(
        proofs
            .iter()
            .map(|entry| entry.as_str().unwrap())
            .collect::<BTreeSet<_>>()
            .len(),
        proofs.len()
    );
    assert_eq!(
        covers
            .iter()
            .map(|entry| entry.as_str().unwrap())
            .collect::<BTreeSet<_>>()
            .len(),
        covers.len()
    );

    let functional = manifest("verification/functional_coverage.json");
    let functional_report = functional_report(&functional);
    assert_eq!(functional_report["required_bin_count"], 234);
    assert_eq!(functional_report["uncovered_bins"], json!([]));
}
