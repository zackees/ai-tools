//! Spawn delegated hook commands, forward stdio, aggregate exit codes.
//!
//! Each `HookCommand` is run through the platform shell so the user's
//! `command` string can use pipes/redirects/quoting just like in a
//! regular `settings.json` invocation. The original envelope JSON is fed
//! on stdin; stdout/stderr inherit from the parent so they reach Claude.

use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::envelope::{is_file_path_tool, Envelope};
use crate::settings::HookCommand;

/// Spawn one hook command with `envelope_json` on stdin, `cwd` set to
/// the sub-repo root, and stdout/stderr inherited from the parent.
/// Returns the child's exit code (defaults to 1 if the process was
/// killed by a signal).
pub fn run_hook(hook: &HookCommand, cwd: &Path, envelope_json: &str) -> anyhow::Result<i32> {
    let mut child = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(&hook.command);
        c
    } else {
        let mut c = Command::new("sh");
        c.arg("-c").arg(&hook.command);
        c
    };
    child
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let mut spawned = child.spawn()?;
    if let Some(mut stdin) = spawned.stdin.take() {
        stdin.write_all(envelope_json.as_bytes())?;
        // Drop closes stdin.
    }
    let status = spawned.wait()?;
    Ok(status.code().unwrap_or(1))
}

/// Run every hook in sequence. Returns the first non-zero exit code
/// encountered, or 0 if all hooks succeeded.
pub fn run_all(hooks: &[HookCommand], cwd: &Path, envelope_json: &str) -> anyhow::Result<i32> {
    let mut first_failure: Option<i32> = None;
    for h in hooks {
        let code = run_hook(h, cwd, envelope_json)?;
        if code != 0 && first_failure.is_none() {
            first_failure = Some(code);
        }
    }
    Ok(first_failure.unwrap_or(0))
}

/// Re-encode the envelope with `tool_input.file_path` rewritten to be
/// relative to `sub_repo_root`, but **only** for tools whose `file_path`
/// is well-typed (`Edit` / `Write` / `NotebookEdit` / `MultiEdit`). All
/// other tools (notably `Bash`) get the envelope verbatim — rewriting an
/// arbitrary command string is brittle and we rely on the cwd switch
/// instead.
pub fn normalize_envelope(
    env: &Envelope,
    raw_json: &str,
    sub_repo_root: &Path,
) -> anyhow::Result<String> {
    let tool = env.tool_name_str().unwrap_or("");
    if !is_file_path_tool(tool) {
        return Ok(raw_json.to_string());
    }
    let mut v: Value = serde_json::from_str(raw_json)?;
    let Some(input) = v.get_mut("tool_input").and_then(Value::as_object_mut) else {
        return Ok(serde_json::to_string(&v)?);
    };
    if let Some(fp) = input.get("file_path").and_then(Value::as_str) {
        let abs = PathBuf::from(fp);
        if let Ok(rel) = abs.strip_prefix(sub_repo_root) {
            let rel_str = rel.to_string_lossy().to_string();
            input.insert("file_path".to_string(), Value::String(rel_str));
        }
    }
    Ok(serde_json::to_string(&v)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalize_rewrites_file_path_for_edit() {
        let raw = json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "Edit",
            "tool_input": {"file_path": "/repo/sub/src/file.rs"},
        })
        .to_string();
        let env = Envelope::from_json(&raw).unwrap();
        let out = normalize_envelope(&env, &raw, Path::new("/repo/sub")).unwrap();
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["tool_input"]["file_path"], "src/file.rs");
    }

    #[test]
    fn normalize_leaves_bash_alone() {
        let raw = json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": {"command": "ls /repo/sub"},
        })
        .to_string();
        let env = Envelope::from_json(&raw).unwrap();
        let out = normalize_envelope(&env, &raw, Path::new("/repo/sub")).unwrap();
        assert_eq!(out, raw, "Bash envelopes must pass through verbatim");
    }

    #[test]
    fn normalize_leaves_path_alone_when_not_under_subrepo() {
        let raw = json!({
            "tool_name": "Edit",
            "tool_input": {"file_path": "/elsewhere/file.rs"},
        })
        .to_string();
        let env = Envelope::from_json(&raw).unwrap();
        let out = normalize_envelope(&env, &raw, Path::new("/repo/sub")).unwrap();
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["tool_input"]["file_path"], "/elsewhere/file.rs");
    }

    #[test]
    fn run_all_returns_first_non_zero() {
        let tmp = tempfile::tempdir().unwrap();
        // "true" succeeds, "exit 7" fails, "exit 9" fails. We expect 7.
        let hooks = if cfg!(windows) {
            vec![
                HookCommand {
                    kind: Some("command".into()),
                    command: "cmd /C exit 0".into(),
                    timeout: None,
                },
                HookCommand {
                    kind: Some("command".into()),
                    command: "cmd /C exit 7".into(),
                    timeout: None,
                },
                HookCommand {
                    kind: Some("command".into()),
                    command: "cmd /C exit 9".into(),
                    timeout: None,
                },
            ]
        } else {
            vec![
                HookCommand {
                    kind: Some("command".into()),
                    command: "true".into(),
                    timeout: None,
                },
                HookCommand {
                    kind: Some("command".into()),
                    command: "exit 7".into(),
                    timeout: None,
                },
                HookCommand {
                    kind: Some("command".into()),
                    command: "exit 9".into(),
                    timeout: None,
                },
            ]
        };
        let code = run_all(&hooks, tmp.path(), "{}").unwrap();
        assert_eq!(code, 7);
    }
}
