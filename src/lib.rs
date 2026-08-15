//! JSON Schema validation transform plugin.
//!
//! Validates a JSON value against an operator-supplied inline JSON Schema. A
//! valid value passes through unchanged (`TransformResult::Unchanged`); an
//! invalid value yields `TransformResult::Error` listing the failing instance
//! paths (bounded by `max_errors`). This is a VALIDATION transform — it never
//! rewrites the value (that distinguishes it from a mutating transform).
//!
//! Stateless apart from the manifest; the schema + options arrive per call in
//! `config`, so one instance serves the global transform chain and the pipeline
//! `plugin_transform` bridge. Pure compute, fully offline — only inline schemas
//! (in-document `$ref`) are supported; no remote `$ref` is fetched.

use mcpg_plugin_protocol::{PluginContext, PluginManifest, TransformResult, firstparty_manifest};
use mcpg_plugin_sdk::ffi::SyncTransform;
use serde::Deserialize;
use serde_json::Value;

const DEFAULT_MAX_ERRORS: usize = 32;

/// Which dispatch phase(s) a global transform fires on. Ignored by the
/// pipeline bridge (the host calls `transform_result` directly there).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Arguments,
    Result,
    #[default]
    Both,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct JsonSchemaConfig {
    /// The inline JSON Schema to validate against (in-document `$ref` only).
    schema: Value,
    /// JSON Pointer (RFC 6901) to the sub-value to validate. When omitted (or
    /// `""`), the whole value is validated.
    #[serde(default)]
    pointer: Option<String>,
    #[serde(default)]
    phase: Phase,
    /// Cap on the number of validation errors reported (bounds the message).
    #[serde(default = "default_max_errors")]
    max_errors: usize,
}

fn default_max_errors() -> usize {
    DEFAULT_MAX_ERRORS
}

pub struct JsonSchemaTransform {
    manifest: PluginManifest,
}

impl JsonSchemaTransform {
    pub fn new(_config_json: &str) -> Self {
        Self {
            manifest: firstparty_manifest! {
                id: "dev.mcpg.transform.json-schema",
                name: "JSON Schema Validation Transform",
                class: Transform,
            },
        }
    }

    fn run(&self, value: &Value, config: &Value, phase: Phase) -> TransformResult {
        let cfg: JsonSchemaConfig = match serde_json::from_value(config.clone()) {
            Ok(c) => c,
            Err(e) => {
                return TransformResult::Error {
                    message: format!("json-schema transform config: {e}"),
                };
            }
        };
        // Global-mode phase gating; pipeline-mode always calls transform_result.
        if cfg.phase != Phase::Both && cfg.phase != phase {
            return TransformResult::Unchanged;
        }

        let ptr = cfg.pointer.as_deref().unwrap_or("");
        let target = match value.pointer(ptr) {
            Some(t) => t,
            None => {
                return TransformResult::Error {
                    message: format!("pointer {ptr:?} not found in value"),
                };
            }
        };

        let validator = match jsonschema::validator_for(&cfg.schema) {
            Ok(v) => v,
            Err(e) => {
                return TransformResult::Error {
                    message: format!("invalid JSON Schema: {e}"),
                };
            }
        };

        let mut messages: Vec<String> = Vec::new();
        let mut truncated = false;
        for (i, err) in validator.iter_errors(target).enumerate() {
            if i >= cfg.max_errors {
                truncated = true;
                break;
            }
            // Empty instance_path = the root value; otherwise prefix the path.
            if err.instance_path.as_str().is_empty() {
                messages.push(err.to_string());
            } else {
                messages.push(format!("{}: {}", err.instance_path, err));
            }
        }

        if messages.is_empty() {
            // Valid — validation never mutates the value.
            TransformResult::Unchanged
        } else {
            let mut message = format!("JSON Schema validation failed: {}", messages.join("; "));
            if truncated {
                message.push_str(&format!(" (… more than {} errors)", cfg.max_errors));
            }
            TransformResult::Error { message }
        }
    }
}

impl SyncTransform for JsonSchemaTransform {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn transform_arguments(
        &self,
        _ctx: &PluginContext,
        arguments: &Value,
        config: &Value,
    ) -> TransformResult {
        self.run(arguments, config, Phase::Arguments)
    }

    fn transform_result(
        &self,
        _ctx: &PluginContext,
        result: &Value,
        config: &Value,
    ) -> TransformResult {
        self.run(result, config, Phase::Result)
    }
}

// cdylib export — gated so a plain workspace build emits only the rlib (no
// duplicate `mcpg_plugin_register` symbol across plugin crates).
#[cfg(any(feature = "cdylib-export", feature = "static-firstparty"))]
mcpg_plugin_sdk::declare_plugin! {
    plugin_id: "dev.mcpg.transform.json-schema",
    plugin_version: env!("CARGO_PKG_VERSION"),
    descriptor_yaml: include_str!("../plugin.yaml"),
    capabilities: &[],
    entities: [
        transform as xform {
            inner_name: "",
            plugin_type: JsonSchemaTransform,
            factory: |cfg: &str, _host: ::mcpg_plugin_sdk::HostHandle| JsonSchemaTransform::new(cfg),
        },
    ],
}

#[cfg(test)]
mod tests;
