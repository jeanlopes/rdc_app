use serde_json::{json, Value};
use std::collections::HashSet;
use crate::variable::{Variable, VariableValue, ScalarValue};

/// Configures the recursive variable serializer.
///
/// # Example
/// ```
/// use runtime_core::serialization::SerializerOpts;
/// let opts = SerializerOpts { max_depth: 4, max_array_elements: 64, max_string_bytes: 1024 };
/// ```
#[derive(Debug, Clone)]
pub struct SerializerOpts {
    pub max_depth: u32,
    pub max_array_elements: usize,
    pub max_string_bytes: usize,
}

impl Default for SerializerOpts {
    fn default() -> Self {
        Self {
            max_depth: 8,
            max_array_elements: 256,
            max_string_bytes: 4096,
        }
    }
}

/// Converts `Variable` values to `serde_json::Value` with depth limiting,
/// array truncation, string truncation, and cyclic reference detection.
pub struct Serializer {
    opts: SerializerOpts,
}

impl Serializer {
    pub fn new(opts: SerializerOpts) -> Self {
        Self { opts }
    }

    pub fn serialize_variable(&self, var: &Variable) -> Value {
        let mut visited = HashSet::new();
        let value = self.serialize_value(&var.value, 0, &mut visited);
        let mut obj = json!({
            "name": var.name,
            "type": var.type_name,
            "value": value,
        });
        if let Some(addr) = var.address {
            obj["address"] = json!(format!("0x{:x}", addr));
        }
        if let Some(sem) = &var.semantic {
            obj["qualified_name"] = json!(&sem.qualified_name);
            obj["semantic_context"] = json!(&sem.context);
        } else {
            obj["qualified_name"] = json!(&var.name);
            obj["semantic_context"] = Value::Null;
        }
        obj
    }

    fn serialize_value(&self, val: &VariableValue, depth: u32, visited: &mut HashSet<u64>) -> Value {
        if depth >= self.opts.max_depth {
            return json!({ "$depth_limit": true });
        }
        match val {
            VariableValue::Scalar(s) => self.serialize_scalar(s),
            VariableValue::String { value, truncated } => {
                let displayed = if value.len() > self.opts.max_string_bytes {
                    let truncated_val = &value[..self.opts.max_string_bytes];
                    return json!({ "value": truncated_val, "$truncated": true, "total_bytes": value.len() });
                } else {
                    value.as_str()
                };
                if *truncated {
                    json!({ "value": displayed, "$truncated": true })
                } else {
                    json!(displayed)
                }
            }
            VariableValue::Struct { fields } => {
                let obj: serde_json::Map<String, Value> = fields
                    .iter()
                    .map(|f| (f.name.clone(), self.serialize_value(&f.value, depth + 1, visited)))
                    .collect();
                Value::Object(obj)
            }
            VariableValue::Array { elements, truncated, total } => {
                let shown: Vec<Value> = elements
                    .iter()
                    .take(self.opts.max_array_elements)
                    .map(|e| self.serialize_value(e, depth + 1, visited))
                    .collect();
                let actually_truncated = *truncated || elements.len() > self.opts.max_array_elements;
                if actually_truncated {
                    json!({ "elements": shown, "$truncated": true, "shown": shown.len(), "total": total })
                } else {
                    json!({ "elements": shown, "len": elements.len() })
                }
            }
            VariableValue::Pointer { address, dereferenced } => {
                if *address == 0 {
                    return json!({ "address": "0x0", "null": true });
                }
                if visited.contains(address) {
                    return json!({ "$ref": format!("0x{:x}", address) });
                }
                visited.insert(*address);
                let deref = dereferenced
                    .as_ref()
                    .map(|d| self.serialize_value(d, depth + 1, visited));
                json!({ "address": format!("0x{:x}", address), "dereferenced": deref })
            }
            VariableValue::CyclicRef { address } => {
                json!({ "$ref": format!("0x{:x}", address) })
            }
            VariableValue::Opaque { summary } => json!({ "$opaque": summary }),
            VariableValue::Error { message } => json!({ "$error": message }),
        }
    }

    fn serialize_scalar(&self, s: &ScalarValue) -> Value {
        match s {
            ScalarValue::Bool(b) => json!(b),
            ScalarValue::Int(i) => json!(i),
            ScalarValue::UInt(u) => json!(u),
            ScalarValue::Float(f) => json!(f),
            ScalarValue::Char(c) => json!(c.to_string()),
            ScalarValue::Unit => Value::Null,
        }
    }
}
