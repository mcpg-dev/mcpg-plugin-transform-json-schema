use mcpg_plugin_protocol::{PluginContext, PluginIdentity, TransformResult};
use mcpg_plugin_sdk::ffi::SyncTransform;
use serde_json::json;

use super::JsonSchemaTransform;

fn ctx() -> PluginContext {
    PluginContext {
        request_id: "t".into(),
        session_id: None,
        tool_name: "x".into(),
        surface: "tool".into(),
        identity: PluginIdentity {
            kind: "anonymous".into(),
            trust_level: "unauthenticated".into(),
            subject_id: None,
            auth_provider: None,
            issuer: None,
            roles: Vec::new(),
            groups: Vec::new(),
            scopes: Vec::new(),
            attributes: Default::default(),
        },
        transport: "http".into(),
    }
}

fn error_msg(r: TransformResult) -> String {
    match r {
        TransformResult::Error { message } => message,
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn valid_value_passes_through_unchanged() {
    let p = JsonSchemaTransform::new("{}");
    let cfg = json!({ "schema": { "type": "object", "required": ["name"] } });
    assert!(matches!(
        p.transform_result(&ctx(), &json!({ "name": "a" }), &cfg),
        TransformResult::Unchanged
    ));
}

#[test]
fn invalid_value_returns_error_with_details() {
    let p = JsonSchemaTransform::new("{}");
    let cfg = json!({ "schema": { "type": "object", "required": ["name"] } });
    let msg = error_msg(p.transform_result(&ctx(), &json!({}), &cfg));
    assert!(msg.contains("name") || msg.contains("required"), "{msg}");
}

#[test]
fn type_mismatch_is_error() {
    let p = JsonSchemaTransform::new("{}");
    let cfg = json!({ "schema": { "type": "string" } });
    assert!(matches!(
        p.transform_result(&ctx(), &json!(42), &cfg),
        TransformResult::Error { .. }
    ));
}

#[test]
fn pointer_validates_subfield_only() {
    let p = JsonSchemaTransform::new("{}");
    let cfg = json!({ "schema": { "type": "array" }, "pointer": "/data" });
    // /data is an array → valid; the non-array /meta is ignored.
    assert!(matches!(
        p.transform_result(&ctx(), &json!({ "data": [1, 2], "meta": { "x": 1 } }), &cfg),
        TransformResult::Unchanged
    ));
    // /data is a string → invalid.
    assert!(matches!(
        p.transform_result(&ctx(), &json!({ "data": "oops" }), &cfg),
        TransformResult::Error { .. }
    ));
}

#[test]
fn pointer_not_found_is_error() {
    let p = JsonSchemaTransform::new("{}");
    let cfg = json!({ "schema": { "type": "array" }, "pointer": "/missing" });
    let msg = error_msg(p.transform_result(&ctx(), &json!({ "data": [] }), &cfg));
    assert!(msg.contains("not found"), "{msg}");
}

#[test]
fn invalid_schema_is_error() {
    let p = JsonSchemaTransform::new("{}");
    // `type` must be a string/array of strings, not a number.
    let cfg = json!({ "schema": { "type": 123 } });
    let msg = error_msg(p.transform_result(&ctx(), &json!({}), &cfg));
    assert!(msg.contains("JSON Schema"), "{msg}");
}

#[test]
fn phase_result_skips_arguments_phase() {
    let p = JsonSchemaTransform::new("{}");
    let cfg = json!({ "schema": { "type": "string" }, "phase": "result" });
    // Arguments phase is gated out → Unchanged even though 42 is invalid.
    assert!(matches!(
        p.transform_arguments(&ctx(), &json!(42), &cfg),
        TransformResult::Unchanged
    ));
    // Result phase runs → Error.
    assert!(matches!(
        p.transform_result(&ctx(), &json!(42), &cfg),
        TransformResult::Error { .. }
    ));
}

#[test]
fn phase_both_fires_on_both() {
    let p = JsonSchemaTransform::new("{}");
    let cfg = json!({ "schema": { "type": "string" } });
    assert!(matches!(
        p.transform_arguments(&ctx(), &json!(42), &cfg),
        TransformResult::Error { .. }
    ));
    assert!(matches!(
        p.transform_result(&ctx(), &json!(42), &cfg),
        TransformResult::Error { .. }
    ));
}

#[test]
fn unknown_config_key_is_rejected() {
    let p = JsonSchemaTransform::new("{}");
    let cfg = json!({ "schema": { "type": "string" }, "schemaa": 1 });
    let msg = error_msg(p.transform_result(&ctx(), &json!("a"), &cfg));
    assert!(msg.contains("config"), "{msg}");
}

#[test]
fn missing_schema_is_error() {
    let p = JsonSchemaTransform::new("{}");
    let msg = error_msg(p.transform_result(&ctx(), &json!("a"), &json!({})));
    assert!(msg.contains("config"), "{msg}");
}

#[test]
fn max_errors_caps_message() {
    let p = JsonSchemaTransform::new("{}");
    // Require 5 distinct properties; supply none → 5 errors, cap at 2.
    let cfg = json!({
        "schema": { "type": "object", "required": ["a", "b", "c", "d", "e"] },
        "max_errors": 2
    });
    let msg = error_msg(p.transform_result(&ctx(), &json!({}), &cfg));
    assert!(msg.contains("more than 2 errors"), "{msg}");
}

#[test]
fn nested_defs_ref_resolves_offline() {
    let p = JsonSchemaTransform::new("{}");
    // In-document $ref must resolve with default-features=false (no network).
    let cfg = json!({
        "schema": {
            "type": "object",
            "properties": { "item": { "$ref": "#/$defs/Foo" } },
            "required": ["item"],
            "$defs": { "Foo": { "type": "object", "required": ["id"] } }
        }
    });
    assert!(matches!(
        p.transform_result(&ctx(), &json!({ "item": { "id": "x" } }), &cfg),
        TransformResult::Unchanged
    ));
    assert!(matches!(
        p.transform_result(&ctx(), &json!({ "item": {} }), &cfg),
        TransformResult::Error { .. }
    ));
}
