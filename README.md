# JSON Schema Validation Transform — `dev.mcpg.transform.json-schema`

> class `transform` · `native` · package `mcpg-plugin-transform-json-schema` · artifact `libmcpg_plugin_transform_json_schema.so` · Apache-2.0

Transform plugin that validates a JSON value against an operator-supplied inline
JSON Schema. A valid value passes through unchanged; an invalid value produces a
transform error naming the failing instance paths. This is a validation
transform: it never rewrites the value, which is what separates it from its
reshaping siblings. Validation runs fully offline — the validator is built
without the HTTP resolver, so only in-document `$ref` is resolved and no remote
schema is ever fetched. Reach for it to assert a contract on tool arguments or
on a backend's response before the rest of a pipeline depends on that shape.

## What it does
- Compiles the inline `schema` and validates the input value against it.
- Passes a valid value through unchanged, leaving the payload byte-identical.
- Reports an invalid value as a transform error listing each failing instance path, capped at `max_errors` with a truncation marker.
- Validates only a sub-value when an RFC 6901 JSON Pointer is given, ignoring the rest of the payload.
- Resolves in-document `$ref` such as `#/$defs/Foo` locally; no remote `$ref` is fetched and no network stack is linked in.
- Treats a malformed schema, a missing `schema`, an unresolvable pointer, and an unknown config key as errors rather than as an implicit pass.
- Declares no `required_capabilities` — it never calls back into the host for network, filesystem, or secret access.

## Configuration
Loaded from the flat top-level `plugins:` list. An entry there joins the global
transform chain and sees every tool call; the same registered plugin can also be
named by a pipeline `plugin_transform` step for a single binding.

```yaml
plugins:
  - id: dev.mcpg.transform.json-schema
    class: transform
    source: { oci: ghcr.io/mcpg-dev/source-code/plugins/transform-json-schema:protocol-1 }
    config:
      phase: arguments
      schema:
        type: object
        required: [query]
        properties:
          query: { type: string, minLength: 1 }
          limit: { type: integer, maximum: 100 }
```

| Field | Type | Default | Description |
|---|---|---|---|
| `schema` | JSON Schema object | *(required)* | The schema to validate against. Inline only; in-document `$ref` resolves, remote `$ref` does not. |
| `pointer` | string (RFC 6901) | whole value | Validate only the sub-value at this pointer. A pointer that does not resolve is an error. |
| `phase` | `arguments` \| `result` \| `both` | `both` | Which dispatch phase the global chain fires this transform on. A pipeline step always dispatches through the result path, so `arguments` there turns the step into a no-op. |
| `max_errors` | integer | `32` | Cap on the number of validation errors included in the message, which bounds its size. |

In the global chain the pre-dispatch value is the tool's `arguments` object and
the post-dispatch value is the serialised tool result — `content`, optional
`structuredContent`, `isError` — so a `phase: result` schema describes that
envelope, and a pointer such as `/structuredContent` narrows it to the payload.

Unknown fields are rejected, so a mistyped key fails the transform instead of
being silently ignored.

Referenced from a pipeline instead, the plugin receives the whole pipeline
context — `arguments`, `tool_name`, `steps`, and `context` — as its input value,
so a pointer addresses a prior step by id:

```yaml
mcp:
  capabilities:
    tools:
      - name: orders.enriched
        description: Fetch orders and refuse to continue unless the shape matches.
        backend:
          kind: pipeline
          steps:
            - kind: http
              id: fetch
              url: https://orders.example.com/list
            - kind: plugin_transform
              id: check
              plugin: dev.mcpg.transform.json-schema
              config:
                pointer: /steps/fetch/output
                schema: { type: array }
```

## Security
The two wiring points differ in what a validation failure costs, and the
difference is the single most important thing to get right:

- **In the global transform chain**, an error is logged as a warning and the
  chain carries the last good value forward. Validation there is an observability
  and alerting signal, not an enforcement point — the tool call still proceeds.
- **In a pipeline `plugin_transform` step**, an error fails the step. Use this
  wiring when an invalid payload must stop the call.

The validator is compiled from the entry's `config:`, which is operator-authored
and config-origin; request data never supplies the schema. Because the
`jsonschema` dependency is built with default features off, no HTTP client or TLS
stack is linked in, so a schema containing a remote `$ref` cannot cause the
gateway to make an outbound request.

## Observability
Every application through the global chain increments
`mcpg_transform_applies_total` (labels `plugin_id`, `phase` of `pre` or `post`,
`outcome` of `unchanged`, `modified`, or `error`) and records
`mcpg_transform_apply_ms`. Because a passing validation reports `unchanged` and a
failing one reports `error`, the `outcome` label alone gives a contract-violation
rate per plugin id.

## Build
Default feature set is OFF (avoids `mcpg_plugin_register` linker
collisions in the workspace build); opt in to the cdylib:

```bash
cargo build -p mcpg-plugin-transform-json-schema --features cdylib-export --release   # → target/release/libmcpg_plugin_transform_json_schema.so
```

## Sign & load (production)
Sign the artifact, pin/verify via the entry's `signature:` block, and honour
revocations. See <https://mcpg.dev/docs/security/plugin-security>.

## See also
- Pipeline step reference: <https://mcpg.dev/docs/reference/pipeline-steps>
- What a plugin is and how the ABI works: <https://mcpg.dev/docs/plugins/plugins-and-protocol>
- Reshape rather than validate: `libs/plugins/transform/jsonata`, `libs/plugins/transform/template`
- Format conversion: `libs/plugins/transform/csv`, `libs/plugins/transform/xml`
