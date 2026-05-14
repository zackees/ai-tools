//! Integration tests that programmatically generate a child sub-repo's
//! `.claude/settings.json` and then drive `meta-hook --post-tool` through
//! the chain, asserting end-to-end delegation works on both POSIX and
//! Windows hosts.
//!
//! Fixture shape:
//! ```text
//! <tempdir>/                       session cwd, workspace root
//!   child/
//!     .git/
//!     .claude/settings.json        generated per test
//!     src/edited.txt
//! ```
//!
//! Cross-platform contract: no `#[cfg(windows)]` / `#[cfg(unix)]` blocks
//! inside test bodies. OS-specific values come from
//! `common::platform_bin_name`.

mod common;

use common::{gen_settings, meta_hook_bin, platform_bin_name};
use serde_json::json;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::tempdir;

/// Build the `<tempdir>/child/{,.git,.claude,src/edited.txt}` fixture and
/// return `(tempdir_guard, workspace_root, child_root, edited_file)`.
fn build_child_fixture() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let tmp = tempdir().expect("create tempdir");
    let workspace = tmp.path().to_path_buf();
    let child = workspace.join("child");
    fs::create_dir_all(child.join(".git")).expect("create child/.git");
    fs::create_dir_all(child.join("src")).expect("create child/src");
    let edited = child.join("src").join("edited.txt");
    fs::write(&edited, "hello\n").expect("write edited.txt");
    (tmp, workspace, child, edited)
}

/// Run `meta-hook` with the given args, envelope on stdin, cwd =
/// `workspace`, and `CLAUDE_PROJECT_DIR` set to the same. Returns the
/// exit code plus captured stdout/stderr.
fn run_meta_hook(args: &[&str], envelope: &str, workspace: &Path) -> (i32, String, String) {
    let bin = meta_hook_bin();
    let mut child = Command::new(&bin)
        .args(args)
        .env("CLAUDE_PROJECT_DIR", workspace)
        .current_dir(workspace)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to spawn {}: {e}", bin.display()));
    if let Err(e) = child.stdin.as_mut().unwrap().write_all(envelope.as_bytes()) {
        if e.kind() != std::io::ErrorKind::BrokenPipe {
            panic!("write to child stdin failed: {e}");
        }
    }
    let out = child.wait_with_output().expect("wait_with_output");
    let code = out.status.code().unwrap_or(-1);
    (
        code,
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn post_tool_chain_via_generated_child_settings() {
    let (_tmp, workspace, child, edited) = build_child_fixture();

    // Portable one-liner: `echo X > FILE` works under both `sh -c` and
    // `cmd /C`. The hook runs with cwd = child sub-repo root (see
    // dispatch.rs), so a bare filename lands in `<workspace>/child/`.
    gen_settings(&child, "PostToolUse", "echo done > .marker");

    let envelope = json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Edit",
        "tool_input": {"file_path": edited.to_string_lossy()},
    })
    .to_string();

    let (code, _stdout, stderr) = run_meta_hook(&["--post-tool"], &envelope, &workspace);
    assert_eq!(code, 0, "stderr: {stderr}");

    let marker = child.join(".marker");
    assert!(
        marker.exists(),
        "marker file not written at {}; stderr: {stderr}",
        marker.display()
    );
    let content = fs::read_to_string(&marker).expect("read marker");
    // `echo` adds CRLF on Windows, LF on POSIX. Trim trailing whitespace
    // so the assertion is portable.
    assert_eq!(content.trim_end(), "done", "marker content: {content:?}");
}

#[test]
fn post_tool_chain_invokes_platform_specific_binary() {
    let (_tmp, workspace, child, edited) = build_child_fixture();

    // Build the absolute path to the meta-hook binary by replacing the
    // file name component with the platform-correct name. This exercises
    // the cross-platform binary-name contract: on Windows the path must
    // end in `.exe`; elsewhere it must not.
    let bin = meta_hook_bin();
    let parent = bin
        .parent()
        .expect("meta-hook bin has parent directory")
        .to_path_buf();
    let platform_name = platform_bin_name("meta-hook");
    let resolved_bin: PathBuf = parent.join(&platform_name);

    // Cross-platform contract sanity-check, asserted via the helper —
    // NOT a `#[cfg]` block inside the test body.
    assert_eq!(
        platform_bin_name("meta-hook").ends_with(".exe"),
        cfg!(windows),
        "platform_bin_name contract violation: {} on cfg!(windows)={}",
        platform_name,
        cfg!(windows)
    );

    // The generated hook command invokes the inner meta-hook in
    // --dry-run mode. We pass the resolved path through
    // `to_string_lossy()` so backslashes survive JSON encoding by
    // serde_json (which handles escaping).
    let inner_cmd = format!("{} --post-tool --dry-run", resolved_bin.to_string_lossy());
    gen_settings(&child, "PostToolUse", &inner_cmd);

    let envelope = json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Edit",
        "tool_input": {"file_path": edited.to_string_lossy()},
    })
    .to_string();

    let (code, stdout, stderr) = run_meta_hook(&["--post-tool"], &envelope, &workspace);
    assert_eq!(code, 0, "stderr: {stderr}");

    // The resolved binary path used in the generated settings must end
    // with `.exe` on Windows, and must not on other OSes.
    let resolved_str = resolved_bin.to_string_lossy();
    assert_eq!(
        resolved_str.ends_with(".exe"),
        cfg!(windows),
        "resolved binary path {resolved_str:?} disagrees with platform"
    );

    // The inner meta-hook ran in --dry-run mode and printed JSON to
    // stdout, which propagated through the outer process (run_hook uses
    // Stdio::inherit for the child). Every dry-run code path includes
    // the `"dry_run":true` key, so that's our portable substring marker.
    assert!(
        stdout.contains("\"dry_run\":true") || stdout.contains("\"dry_run\": true"),
        "outer stdout should contain inner dry-run JSON; got: {stdout:?}; stderr: {stderr:?}"
    );
}
