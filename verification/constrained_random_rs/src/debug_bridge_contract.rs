//! Public synchronous debug bridge release contract.

use std::fs;

use crate::campaign::repo_root;

fn read(relative: &str) -> String {
    fs::read_to_string(repo_root().join(relative))
        .unwrap_or_else(|error| panic!("cannot read {relative}: {error}"))
}

#[test]
fn bridge_is_public_synchronous_and_fail_closed() {
    let bridge = read("rtl/jtag/arm7tdmis_sync_debug_port.sv");
    let wrapper = read("rtl/top/arm7tdmi_mister.sv");
    let package = read("fpga/arm7tdmi_mister.qip");
    for signal in [
        "STEP_VALID",
        "STEP_READY",
        "STEP_RSP_VALID",
        "STEP_RSP_READY",
        "STEP_TDO_OE",
        "DBGTCKEN",
        "PORT_ENABLE",
    ] {
        assert!(bridge.contains(signal), "bridge omits {signal}");
    }
    assert!(bridge.contains("assign DBGTCKEN   = step_accept"));
    assert!(bridge.contains("response_valid_q"));
    assert!(wrapper.contains("arm7tdmis_sync_debug_port u_debug_transport"));
    assert!(wrapper.contains("DBG_STEP_VALID"));
    assert!(package.contains("arm7tdmis_sync_debug_port.sv"));
}

#[test]
fn bridge_protocol_and_full_scan_evidence_are_mandatory() {
    let transport = read("tb/unit/sync_debug_port_tb.sv");
    let makefile = read("scripts/Makefile");
    let tasks = read("TASKS.md");
    for evidence in [
        "response was blocked",
        "duplicate clock",
        "response replacement",
        "disabled transport",
        "IDCODE mismatch",
        "$fatal",
        "PASS",
    ] {
        assert!(transport.contains(evidence), "transport omits {evidence}");
    }
    assert!(makefile.contains("sync_debug_port"));
    for test in [
        "debug_register_scan",
        "debug_system_speed",
        "debug_monitor_mode",
        "cp14_dcc",
    ] {
        assert!(makefile.contains(test), "Makefile omits {test}");
    }
    assert!(tasks.contains("- [x] **JTAG-006:**"));
    let block = tasks
        .split_once("**JTAG-006:**")
        .unwrap()
        .1
        .split_once("**ETM-001:**")
        .unwrap()
        .0;
    for evidence in ["project-specific bridge", "sync_debug_port_tb.sv"] {
        assert!(block.contains(evidence), "JTAG-006 omits {evidence}");
    }
}

#[test]
fn documentation_explains_physical_pod_boundary() {
    let debug = read("docs/DEBUG.md");
    let support = read("docs/SUPPORT.md");
    let limitations = read("docs/LIMITATIONS.md");
    for evidence in [
        "not a TCK synchronizer",
        "no RTCK claim",
        "asynchronous JTAG pod",
        "separately specified and verified CDC bridge",
        "same-clock",
    ] {
        assert!(debug.contains(evidence), "debug docs omit {evidence}");
    }
    assert!(support.contains("Synchronous debug-step bridge"));
    assert!(!limitations.contains("JTAG-006"));
}
