//! End-to-end integration tests for the `meta-hook` binary.
//!
//! Builds a workspace tempdir like:
//! ```
//! /workspace/         session cwd, no .claude
//!   project-a/        .git, .claude/settings.json with PreToolUse hook that writes a marker
//!   project-b/        .git, .claude/settings.json with PostToolUse only
//!   shared.txt        file owned by no sub-repo
//! ```
//! and runs `meta-hook --pre-tool` (etc.) against envelopes pointing into
//! that tree.

use serde_json::json;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::tempdir;

/// Locate the freshly-built `meta-hook` binary.
///
/// Uses `CARGO_BIN_EXE_meta-hook` when running under `cargo test`; falls
/// back to `target/debug/meta-hook` otherwise.
fn meta_hook_bin() -> PathBuf {
    if let Some(p) = option_env!("CARGO_BIN_EXE_meta-hook") {
        return PathBuf::from(p);
    }
    let exe = if cfg!(windows) {
        "meta-hook.exe"
    } else {
        "meta-hook"
    };
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target")
        .join("debug")
        .join(exe)
}

/// Build the fixture tree and return `(workspace, project_a, project_b, shared_file)`.
fn build_fixture() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let tmp = tempdir().unwrap();
    let ws = tmp.path().to_path_buf();
    let pa = ws.join("project-a");
    let pb = ws.join("project-b");
    fs::create_dir_all(&pa).unwrap();
    fs::create_dir_all(&pb).unwrap();
    fs::create_dir_all(pa.join(".git")).unwrap();
    fs::create_dir_all(pb.join(".git")).unwrap();
    fs::create_dir_all(pa.join(".claude")).unwrap();
    fs::create_dir_all(pb.join(".claude")).unwrap();
    fs::create_dir_all(pa.join("src")).unwrap();
    fs::create_dir_all(pb.join("src")).unwrap();

    // project-a: PreToolUse hook writes a fixed-name marker file. The
    // command runs with `cwd` = sub-repo root (see dispatch.rs), so a
    // bare filename lands in the sub-repo. `echo > FILE` works on both
    // cmd.exe and POSIX shells.
    let pa_settings = json!({
        "hooks": {
            "PreToolUse": [
                {"matcher": "*", "hooks": [
                    {"type": "command", "command": "echo pre-a > .pre-tool-marker"}
                ]}
            ]
        }
    });
    fs::write(
        pa.join(".claude").join("settings.json"),
        serde_json::to_string_pretty(&pa_settings).unwrap(),
    )
    .unwrap();

    // project-b: PostToolUse only — should NOT fire for --pre-tool.
    let pb_settings = json!({
        "hooks": {
            "PostToolUse": [
                {"matcher": "*", "hooks": [
                    {"type": "command", "command": "echo post-b > .post-tool-marker"}
                ]}
            ]
        }
    });
    fs::write(
        pb.join(".claude").join("settings.json"),
        serde_json::to_string_pretty(&pb_settings).unwrap(),
    )
    .unwrap();

    let shared = ws.join("shared.txt");
    fs::write(&shared, "shared").unwrap();
    (tmp, pa, pb, shared)
}

/// Run `meta-hook --pre-tool` with the given envelope JSON on stdin and
/// the workspace as `CLAUDE_PROJECT_DIR`.
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
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(envelope.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let code = out.status.code().unwrap_or(-1);
    (
        code,
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn pre_tool_delegates_to_project_a() {
    let (tmp, pa, _pb, _shared) = build_fixture();
    let file = pa.join("src").join("x.rs");
    fs::write(&file, "").unwrap();

    let envelope = json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Edit",
        "tool_input": {"file_path": file.to_string_lossy()},
    })
    .to_string();

    let (code, _stdout, stderr) = run_meta_hook(&["--pre-tool"], &envelope, tmp.path());
    assert_eq!(code, 0, "stderr: {stderr}");
    let marker = pa.join(".pre-tool-marker");
    assert!(
        marker.exists(),
        "pre-tool marker not written; stderr: {stderr}"
    );
}

#[test]
fn pre_tool_does_not_fire_post_only_subrepo() {
    let (tmp, _pa, pb, _shared) = build_fixture();
    let file = pb.join("src").join("y.rs");
    fs::write(&file, "").unwrap();

    let envelope = json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Edit",
        "tool_input": {"file_path": file.to_string_lossy()},
    })
    .to_string();

    let (code, _stdout, stderr) = run_meta_hook(&["--pre-tool"], &envelope, tmp.path());
    assert_eq!(code, 0, "stderr: {stderr}");
    // project-b has only PostToolUse, so no marker should appear.
    assert!(!pb.join(".pre-tool-marker").exists());
    assert!(!pb.join(".post-tool-marker").exists());
}

#[test]
fn post_tool_fires_project_b_post_hook() {
    let (tmp, _pa, pb, _shared) = build_fixture();
    let file = pb.join("src").join("y.rs");
    fs::write(&file, "").unwrap();

    let envelope = json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Edit",
        "tool_input": {"file_path": file.to_string_lossy()},
    })
    .to_string();

    let (code, _stdout, stderr) = run_meta_hook(&["--post-tool"], &envelope, tmp.path());
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(pb.join(".post-tool-marker").exists(), "stderr: {stderr}");
}

#[test]
fn workspace_file_outside_any_subrepo_is_noop() {
    let (tmp, pa, pb, shared) = build_fixture();
    let envelope = json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Edit",
        "tool_input": {"file_path": shared.to_string_lossy()},
    })
    .to_string();

    let (code, _stdout, _stderr) = run_meta_hook(&["--pre-tool"], &envelope, tmp.path());
    assert_eq!(code, 0);
    assert!(!pa.join(".pre-tool-marker").exists());
    assert!(!pb.join(".post-tool-marker").exists());
}

#[test]
fn bogus_file_path_is_noop() {
    let (tmp, pa, pb, _shared) = build_fixture();
    let envelope = json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Edit",
        "tool_input": {"file_path": "/definitely/nowhere/xyz.txt"},
    })
    .to_string();

    let (code, _stdout, _stderr) = run_meta_hook(&["--pre-tool"], &envelope, tmp.path());
    assert_eq!(code, 0);
    assert!(!pa.join(".pre-tool-marker").exists());
    assert!(!pb.join(".post-tool-marker").exists());
}

#[test]
fn dry_run_emits_json_plan() {
    let (tmp, pa, _pb, _shared) = build_fixture();
    let file = pa.join("src").join("x.rs");
    fs::write(&file, "").unwrap();

    let envelope = json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Edit",
        "tool_input": {"file_path": file.to_string_lossy()},
    })
    .to_string();

    let (code, stdout, _stderr) =
        run_meta_hook(&["--pre-tool", "--dry-run"], &envelope, tmp.path());
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("dry-run output should be JSON: {e}\nstdout was: {stdout}"));
    assert_eq!(v["status"], "would-delegate");
    assert_eq!(v["event"], "PreToolUse");
    assert!(!v["hooks"].as_array().unwrap().is_empty());
    // No marker was written because dry-run short-circuits.
    assert!(!pa.join(".pre-tool-marker").exists());
}

#[test]
fn user_prompt_mode_is_noop() {
    let (tmp, pa, pb, _shared) = build_fixture();
    let envelope = json!({
        "hook_event_name": "UserPromptSubmit",
        "prompt": "hi",
    })
    .to_string();
    let (code, _stdout, _stderr) = run_meta_hook(&["--user-prompt"], &envelope, tmp.path());
    assert_eq!(code, 0);
    assert!(!pa.join(".pre-tool-marker").exists());
    assert!(!pb.join(".post-tool-marker").exists());
}

#[test]
fn empty_stdin_is_noop() {
    let (tmp, pa, _pb, _shared) = build_fixture();
    let (code, _stdout, _stderr) = run_meta_hook(&["--pre-tool"], "", tmp.path());
    assert_eq!(code, 0);
    assert!(!pa.join(".pre-tool-marker").exists());
}
