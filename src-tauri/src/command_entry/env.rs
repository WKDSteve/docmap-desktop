//! The process environment of a hub, copied once.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;

/// The process environment, copied once. Tests hand in their own.
#[derive(Debug, Clone, Default)]
pub struct Env {
    vars: HashMap<String, OsString>,
}

impl Env {
    #[must_use]
    pub fn from_process() -> Self {
        Self {
            vars: std::env::vars_os()
                .filter_map(|(k, v)| k.into_string().ok().map(|k| (key(&k), v)))
                .collect(),
        }
    }

    #[cfg(test)]
    #[must_use]
    pub fn from_pairs<I, K, V>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: Into<OsString>,
    {
        Self {
            vars: pairs
                .into_iter()
                .map(|(k, v)| (key(k.as_ref()), v.into()))
                .collect(),
        }
    }

    /// A variable that is set and not empty.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&OsString> {
        self.vars.get(&key(name)).filter(|v| !v.is_empty())
    }

    #[must_use]
    pub fn path(&self, name: &str) -> Option<PathBuf> {
        self.get(name).map(PathBuf::from)
    }
}

/// Variable names are case-insensitive on Windows only.
fn key(name: &str) -> String {
    if cfg!(windows) {
        name.to_ascii_uppercase()
    } else {
        name.to_owned()
    }
}
