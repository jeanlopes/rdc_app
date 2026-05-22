// T049, T050 — stub
use std::collections::HashMap;

/// Registry mapping probe context names to their variable filter lists.
///
/// Use the [`probe!`] macro to register named variable groups, then pass
/// the context name to `read_locals` via `probe_context` to receive
/// semantically annotated variables.
///
/// # Example
/// ```
/// use runtime_core::probe::ProbeRegistry;
/// let mut reg = ProbeRegistry::new();
/// reg.register("measure_layout", vec!["current_x".into(), "remaining_width".into()]);
/// assert_eq!(reg.lookup("measure_layout"), Some(&["current_x", "remaining_width"][..]));
/// ```
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

/// Convenience macro that captures a list of variable names under a named context.
///
/// Returns a `(String, Vec<String>)` tuple of `(context_name, variable_names)`.
/// Pass the context name to `read_locals` as `probe_context` to receive
/// qualified names like `measure_layout.remaining_width`.
///
/// # Example
/// ```
/// use runtime_core::probe;
/// let (ctx, vars) = probe!("measure_layout", current_x, remaining_width, overflow);
/// assert_eq!(ctx, "measure_layout");
/// assert_eq!(vars, vec!["current_x", "remaining_width", "overflow"]);
/// ```
#[macro_export]
macro_rules! probe {
    ($context:expr, $($var:ident),+ $(,)?) => {
        {
            let vars = vec![$(stringify!($var).to_string()),+];
            ($context.to_string(), vars)
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_registry_register_lookup() {
        let mut reg = ProbeRegistry::new();
        reg.register("ctx", vec!["a".into(), "b".into()]);
        let result = reg.lookup("ctx").unwrap();
        assert_eq!(result, &["a".to_string(), "b".to_string()][..]);
    }

    #[test]
    fn probe_registry_unknown_returns_none() {
        let reg = ProbeRegistry::new();
        assert_eq!(reg.lookup("missing"), None);
    }

    #[test]
    fn probe_macro_returns_context_and_vars() {
        let (ctx, vars) = probe!("ctx", x, y);
        assert_eq!(ctx, "ctx");
        assert_eq!(vars, vec!["x", "y"]);
    }
}
