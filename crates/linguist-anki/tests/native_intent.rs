use linguist_anki::native::validate_create_note_intent;
use linguist_core::model;
use serde_json::{Value, json};
use uuid::Uuid;

fn intent(grammar: bool, operation: Uuid, approved: &str) -> Value {
    let model = if grammar {
        model::grammar()
    } else {
        model::vocabulary()
    };
    let mut fields = serde_json::Map::new();
    for field in model.fields {
        fields.insert(field, json!(""));
    }
    fields.insert("Meaning".into(), json!("meaning"));
    if grammar {
        fields.insert("Language".into(), json!("ja"));
        for field in ["Pattern", "Formation", "Examples", "UseKey"] {
            fields.insert(field.into(), json!("source-backed value"));
        }
    } else {
        fields.insert("Expression".into(), json!("食べる"));
    }
    let marker = format!("lab_op_{}", operation.simple());
    json!({
        "schema_version":1,
        "variant":"create_note",
        "body":{
            "model_name":model.name,
            "model_manifest_digest":"a".repeat(64),
            "deck_id":"9007199254740991",
            "fields":fields,
            "tags":[marker.clone(),"reviewed"],
            "marker_tag":marker,
            "source_plan_digest":approved,
            "checkpoint_digest":"b".repeat(64),
            "binding":{"profile_fingerprint":"c".repeat(64),"path_fingerprint":"d".repeat(64)},
            "expected_absent":true
        }
    })
}

fn validate(value: &Value, operation: Uuid, approved: &str) -> bool {
    validate_create_note_intent(value.to_string().as_bytes(), operation, approved).is_ok()
}

#[test]
fn exact_vocabulary_and_grammar_intents_are_structurally_accepted() {
    let operation = Uuid::new_v4();
    let approved = format!("lab-jcs-v1:plan:{}", "e".repeat(64));
    for grammar in [false, true] {
        let value = intent(grammar, operation, &approved);
        let parsed =
            validate_create_note_intent(value.to_string().as_bytes(), operation, &approved)
                .unwrap();
        assert_eq!(parsed.body.model_name, value["body"]["model_name"]);
        assert_eq!(parsed.body.source_plan_digest, approved);
    }
}

#[test]
fn altered_identity_fields_tags_and_grammar_requirements_fail_closed() {
    let operation = Uuid::new_v4();
    let approved = format!("lab-jcs-v1:plan:{}", "e".repeat(64));
    let valid = intent(false, operation, &approved);
    let mut changed = valid.clone();
    changed["body"]["source_plan_digest"] = json!(format!("lab-jcs-v1:plan:{}", "f".repeat(64)));
    assert!(!validate(&changed, operation, &approved));
    let mut changed = valid.clone();
    changed["body"]["binding"]["extra"] = json!(true);
    assert!(!validate(&changed, operation, &approved));
    let mut changed = valid.clone();
    changed["body"]["deck_id"] = json!("09007199254740991");
    assert!(!validate(&changed, operation, &approved));
    let mut changed = valid.clone();
    changed["body"]["fields"]["Expression"] = json!(" ");
    assert!(!validate(&changed, operation, &approved));
    let mut changed = valid.clone();
    changed["body"]["fields"]["EnableProduction"] = json!("yes");
    assert!(!validate(&changed, operation, &approved));
    let mut changed = valid.clone();
    changed["body"]["fields"]
        .as_object_mut()
        .unwrap()
        .remove("Kanji");
    assert!(!validate(&changed, operation, &approved));
    let mut changed = valid.clone();
    changed["body"]["fields"]["Language"] = json!("ja");
    assert!(!validate(&changed, operation, &approved));
    let mut changed = valid.clone();
    changed["body"]["tags"] = json!([valid["body"]["marker_tag"], "reviewed", "reviewed"]);
    assert!(!validate(&changed, operation, &approved));
    let mut changed = intent(true, operation, &approved);
    changed["body"]["fields"]["Examples"] = json!("");
    assert!(!validate(&changed, operation, &approved));
    assert!(!validate(&valid, Uuid::new_v4(), &approved));
}

#[test]
fn duplicate_json_keys_oversized_values_and_unknown_variants_fail() {
    let operation = Uuid::new_v4();
    let approved = format!("lab-jcs-v1:plan:{}", "e".repeat(64));
    assert!(
        validate_create_note_intent(
            b"{\"schema_version\":1,\"schema_version\":1,\"variant\":\"create_note\",\"body\":{}}",
            operation,
            &approved
        )
        .is_err()
    );
    let mut changed = intent(false, operation, &approved);
    changed["body"]["fields"]["UsageExamples"] = json!("x".repeat(262_145));
    assert!(!validate(&changed, operation, &approved));
    let mut changed = intent(false, operation, &approved);
    changed["variant"] = json!("update_note");
    assert!(
        validate_create_note_intent(changed.to_string().as_bytes(), operation, &approved)
            .unwrap_err()
            .contains("CAPABILITY_UNAVAILABLE")
    );
}
