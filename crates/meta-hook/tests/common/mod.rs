//! Shared helpers for `meta-hook` integration tests.
//!
//! Centralizes binary discovery, `.claude/settings.json` generation, and
//! the cross-platform binary-name contract so individual tests stay free
//! of `#[cfg(windows)]` / `#[cfg(unix)]` branches.
//!
//! Each integration test file is its own crate, so any helper not used
//! by a given test file triggers `dead_code`. `#[allow(dead_code)]` on
//! each function keeps the shared module compiling cleanly regardless
//! of which subset of helpers a particular test file imports.

#![allow(dead_code)]

use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

/// Locate the freshly-built `meta-hook` binary.
///
/// Uses `CARGO_BIN_EXE_meta-hook` when running under `cargo test`; falls
/// back to `target/debug/<platform_bin_name("meta-hook")>` otherwise.
pub fn meta_hook_bin() -> PathBuf {
    if let Some(p) = option_env!("CARGO_BIN_EXE_meta-hook") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target")
        .join("debug")
        .join(platform_bin_name("meta-hook"))
}

/// Returns `"{stem}.exe"` on Windows, `stem.to_string()` elsewhere.
///
/// This is the single source of truth for the cross-platform binary-name
/// contract — tests should call this instead of using `#[cfg]` blocks.
pub fn platform_bin_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_string()
    }
}

/// Write a minimal `.claude/settings.json` to `dir/.claude/` registering
/// `command` under `hooks[event]` with a wildcard matcher.
///
/// Creates `dir/.claude/` if it doesn't exist. Caller is responsible for
/// supplying a command string that is portable across `sh -c` (POSIX)
/// and `cmd /C` (Windows).
pub fn gen_settings(dir: &Path, event: &str, command: &str) {
    let claude_dir = dir.join(".claude");
    fs::create_dir_all(&claude_dir).expect("create .claude dir");
    let settings = json!({
        "hooks": {
            event: [
                {
                    "matcher": "*",
                    "hooks": [
                        {"type": "command", "command": command}
                    ]
                }
            ]
        }
    });
    fs::write(
        claude_dir.join("settings.json"),
        serde_json::to_string_pretty(&settings).expect("serialize settings"),
    )
    .expect("write settings.json");
}
