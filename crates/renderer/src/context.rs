//! The `{{ variable }}` context a scene is rendered against.
//!
//! Deliberately stringly-typed: providers (Steam, sensors, the event engine)
//! drop in `key -> value` pairs and scenes interpolate them into text or numeric
//! fields. No provider-specific types leak into the engine.

use std::collections::HashMap;

#[derive(Debug, Default, Clone)]
pub struct Context {
    vars: HashMap<String, String>,
}

impl Context {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, key: impl Into<String>, val: impl Into<String>) {
        self.vars.insert(key.into(), val.into());
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.vars.get(key).map(String::as_str)
    }

    /// Replace every `{{ key }}` in `s`. Unknown keys expand to an empty string.
    pub fn expand(&self, s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut rest = s;
        while let Some(open) = rest.find("{{") {
            out.push_str(&rest[..open]);
            let after = &rest[open + 2..];
            match after.find("}}") {
                Some(close) => {
                    out.push_str(self.get(after[..close].trim()).unwrap_or(""));
                    rest = &after[close + 2..];
                }
                None => {
                    out.push_str(&rest[open..]);
                    rest = "";
                }
            }
        }
        out.push_str(rest);
        out
    }

    /// Expand then parse as `f32`.
    pub fn expand_f32(&self, s: &str) -> Option<f32> {
        self.expand(s).trim().parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_known_and_unknown() {
        let mut c = Context::new();
        c.set("name", "DOOM");
        assert_eq!(c.expand("<< {{ name }} >>"), "<< DOOM >>");
        assert_eq!(c.expand("a {{ missing }} b"), "a  b");
        assert_eq!(c.expand("no placeholders"), "no placeholders");
    }

    #[test]
    fn expands_numbers() {
        let mut c = Context::new();
        c.set("gpu.load", "0.42");
        assert_eq!(c.expand_f32("{{ gpu.load }}"), Some(0.42));
    }
}
