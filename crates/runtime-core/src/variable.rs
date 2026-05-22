// T046, T047 — stub
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticAnnotation {
    pub context: String,
    pub qualified_name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ScalarValue {
    Bool(bool),
    Int(i128),
    UInt(u128),
    Float(f64),
    Char(char),
    Unit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VariableValue {
    Scalar(ScalarValue),
    Struct { fields: Vec<Variable> },
    Array { elements: Vec<VariableValue>, truncated: bool, total: usize },
    Pointer { address: u64, dereferenced: Option<Box<VariableValue>> },
    String { value: String, truncated: bool },
    CyclicRef { address: u64 },
    Opaque { summary: String },
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Variable {
    pub name: String,
    pub type_name: String,
    pub value: VariableValue,
    pub address: Option<u64>,
    pub semantic: Option<SemanticAnnotation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalResult {
    pub expression: String,
    pub value: VariableValue,
    pub type_name: String,
    pub error: Option<String>,
}
