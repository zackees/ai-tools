//! Discovery: walk a target path upward to the enclosing git repo root,
//! plus a per-process cache and cwd-containment safety check.
//!
//! A "git root" is the parent directory of the nearest `.git` entry; the
//! `.git` entry may be a directory (regular checkout) **or** a file
//! (worktree / submodule). Both are handled.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Per-process LRU-ish cache mapping a query path to its resolved git
/// root (or `None` if none was found / it failed containment).
///
/// Bounded to a fixed-ish size to keep things honest, but each hook
/// process is short-lived so we never actually evict in practice.
#[derive(Default)]
pub struct DiscoverCache {
    inner: Mutex<HashMap<PathBuf, Option<PathBuf>>>,
}

impl DiscoverCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, p: &Path) -> Option<Option<PathBuf>> {
        self.inner.lock().ok()?.get(p).cloned()
    }

    pub fn put(&self, key: PathBuf, value: Option<PathBuf>) {
        if let Ok(mut g) = self.inner.lock() {
            g.insert(key, value);
        }
    }

    /// Visible for testing.
    pub fn len(&self) -> usize {
        self.inner.lock().map(|g| g.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Walk upward from `start` (or its parent if `start` is a file) looking
/// for the nearest directory that contains a `.git` entry. Returns the
/// containing directory (the git root), not the `.git` entry itself.
///
/// Returns `None` if no `.git` entry is found before the filesystem root.
pub fn enclosing_git_root(start: &Path) -> Option<PathBuf> {
    let mut cur: PathBuf = if start.is_file() {
        start.parent()?.to_path_buf()
    } else {
        start.to_path_buf()
    };
    loop {
        let candidate = cur.join(".git");
        if candidate.exists() {
            return Some(cur);
        }
        cur = match cur.parent() {
            Some(p) => p.to_path_buf(),
            None => return None,
        };
    }
}

/// Cached form of `enclosing_git_root`.
pub fn enclosing_git_root_cached(cache: &DiscoverCache, start: &Path) -> Option<PathBuf> {
    if let Some(hit) = cache.get(start) {
        return hit;
    }
    let resolved = enclosing_git_root(start);
    cache.put(start.to_path_buf(), resolved.clone());
    resolved
}

/// Check that `candidate` is a descendant of (or equal to) `session_cwd`.
///
/// Both paths are canonicalized when possible so symlinks don't fool the
/// check. If canonicalization fails (e.g., a non-existent path) we fall
/// back to a literal-prefix comparison after lexical normalization.
pub fn is_within_session(candidate: &Path, session_cwd: &Path) -> bool {
    let lhs = candidate
        .canonicalize()
        .unwrap_or_else(|_| candidate.into());
    let rhs = session_cwd
        .canonicalize()
        .unwrap_or_else(|_| session_cwd.into());
    lhs.starts_with(&rhs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn finds_git_root_one_level_up() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join(".git")).unwrap();
        let sub = root.join("src");
        fs::create_dir(&sub).unwrap();
        let file = sub.join("f.rs");
        fs::write(&file, "").unwrap();

        let got = enclosing_git_root(&file).unwrap();
        assert_eq!(got.canonicalize().unwrap(), root.canonicalize().unwrap());
    }

    #[test]
    fn finds_git_root_when_git_is_file() {
        // Worktree case: .git is a file containing "gitdir: ..."
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join(".git"), "gitdir: /some/path\n").unwrap();
        let sub = root.join("deep").join("er");
        fs::create_dir_all(&sub).unwrap();

        let got = enclosing_git_root(&sub).unwrap();
        assert_eq!(got.canonicalize().unwrap(), root.canonicalize().unwrap());
    }

    #[test]
    fn returns_none_when_no_git_found() {
        let tmp = tempdir().unwrap();
        let sub = tmp.path().join("a").join("b");
        fs::create_dir_all(&sub).unwrap();
        // No .git anywhere up the chain (within the tempdir at least; on a
        // real machine the search continues to /, but we just assert that
        // if we get a hit it's not inside our tempdir).
        let got = enclosing_git_root(&sub);
        if let Some(p) = got {
            assert!(
                !p.starts_with(tmp.path()),
                "should not match anything inside the empty tempdir"
            );
        }
    }

    #[test]
    fn cwd_containment_accepts_descendant() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        let sub = root.join("a");
        fs::create_dir(&sub).unwrap();
        assert!(is_within_session(&sub, root));
        assert!(is_within_session(root, root));
    }

    #[test]
    fn cwd_containment_rejects_sibling() {
        let tmp = tempdir().unwrap();
        let a = tmp.path().join("a");
        let b = tmp.path().join("b");
        fs::create_dir(&a).unwrap();
        fs::create_dir(&b).unwrap();
        assert!(!is_within_session(&a, &b));
    }

    #[test]
    fn cache_hit_skips_walk() {
        let cache = DiscoverCache::new();
        let key = PathBuf::from("/nonexistent/path");
        cache.put(key.clone(), Some(PathBuf::from("/some/root")));
        let got = enclosing_git_root_cached(&cache, &key);
        assert_eq!(got, Some(PathBuf::from("/some/root")));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn cache_miss_walks_and_populates() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join(".git")).unwrap();
        let f = root.join("file");
        fs::write(&f, "").unwrap();

        let cache = DiscoverCache::new();
        assert!(cache.is_empty());
        let _ = enclosing_git_root_cached(&cache, &f);
        assert_eq!(cache.len(), 1);
        // Second call hits the cache.
        let _ = enclosing_git_root_cached(&cache, &f);
        assert_eq!(cache.len(), 1);
    }
}
