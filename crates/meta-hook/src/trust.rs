//! Trust file at `~/.claude/meta-hook-trust.json`.
//!
//! v1 is **warn-only**: if a trust file is present and the resolved
//! delegation target isn't listed, we print a one-line stderr warning
//! but still execute. If the file is absent, all delegations are silent.
//! (A future opt-in could promote this to refuse-mode.)

use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

/// Shape of the trust file: a JSON array of absolute sub-repo paths.
#[derive(Debug, Deserialize, Default)]
#[serde(transparent)]
pub struct TrustFile {
    pub roots: Vec<PathBuf>,
}

impl TrustFile {
    /// Load from a path. `Ok(None)` if the file is absent — that's the
    /// "trust everything silently" default.
    pub fn load(path: &Path) -> anyhow::Result<Option<Self>> {
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(path)?;
        let parsed: TrustFile = serde_json::from_str(&raw)?;
        Ok(Some(parsed))
    }

    /// Resolve the conventional default location: `$HOME/.claude/meta-hook-trust.json`.
    /// Returns `None` if `$HOME` (or `%USERPROFILE%` on Windows) can't be resolved.
    pub fn default_path() -> Option<PathBuf> {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)?;
        Some(home.join(".claude").join("meta-hook-trust.json"))
    }

    /// Is `target` in the trust list?
    pub fn trusts(&self, target: &Path) -> bool {
        // Canonicalize both sides for comparison robustness.
        let target_canon = target.canonicalize().unwrap_or_else(|_| target.into());
        for r in &self.roots {
            let r_canon = r.canonicalize().unwrap_or_else(|_| r.into());
            if r_canon == target_canon {
                return true;
            }
        }
        false
    }
}

/// Decide whether to emit the "untrusted delegation" warning.
/// Returns the warning string when appropriate, or `None` if silent.
///
/// Policy:
/// * No trust file → silent (None).
/// * Trust file present and lists `target` → silent.
/// * Trust file present but does not list `target` → warning.
pub fn warning_for(trust: Option<&TrustFile>, target: &Path) -> Option<String> {
    let trust = trust?;
    if trust.trusts(target) {
        return None;
    }
    Some(format!(
        "meta-hook: delegating to untrusted repo {} - add to ~/.claude/meta-hook-trust.json to silence",
        target.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn load_returns_none_when_absent() {
        let tmp = tempdir().unwrap();
        let p = tmp.path().join("nope.json");
        assert!(TrustFile::load(&p).unwrap().is_none());
    }

    #[test]
    fn load_parses_array_of_paths() {
        let tmp = tempdir().unwrap();
        let p = tmp.path().join("trust.json");
        fs::write(&p, r#"["/a/b", "/c/d"]"#).unwrap();
        let tf = TrustFile::load(&p).unwrap().unwrap();
        assert_eq!(tf.roots.len(), 2);
    }

    #[test]
    fn trusts_matches_listed_path() {
        let tmp = tempdir().unwrap();
        let a = tmp.path().join("a");
        let b = tmp.path().join("b");
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        let tf = TrustFile {
            roots: vec![a.clone()],
        };
        assert!(tf.trusts(&a));
        assert!(!tf.trusts(&b));
    }

    #[test]
    fn warning_silent_when_no_trust_file() {
        assert!(warning_for(None, Path::new("/anything")).is_none());
    }

    #[test]
    fn warning_silent_when_trusted() {
        let tmp = tempdir().unwrap();
        let p = tmp.path().to_path_buf();
        let tf = TrustFile {
            roots: vec![p.clone()],
        };
        assert!(warning_for(Some(&tf), &p).is_none());
    }

    #[test]
    fn warning_emitted_when_not_trusted() {
        let tmp = tempdir().unwrap();
        let p = tmp.path().join("foo");
        fs::create_dir_all(&p).unwrap();
        let tf = TrustFile { roots: vec![] };
        let msg = warning_for(Some(&tf), &p).unwrap();
        assert!(msg.contains("meta-hook:"));
        assert!(msg.contains("foo"));
    }
}
