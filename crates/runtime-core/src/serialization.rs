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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::variable::{ScalarValue, Variable, VariableValue};

    fn make_var(name: &str, value: VariableValue) -> Variable {
        Variable { name: name.to_string(), type_name: "test".to_string(), value, address: None, semantic: None }
    }

    fn ser() -> Serializer { Serializer::new(SerializerOpts::default()) }

    fn limited(max_depth: u32, max_array: usize, max_string: usize) -> Serializer {
        Serializer::new(SerializerOpts { max_depth, max_array_elements: max_array, max_string_bytes: max_string })
    }

    #[test]
    fn serialize_bool_true() {
        let v = ser().serialize_variable(&make_var("x", VariableValue::Scalar(ScalarValue::Bool(true))));
        assert_eq!(v["value"], true);
    }

    #[test]
    fn serialize_bool_false() {
        let v = ser().serialize_variable(&make_var("x", VariableValue::Scalar(ScalarValue::Bool(false))));
        assert_eq!(v["value"], false);
    }

    #[test]
    fn serialize_int_negative() {
        let v = ser().serialize_variable(&make_var("x", VariableValue::Scalar(ScalarValue::Int(-12))));
        assert_eq!(v["value"], -12);
    }

    #[test]
    fn serialize_string_within_limit() {
        let v = limited(8, 256, 1024).serialize_variable(
            &make_var("x", VariableValue::String { value: "hello".to_string(), truncated: false })
        );
        assert!(v["value"].get("$truncated").is_none() || v["value"]["$truncated"].is_null());
    }

    #[test]
    fn serialize_string_over_limit() {
        let v = limited(8, 256, 4).serialize_variable(
            &make_var("x", VariableValue::String { value: "hello world!".to_string(), truncated: false })
        );
        assert_eq!(v["value"]["$truncated"], true);
        assert!(v["value"]["total_bytes"].as_u64().unwrap() > 4);
    }

    #[test]
    fn serialize_depth_limit() {
        let inner = VariableValue::Scalar(ScalarValue::Int(1));
        let level2 = VariableValue::Struct { fields: vec![Variable {
            name: "inner".into(), type_name: "i32".into(), value: inner, address: None, semantic: None,
        }]};
        let level1 = VariableValue::Struct { fields: vec![Variable {
            name: "level2".into(), type_name: "L2".into(), value: level2, address: None, semantic: None,
        }]};
        let v = limited(2, 256, 4096).serialize_variable(&make_var("root", level1));
        assert_eq!(v["value"]["level2"]["inner"]["$depth_limit"], true);
    }

    #[test]
    fn serialize_array_within_limit() {
        let elems: Vec<VariableValue> = (0..5).map(|i| VariableValue::Scalar(ScalarValue::Int(i))).collect();
        let v = limited(8, 10, 4096).serialize_variable(
            &make_var("arr", VariableValue::Array { elements: elems, truncated: false, total: 5 })
        );
        assert!(v["value"].get("$truncated").is_none() || v["value"]["$truncated"] == false);
    }

    #[test]
    fn serialize_array_over_limit() {
        let elems: Vec<VariableValue> = (0..10).map(|i| VariableValue::Scalar(ScalarValue::Int(i))).collect();
        let v = limited(8, 3, 4096).serialize_variable(
            &make_var("arr", VariableValue::Array { elements: elems, truncated: false, total: 10 })
        );
        assert_eq!(v["value"]["$truncated"], true);
        assert!(v["value"]["shown"].as_u64().is_some());
        assert!(v["value"]["total"].as_u64().is_some());
    }

    #[test]
    fn serialize_cyclic_ref() {
        let addr = 0xdeadbeef_u64;
        let v = ser().serialize_variable(&make_var("p", VariableValue::Pointer {
            address: addr,
            dereferenced: Some(Box::new(VariableValue::CyclicRef { address: addr })),
        }));
        assert!(v["value"]["dereferenced"]["$ref"].as_str().unwrap().starts_with("0x"));
    }

    #[test]
    fn serialize_null_pointer() {
        let v = ser().serialize_variable(&make_var("p", VariableValue::Pointer { address: 0, dereferenced: None }));
        assert_eq!(v["value"]["null"], true);
    }
}
