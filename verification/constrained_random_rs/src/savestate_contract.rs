//! Versioned MiSTer architectural save-state interface contracts.

use std::fs;

use crate::campaign::repo_root;

fn read(relative: &str) -> String {
    fs::read_to_string(repo_root().join(relative))
        .unwrap_or_else(|error| panic!("cannot read {relative}: {error}"))
}

#[test]
fn state_interface_is_opt_in_and_has_frozen_word_schema() {
    let wrapper = read("rtl/top/arm7tdmi_mister.sv");
    let docs_path = repo_root().join("docs/SAVESTATE.md");
    assert!(docs_path.is_file(), "missing docs/SAVESTATE.md");
    let docs = read("docs/SAVESTATE.md");
    assert!(wrapper.contains("ARM7TDMIS_SAVE_STATE"));
    for signal in [
        "STATE_REQUEST",
        "STATE_READY",
        "STATE_WRITE",
        "STATE_INDEX",
        "STATE_WDATA",
        "STATE_RDATA",
    ] {
        assert!(wrapper.contains(signal), "wrapper omits {signal}");
        assert!(docs.contains(signal), "save-state docs omit {signal}");
    }
    assert!(wrapper.contains("STATE_SCHEMA_VERSION"));
    assert!(wrapper.contains("STATE_WORDS"));
    assert!(docs.contains("37"));
    assert!(docs.contains("Thumb BL"));
}

#[test]
fn determinism_test_covers_all_words_and_difficult_boundaries() {
    let path = repo_root().join("tb/integration/arm7tdmis_mister_savestate_tb.sv");
    assert!(path.is_file(), "missing {}", path.display());
    let bench = read("tb/integration/arm7tdmis_mister_savestate_tb.sv");
    for evidence in [
        "STATE_WORDS",
        "snapshot",
        "mutated",
        "trace_first",
        "trace_second",
        "MEM_READY",
        "CPU_CE",
        "THUMB_SUFFIX_PC",
        "32'h0000_0084",
        "32'h0000_0085",
    ] {
        assert!(
            bench.contains(evidence),
            "save-state bench omits {evidence}"
        );
    }
    assert!(bench.contains("[mister_savestate] PASS"));
    assert!(bench.contains("$fatal"));
}

#[test]
fn save_state_test_is_registered_with_feature_define() {
    let makefile = read("scripts/Makefile");
    let regression = read("verification/constrained_random_rs/src/regression.rs");
    let tasks = read("TASKS.md");
    assert!(makefile.contains("mister_savestate"));
    assert!(makefile.contains("INTEG_EXTRA_FLAGS_mister_savestate := -DARM7TDMIS_SAVE_STATE"));
    // The runner reads the Make manifest rather than duplicating individual benches.
    assert!(regression.contains("args.integrations = value.split_whitespace()"));
    assert!(regression.contains("format!(\"integ-{integration}\")"));
    assert!(tasks.contains("- [x] **MIST-006:**"));
    let block = tasks
        .split_once("**MIST-006:**")
        .unwrap()
        .1
        .split_once("**MIST-007:**")
        .unwrap()
        .0;
    assert!(block.contains("arm7tdmis_mister_savestate_tb.sv"));
    assert!(block.contains("docs/SAVESTATE.md"));
}
