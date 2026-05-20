// T049, T050 — stub
use std::collections::HashMap;

/// Registry mapping probe context names to their variable filter lists.
#[derive(Debug, Default)]
pub struct ProbeRegistry {
    probes: HashMap<String, Vec<String>>,
}

impl ProbeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, context: impl Into<String>, variables: Vec<String>) {
        self.probes.insert(context.into(), variables);
    }

    pub fn lookup(&self, context: &str) -> Option<&[String]> {
        self.probes.get(context).map(Vec::as_slice)
    }
}

/// Convenience macro: `probe!("context", var1, var2)` registers variables with the global registry.
#[macro_export]
macro_rules! probe {
    ($context:expr, $($var:ident),+ $(,)?) => {
        {
            let vars = vec![$(stringify!($var).to_string()),+];
            ($context.to_string(), vars)
        }
    };
}
