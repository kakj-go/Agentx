use std::{collections::BTreeSet, fs, path::PathBuf};

use serde_json::Value;

#[test]
fn frozen_reference_fixtures_are_schema_valid_and_cover_the_gate() {
    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/plan3/fixtures/agent-core");
    let schema: Value = serde_json::from_slice(
        &fs::read(root.join("fixture.schema.json")).expect("fixture schema"),
    )
    .expect("valid schema json");
    let validator = jsonschema::validator_for(&schema).expect("valid fixture schema");
    let source = fs::read_to_string(root.join("cases.jsonl")).expect("fixture cases");
    let mut capabilities = BTreeSet::new();
    let mut case_ids = BTreeSet::new();
    for (index, line) in source
        .lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
    {
        let value: Value = serde_json::from_str(line)
            .unwrap_or_else(|error| panic!("fixture line {} is invalid JSON: {error}", index + 1));
        if let Err(error) = validator.validate(&value) {
            panic!("fixture line {} violates schema: {error}", index + 1);
        }
        let case_id = value["case_id"].as_str().expect("case id");
        assert!(
            case_ids.insert(case_id.to_owned()),
            "duplicate case {case_id}"
        );
        capabilities.insert(value["capability"].as_str().expect("capability").to_owned());
    }
    for required in [
        "agent_loop",
        "tool_loop",
        "tool_batch",
        "steering",
        "follow_up",
        "threshold_compaction",
        "overflow_compaction",
        "session_resume",
        "tool_error",
        "model_error",
        "cancel",
        "budget",
        "sandbox_tool_registration",
        "effect_recovery",
    ] {
        assert!(
            capabilities.contains(required),
            "missing capability {required}"
        );
    }
}
