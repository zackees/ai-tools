//! Hook-event envelope parsing and per-mode target-path extraction.
//!
//! Claude Code's hook contract delivers a JSON envelope on stdin with at
//! least:
//! ```json
//! {
//!   "session_id": "...",
//!   "transcript_path": "...",
//!   "cwd": "...",
//!   "hook_event_name": "PreToolUse",
//!   "tool_name": "Edit",
//!   "tool_input": { "...": "..." }
//! }
//! ```
//! The exact shape of `tool_input` varies per tool. We parse only the
//! fields we need and tolerate the rest as `serde_json::Value`.

use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;

use crate::cli::Mode;

/// Subset of the hook envelope `meta-hook` consumes.
///
/// Unknown fields are preserved in `extra` so we can write the envelope
/// back out untouched (for Bash tools we never rewrite, only the
/// `tool_input.file_path` is normalized for Edit/Write/NotebookEdit).
#[derive(Debug, Clone, Deserialize)]
pub struct Envelope {
    #[serde(default)]
    pub hook_event_name: Option<String>,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub tool_input: Option<Value>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

impl Envelope {
    /// Parse the envelope from a JSON string.
    pub fn from_json(s: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(s)?)
    }

    /// Tool name string, lower-cased for matching convenience.
    pub fn tool_name_str(&self) -> Option<&str> {
        self.tool_name.as_deref()
    }
}

/// Tools whose `tool_input.file_path` is the authoritative target path.
pub fn is_file_path_tool(name: &str) -> bool {
    matches!(name, "Edit" | "Write" | "NotebookEdit" | "MultiEdit")
}

/// Extract the candidate target path from an envelope, per mode.
///
/// Returns `None` when:
/// * the mode does not operate on a tool input (UserPrompt / Stop / None), or
/// * no recognizable path field is present.
pub fn extract_target(env: &Envelope, mode: Mode) -> Option<PathBuf> {
    if !mode.is_tool_mode() {
        return None;
    }
    let input = env.tool_input.as_ref()?;
    let tool = env.tool_name_str().unwrap_or("");

    match mode {
        Mode::PreEdit | Mode::PostEdit => {
            // Edit-style modes always come from file_path.
            string_field(input, "file_path").map(PathBuf::from)
        }
        Mode::PreTool | Mode::PostTool => {
            if is_file_path_tool(tool) {
                if let Some(p) = string_field(input, "file_path") {
                    return Some(PathBuf::from(p));
                }
            }
            if let Some(p) = string_field(input, "cwd") {
                return Some(PathBuf::from(p));
            }
            if let Some(cmd) = string_field(input, "command") {
                return first_path_token(&cmd).map(PathBuf::from);
            }
            None
        }
        _ => None,
    }
}

/// Helper: pull a string-typed field from a JSON object.
fn string_field(v: &Value, key: &str) -> Option<String> {
    v.as_object()?.get(key)?.as_str().map(str::to_string)
}

/// Best-effort extraction of the first path-shaped token from a Bash
/// command string. Heuristic: tokenize on whitespace honoring single/
/// double quotes; return the first token that looks like a path
/// (contains `/`, `\`, or starts with `.` / `~` / a drive letter, and is
/// not a flag).
pub fn first_path_token(cmd: &str) -> Option<String> {
    for raw in tokenize(cmd) {
        let token = strip_quotes(&raw);
        if looks_like_path(&token) {
            return Some(token);
        }
    }
    None
}

fn tokenize(cmd: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut chars = cmd.chars().peekable();
    while let Some(c) = chars.next() {
        match quote {
            Some(q) if c == q => {
                cur.push(c);
                quote = None;
            }
            Some(_) => cur.push(c),
            None => match c {
                '\'' | '"' => {
                    cur.push(c);
                    quote = Some(c);
                }
                c if c.is_whitespace() => {
                    if !cur.is_empty() {
                        out.push(std::mem::take(&mut cur));
                    }
                }
                '\\' => {
                    // Treat backslash as a literal character on Windows-style paths;
                    // shell escaping is best-effort here.
                    cur.push(c);
                    if let Some(&next) = chars.peek() {
                        cur.push(next);
                        chars.next();
                    }
                }
                _ => cur.push(c),
            },
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn strip_quotes(s: &str) -> String {
    let bytes = s.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' || first == b'\'') && first == last {
            return s[1..s.len() - 1].to_string();
        }
    }
    s.to_string()
}

fn looks_like_path(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    if s.starts_with('-') {
        return false;
    }
    if s.starts_with('/') || s.starts_with("./") || s.starts_with("../") || s.starts_with('~') {
        return true;
    }
    // Windows drive letter: C:\ or C:/
    let bytes = s.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        return true;
    }
    // Otherwise require a directory separator somewhere.
    s.contains('/') || s.contains('\\')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn env_with_input(tool: &str, input: serde_json::Value) -> Envelope {
        Envelope::from_json(
            &json!({
                "hook_event_name": "PreToolUse",
                "tool_name": tool,
                "tool_input": input,
            })
            .to_string(),
        )
        .unwrap()
    }

    #[test]
    fn parses_minimal_envelope() {
        let raw = r#"{"hook_event_name":"PreToolUse","tool_name":"Edit","tool_input":{"file_path":"/tmp/x"}}"#;
        let env = Envelope::from_json(raw).unwrap();
        assert_eq!(env.tool_name.as_deref(), Some("Edit"));
        assert!(env.tool_input.is_some());
    }

    #[test]
    fn parses_envelope_with_extra_fields() {
        let raw = r#"{"hook_event_name":"PreToolUse","tool_name":"Edit","tool_input":{},"session_id":"abc","extra":42}"#;
        let env = Envelope::from_json(raw).unwrap();
        assert!(env.extra.contains_key("session_id"));
        assert!(env.extra.contains_key("extra"));
    }

    #[test]
    fn extracts_file_path_for_edit_modes() {
        let env = env_with_input("Edit", json!({"file_path": "/repo/file.rs"}));
        let p = extract_target(&env, Mode::PreEdit).unwrap();
        assert_eq!(p, PathBuf::from("/repo/file.rs"));
        let p = extract_target(&env, Mode::PostEdit).unwrap();
        assert_eq!(p, PathBuf::from("/repo/file.rs"));
    }

    #[test]
    fn pre_tool_prefers_file_path_for_edit_like_tools() {
        let env = env_with_input("Write", json!({"file_path": "/a/b.txt"}));
        let p = extract_target(&env, Mode::PreTool).unwrap();
        assert_eq!(p, PathBuf::from("/a/b.txt"));
    }

    #[test]
    fn pre_tool_falls_back_to_cwd() {
        let env = env_with_input("Bash", json!({"cwd": "/work/repo"}));
        let p = extract_target(&env, Mode::PreTool).unwrap();
        assert_eq!(p, PathBuf::from("/work/repo"));
    }

    #[test]
    fn pre_tool_falls_back_to_command_path() {
        let env = env_with_input("Bash", json!({"command": "cat /etc/hostname"}));
        let p = extract_target(&env, Mode::PreTool).unwrap();
        assert_eq!(p, PathBuf::from("/etc/hostname"));
    }

    #[test]
    fn pre_tool_returns_none_when_no_target() {
        let env = env_with_input("Bash", json!({"command": "echo hello"}));
        assert!(extract_target(&env, Mode::PreTool).is_none());
    }

    #[test]
    fn user_prompt_and_stop_have_no_target() {
        let env = env_with_input("Edit", json!({"file_path": "/a"}));
        assert!(extract_target(&env, Mode::UserPrompt).is_none());
        assert!(extract_target(&env, Mode::Stop).is_none());
        assert!(extract_target(&env, Mode::None).is_none());
    }

    #[test]
    fn tokenize_respects_quotes() {
        let toks = tokenize(r#"cat "/a b/c.txt" -n"#);
        assert_eq!(toks, vec!["cat", r#""/a b/c.txt""#, "-n"]);
    }

    #[test]
    fn first_path_token_handles_quoted_paths() {
        let p = first_path_token(r#"cat "/with space/file.txt""#).unwrap();
        assert_eq!(p, "/with space/file.txt");
    }

    #[test]
    fn first_path_token_skips_flags() {
        let p = first_path_token("ls --color /usr/bin").unwrap();
        assert_eq!(p, "/usr/bin");
    }

    #[test]
    fn first_path_token_handles_windows_drive() {
        let p = first_path_token(r#"type C:\Users\me\file.txt"#).unwrap();
        assert!(p.starts_with("C:"));
    }

    #[test]
    fn first_path_token_none_when_only_argv0() {
        assert!(first_path_token("echo hello world").is_none());
    }

    #[test]
    fn looks_like_path_examples() {
        assert!(looks_like_path("/a"));
        assert!(looks_like_path("./rel"));
        assert!(looks_like_path("../rel"));
        assert!(looks_like_path("~/x"));
        assert!(looks_like_path("a/b"));
        assert!(looks_like_path("C:\\x"));
        assert!(!looks_like_path("-n"));
        assert!(!looks_like_path("echo"));
        assert!(!looks_like_path(""));
    }
}
