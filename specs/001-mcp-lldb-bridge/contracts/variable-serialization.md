# Variable Serialization Contract

**Feature**: 001-mcp-lldb-bridge
**Date**: 2026-05-20

Defines how Rust runtime values are serialized from LLDB into JSON for consumption by the AI agent.
Implemented in `crates/runtime-core::serialization`.

---

## Design Goals

1. **AI-consumable**: Values MUST carry enough type and semantic context for the AI to reason
   about them without knowing Rust internals.
2. **Bounded output**: Serialization MUST terminate and produce bounded output regardless of
   value depth or size.
3. **Lossless metadata**: When values are truncated or approximated, metadata MUST be included
   so the AI knows the original shape.
4. **Semantic annotation**: Variables captured via `probe!` MUST carry qualified names and
   context so the AI receives structured meaning, not raw symbols.

---

## JSON Schema

### Scalar Values

```json
{ "type": "bool",  "value": true }
{ "type": "i8",    "value": -42 }
{ "type": "i16",   "value": -42 }
{ "type": "i32",   "value": -12 }
{ "type": "i64",   "value": -12 }
{ "type": "i128",  "value": -12 }
{ "type": "isize", "value": -12 }
{ "type": "u8",    "value": 255 }
{ "type": "u16",   "value": 65535 }
{ "type": "u32",   "value": 42 }
{ "type": "u64",   "value": 42 }
{ "type": "u128",  "value": 42 }
{ "type": "usize", "value": 42 }
{ "type": "f32",   "value": 3.14 }
{ "type": "f64",   "value": 3.14159265358979 }
{ "type": "char",  "value": "A" }
{ "type": "()",    "value": null }
```

### String Values

```json
{ "type": "String", "value": "hello world" }
```

Truncated:
```json
{
  "type": "String",
  "value": "first 4096 chars...",
  "$truncated": true,
  "total_bytes": 8192
}
```

### Struct Values

```json
{
  "type": "layout::LayoutState",
  "fields": {
    "current_x": { "type": "i32", "value": 88 },
    "remaining_width": { "type": "i32", "value": -12 },
    "overflow": { "type": "bool", "value": true }
  }
}
```

Depth limit exceeded:
```json
{
  "type": "layout::LayoutState",
  "$depth_limit": true,
  "summary": "LayoutState { current_x: 88, ... }"
}
```

### Enum Values

```json
{
  "type": "std::option::Option<i32>",
  "variant": "Some",
  "fields": { "0": { "type": "i32", "value": 42 } }
}
```

```json
{
  "type": "std::option::Option<i32>",
  "variant": "None"
}
```

### Array / Slice / Vec Values

```json
{
  "type": "[i32; 5]",
  "elements": [1, 2, 3, 4, 5],
  "len": 5
}
```

Truncated:
```json
{
  "type": "Vec<i32>",
  "elements": [1, 2, 3, "..."],
  "$truncated": true,
  "shown": 256,
  "total": 10000
}
```

### Pointer Values

```json
{
  "type": "*const i32",
  "address": "0x7fff5fbff4a0",
  "dereferenced": { "type": "i32", "value": 42 }
}
```

Null pointer:
```json
{
  "type": "*const i32",
  "address": "0x0",
  "null": true
}
```

### Cyclic Reference

When a value's address has already been serialized in the current traversal:
```json
{
  "$ref": "0x7fff5fbff4a0",
  "type": "layout::Node"
}
```

### Error (LLDB could not read value)

```json
{
  "type": "i32",
  "$error": "Could not read memory at 0xdeadbeef"
}
```

---

## Full Variable Record

A complete `Variable` JSON record (as returned by `read_locals`):

```json
{
  "name": "remaining_width",
  "qualified_name": "measure_layout.remaining_width",
  "type": "i32",
  "address": "0x7fff5fbff4a0",
  "value": -12,
  "semantic_context": "measure_layout",
  "description": null
}
```

Without semantic probe:
```json
{
  "name": "x",
  "qualified_name": "x",
  "type": "i32",
  "address": "0x7fff5fbff4a0",
  "value": 88,
  "semantic_context": null,
  "description": null
}
```

---

## Limits & Configuration

All limits are configurable via `apps/mcp-server` launch arguments and/or the session config.

| Parameter | Default | Description |
|-----------|---------|-------------|
| `max_depth` | 8 | Maximum struct/enum nesting depth |
| `max_array_elements` | 256 | Max elements shown for arrays/slices/Vec |
| `max_string_bytes` | 4096 | Max bytes shown for String/str values |
| `max_variables_per_frame` | 128 | Max locals returned per `read_locals` call |
| `dereference_pointers` | `true` | Whether to follow pointer values one level |

---

## Semantic Probe Wire Format

When `probe_context` is set in `read_locals`, the output MUST include:
- `qualified_name`: `"{context}.{variable_name}"`
- `semantic_context`: the context string
- Variables NOT in the probe filter list are EXCLUDED from the response (if a probe is registered
  for this context); otherwise all locals are returned with the context prefix applied.

Example AI-facing output for `probe!("measure_layout", remaining_width, current_x, overflow)`:

```json
{
  "probe_context": "measure_layout",
  "variables": [
    {
      "name": "current_x",
      "qualified_name": "measure_layout.current_x",
      "type": "i32",
      "value": 88,
      "semantic_context": "measure_layout"
    },
    {
      "name": "remaining_width",
      "qualified_name": "measure_layout.remaining_width",
      "type": "i32",
      "value": -12,
      "semantic_context": "measure_layout"
    },
    {
      "name": "overflow",
      "qualified_name": "measure_layout.overflow",
      "type": "bool",
      "value": true,
      "semantic_context": "measure_layout"
    }
  ]
}
```

Contrast with raw (no probe):
```json
{
  "variables": [
    { "name": "x",      "qualified_name": "x",      "type": "i32",  "value": 88    },
    { "name": "w",      "qualified_name": "w",       "type": "i32",  "value": -12   },
    { "name": "flag",   "qualified_name": "flag",    "type": "bool", "value": true  },
    { "name": "cursor", "qualified_name": "cursor",  "type": "usize","value": 0     }
  ]
}
```

The semantic probe version is unambiguous to the AI; the raw version requires the AI to infer
meaning from single-letter variable names.
