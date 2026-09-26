//! Release-validator mutation contracts. These run in the Rust harness, so
//! validating a report no longer needs the former Python-to-Cargo test client.

use serde_json::{json, Map, Value};

use super::{
    validate_public_suite, validate_random_events, validate_soak, validate_table7,
    RANDOM_EVENT_BINS, RANDOM_EVENT_CLASS_BINS, TABLE7_REQUIRED_CROSSES,
};

const SIGNATURE: &str = "0xe8d71fb2";

fn public_suite_manifest() -> Value {
    json!({
        "schema": "arm7tdmis-public-suites-v1",
        "upstream": {
            "url": "https://github.com/jsmolka/gba-suite.git",
            "commit": "a".repeat(40),
            "tree": "b".repeat(40),
            "license_path": "LICENSE",
            "license_spdx": "MIT",
            "license_sha256": "c".repeat(64),
        },
        "patches": [{
            "operation": "zero header",
            "range": "0x00000004..0x000000bf",
            "reason": "remove non-executable metadata",
        }],
        "suites": {
            "arm": {
                "path": "arm/arm.gba",
                "upstream_sha256": "d".repeat(64),
                "patched_sha256": "1".repeat(64),
                "idle_pc": "0x08001ec4",
                "result_register": 12,
                "expected_vram_signature": SIGNATURE,
                "minimum_retirements": 8_000,
                "word_patches": [{
                    "offset": "0x00000cd4",
                    "expected_word": "0xe3a08020",
                    "replacement_word": "0xea000016",
                }],
            },
            "thumb": {
                "path": "thumb/thumb.gba",
                "upstream_sha256": "e".repeat(64),
                "patched_sha256": "2".repeat(64),
                "idle_pc": "0x08000aac",
                "result_register": 7,
                "expected_vram_signature": SIGNATURE,
                "minimum_retirements": 6_000,
                "word_patches": [],
            },
        },
    })
}

fn public_suite_result(name: &str, definition: &Value) -> Value {
    let (retirements, arm_retirements, thumb_retirements, rom_words, bytes, hash) = if name == "arm"
    {
        (8_596, 8_588, 8, 2_206, 19_854, "3")
    } else {
        (6_691, 6_152, 539, 920, 8_280, "4")
    };
    json!({
        "status": "passed",
        "upstream_path": definition["path"],
        "upstream_sha256": definition["upstream_sha256"],
        "patched_sha256": definition["patched_sha256"],
        "idle_pc": definition["idle_pc"],
        "result_register": definition["result_register"],
        "expected_vram_signature": definition["expected_vram_signature"],
        "word_patches": definition["word_patches"],
        "metrics": {
            "retirements": retirements,
            "arm_retirements": arm_retirements,
            "thumb_retirements": thumb_retirements,
            "rom_words": rom_words,
        },
        "reproducer": ["simulator", format!("+ROM_HEX={name}.hex")],
        "artifacts": {
            format!("{name}.hex"): {
                "path": format!("{name}/{name}.hex"),
                "bytes": bytes,
                "sha256": hash.repeat(64),
            },
        },
    })
}

fn public_suite_report(manifest: &Value) -> Value {
    let mut upstream = manifest["upstream"].clone();
    upstream["acquisition_commands"] =
        json!([["git", "fetch", "--depth=1", "origin", "a".repeat(40)]]);
    upstream["license_artifact"] = json!({
        "path": "upstream-LICENSE",
        "bytes": 1_070,
        "sha256": manifest["upstream"]["license_sha256"],
    });
    json!({
        "schema": "arm7tdmis-public-suite-v1",
        "status": "passed",
        "failure": null,
        "git": {"commit": "a".repeat(40), "dirty": false},
        "upstream": upstream,
        "patches": manifest["patches"],
        "suite_count": 2,
        "total_retirements": 15_287,
        "suites": {
            "arm": public_suite_result("arm", &manifest["suites"]["arm"]),
            "thumb": public_suite_result("thumb", &manifest["suites"]["thumb"]),
        },
    })
}

#[test]
fn public_suite_release_gate_rejects_every_provenance_and_execution_weakening() {
    let manifest = public_suite_manifest();
    let report = public_suite_report(&manifest);
    let commit = "a".repeat(40);
    validate_public_suite(&report, &manifest, &commit).unwrap();

    for mutation in [
        "schema",
        "dirty",
        "commit",
        "upstream",
        "patches",
        "retirements",
        "signature",
        "thumb_execution",
        "reproducer",
        "artifacts",
        "total",
    ] {
        let mut weakened = report.clone();
        match mutation {
            "schema" => weakened["schema"] = json!("wrong"),
            "dirty" => weakened["git"]["dirty"] = json!(true),
            "commit" => weakened["git"]["commit"] = json!("f".repeat(40)),
            "upstream" => weakened["upstream"]["tree"] = json!("f".repeat(40)),
            "patches" => weakened["patches"] = json!([]),
            "retirements" => weakened["suites"]["arm"]["metrics"]["retirements"] = json!(7_999),
            "signature" => {
                weakened["suites"]["thumb"]["expected_vram_signature"] = json!("0x00000000")
            }
            "thumb_execution" => {
                weakened["suites"]["thumb"]["metrics"]["thumb_retirements"] = json!(0)
            }
            "reproducer" => weakened["suites"]["arm"]["reproducer"] = json!([]),
            "artifacts" => weakened["suites"]["thumb"]["artifacts"] = json!({}),
            "total" => weakened["total_retirements"] = json!(15_286),
            _ => unreachable!(),
        }
        assert!(
            validate_public_suite(&weakened, &manifest, &commit).is_err(),
            "accepted public-suite mutation: {mutation}"
        );
    }

    for mutation in ["schema", "license"] {
        let mut untrusted = manifest.clone();
        match mutation {
            "schema" => untrusted["schema"] = json!("wrong"),
            "license" => untrusted["upstream"]["license_spdx"] = json!("unknown"),
            _ => unreachable!(),
        }
        assert!(
            validate_public_suite(&report, &untrusted, &commit).is_err(),
            "accepted public-suite manifest mutation: {mutation}"
        );
    }
}

fn table7_manifest() -> Value {
    let mut crosses = Map::new();
    for (index, name) in TABLE7_REQUIRED_CROSSES.iter().enumerate() {
        let rows = index + 1;
        crosses.insert(
            (*name).into(),
            json!({
                "description": format!("required cross {name}"),
                "dimensions": {
                    "instruction_class": [format!("class_{rows}")],
                    "policy": ["applicable"],
                },
                "minimum_rows": rows,
                "evidence": [{
                    "phase": format!("integration-evidence_{rows}"),
                    "source": format!("tb/integration/evidence_{rows}_tb.sv"),
                    "marker": format!(r"\[evidence_{rows}\] PASS"),
                    "expected_rows": rows,
                }],
            }),
        );
    }
    json!({
        "schema": "arm7tdmis-table7-cross-map-v1",
        "required_crosses": TABLE7_REQUIRED_CROSSES,
        "crosses": crosses,
    })
}

fn table7_report(manifest: &Value) -> Value {
    let mut crosses = Map::new();
    for (index, name) in TABLE7_REQUIRED_CROSSES.iter().enumerate() {
        let rows = index + 1;
        let definition = &manifest["crosses"][name];
        let mut evidence = definition["evidence"][0].clone();
        evidence["status"] = json!("passed");
        evidence["log"] = json!({
            "path": format!("reports/generated/logs/evidence_{rows}.log"),
            "bytes": 100 + rows,
            "sha256": format!("{rows:x}").repeat(64),
        });
        evidence["source_artifact"] = json!({
            "path": definition["evidence"][0]["source"],
            "bytes": 1_000 + rows,
            "sha256": "abcdef0123456789".chars().nth(rows).unwrap().to_string().repeat(64),
        });
        crosses.insert(
            (*name).into(),
            json!({
                "description": definition["description"],
                "dimensions": definition["dimensions"],
                "minimum_rows": rows,
                "observed_rows": rows,
                "evidence": [evidence],
            }),
        );
    }
    json!({
        "schema": "arm7tdmis-table7-cross-v1",
        "status": "passed",
        "failure": null,
        "git": {"commit": "a".repeat(40), "dirty": false},
        "required_crosses": TABLE7_REQUIRED_CROSSES,
        "covered_crosses": TABLE7_REQUIRED_CROSSES,
        "missing_crosses": [],
        "cross_count": TABLE7_REQUIRED_CROSSES.len(),
        "total_minimum_rows": (1..=TABLE7_REQUIRED_CROSSES.len()).sum::<usize>(),
        "total_observed_rows": (1..=TABLE7_REQUIRED_CROSSES.len()).sum::<usize>(),
        "crosses": crosses,
    })
}

#[test]
fn table7_release_gate_rejects_every_cross_and_hash_weakening() {
    let manifest = table7_manifest();
    let report = table7_report(&manifest);
    let commit = "a".repeat(40);
    validate_table7(&report, &manifest, &commit).unwrap();

    let names = TABLE7_REQUIRED_CROSSES;
    for mutation in [
        "schema",
        "failed",
        "dirty",
        "commit",
        "required",
        "covered",
        "missing",
        "count",
        "rows",
        "dimensions",
        "evidence",
        "evidence_status",
        "log_hash",
        "source_hash",
        "minimum_total",
        "observed_total",
    ] {
        let mut weakened = report.clone();
        match mutation {
            "schema" => weakened["schema"] = json!("wrong"),
            "failed" => weakened["status"] = json!("failed"),
            "dirty" => weakened["git"]["dirty"] = json!(true),
            "commit" => weakened["git"]["commit"] = json!("f".repeat(40)),
            "required" => {
                weakened["required_crosses"].as_array_mut().unwrap().pop();
            }
            "covered" => {
                weakened["covered_crosses"].as_array_mut().unwrap().pop();
            }
            "missing" => {
                weakened["missing_crosses"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!(names[0]));
            }
            "count" => weakened["cross_count"] = json!(8),
            "rows" => weakened["crosses"][names[0]]["observed_rows"] = json!(0),
            "dimensions" => {
                weakened["crosses"][names[1]]["dimensions"] = json!({"policy": ["weakened"]})
            }
            "evidence" => {
                weakened["crosses"][names[2]]["evidence"][0]["phase"] =
                    json!("integration-untrusted")
            }
            "evidence_status" => {
                weakened["crosses"][names[3]]["evidence"][0]["status"] = json!("failed")
            }
            "log_hash" => weakened["crosses"][names[4]]["evidence"][0]["log"]["sha256"] = json!(""),
            "source_hash" => {
                weakened["crosses"][names[5]]["evidence"][0]["source_artifact"]["sha256"] =
                    json!("")
            }
            "minimum_total" => weakened["total_minimum_rows"] = json!(44),
            "observed_total" => weakened["total_observed_rows"] = json!(44),
            _ => unreachable!(),
        }
        assert!(
            validate_table7(&weakened, &manifest, &commit).is_err(),
            "accepted Chapter 7 report mutation: {mutation}"
        );
    }

    for mutation in ["schema", "cross", "minimum", "dimensions", "evidence"] {
        let mut untrusted = manifest.clone();
        match mutation {
            "schema" => untrusted["schema"] = json!("wrong"),
            "cross" => {
                untrusted["crosses"]
                    .as_object_mut()
                    .unwrap()
                    .remove(names[0]);
            }
            "minimum" => untrusted["crosses"][names[1]]["minimum_rows"] = json!(0),
            "dimensions" => untrusted["crosses"][names[2]]["dimensions"] = json!({}),
            "evidence" => untrusted["crosses"][names[3]]["evidence"] = json!([]),
            _ => unreachable!(),
        }
        assert!(
            validate_table7(&report, &untrusted, &commit).is_err(),
            "accepted Chapter 7 manifest mutation: {mutation}"
        );
    }
}

#[test]
fn soak_release_gate_rejects_weakened_sanitizer_and_seed_evidence() {
    let seeds = (1..=256)
        .map(|seed| {
            json!({
                "seed": seed,
                "status": "passed",
                "exit_code": 0,
                "pass_marker_found": true,
            })
        })
        .collect::<Vec<_>>();
    let report = json!({
        "schema": "arm7tdmis-soak-v1",
        "status": "passed",
        "git": {"commit": "abc123", "dirty": false},
        "configuration": {
            "seed_count": 256,
            "timeout_seconds": 30.0,
            "x_assignment": "unique",
            "x_initialization": "unique",
            "sanitizers": ["address", "undefined"],
            "environment": {
                "ASAN_OPTIONS": "halt_on_error=1",
                "UBSAN_OPTIONS": "halt_on_error=1",
            },
        },
        "tools": {"binary_dependencies": ["libasan.so.8", "libubsan.so.1"]},
        "completed_seed_count": 256,
        "seeds": seeds,
        "failures": [],
    });
    validate_soak(&report, "abc123").unwrap();

    for mutation in ["dirty", "duplicate", "unsanitized", "failed"] {
        let mut weakened = report.clone();
        match mutation {
            "dirty" => weakened["git"]["dirty"] = json!(true),
            "duplicate" => weakened["seeds"][1]["seed"] = json!(1),
            "unsanitized" => weakened["tools"]["binary_dependencies"] = json!([]),
            "failed" => weakened["seeds"][0]["status"] = json!("failed"),
            _ => unreachable!(),
        }
        assert!(
            validate_soak(&weakened, "abc123").is_err(),
            "accepted soak mutation: {mutation}"
        );
    }
}

#[test]
fn random_event_release_gate_rejects_weak_seed_and_bin_evidence() {
    let seeds = (1..=32)
        .map(|seed| {
            json!({
                "seed": seed,
                "status": "passed",
                "exit_code": 0,
                "pass_marker_found": true,
                "decision_count": 512,
                "class_bins": RANDOM_EVENT_CLASS_BINS,
                "class_stall_bins": RANDOM_EVENT_CLASS_BINS,
                "event_bins": RANDOM_EVENT_BINS,
                "reproducer": ["simulator", format!("+SEED={seed}")],
            })
        })
        .collect::<Vec<_>>();
    let report = json!({
        "schema": "arm7tdmis-random-events-v1",
        "status": "passed",
        "git": {"commit": "abc123", "dirty": false},
        "configuration": {
            "seed_count": 32,
            "minimum_decisions_per_seed": 256,
        },
        "required_class_bins": RANDOM_EVENT_CLASS_BINS,
        "required_event_bins": RANDOM_EVENT_BINS,
        "covered_class_bins": RANDOM_EVENT_CLASS_BINS,
        "covered_class_stall_bins": RANDOM_EVENT_CLASS_BINS,
        "covered_event_bins": RANDOM_EVENT_BINS,
        "completed_seed_count": 32,
        "total_decision_count": 32 * 512,
        "seeds": seeds,
        "failures": [],
    });
    validate_random_events(&report, "abc123").unwrap();

    for mutation in [
        "dirty",
        "commit",
        "seed_count",
        "duplicate_seed",
        "short_run",
        "class",
        "stall",
        "event",
        "failed_seed",
        "total",
    ] {
        let mut weakened = report.clone();
        match mutation {
            "dirty" => weakened["git"]["dirty"] = json!(true),
            "commit" => weakened["git"]["commit"] = json!("def456"),
            "seed_count" => weakened["configuration"]["seed_count"] = json!(31),
            "duplicate_seed" => weakened["seeds"][1]["seed"] = json!(1),
            "short_run" => weakened["seeds"][0]["decision_count"] = json!(255),
            "class" => {
                weakened["covered_class_bins"].as_array_mut().unwrap().pop();
            }
            "stall" => {
                weakened["seeds"][0]["class_stall_bins"]
                    .as_array_mut()
                    .unwrap()
                    .pop();
            }
            "event" => {
                weakened["covered_event_bins"].as_array_mut().unwrap().pop();
            }
            "failed_seed" => weakened["seeds"][0]["status"] = json!("failed"),
            "total" => weakened["total_decision_count"] = json!(32 * 512 - 1),
            _ => unreachable!(),
        }
        assert!(
            validate_random_events(&weakened, "abc123").is_err(),
            "accepted random-event mutation: {mutation}"
        );
    }
}
