//! Parse `<sub-repo>/.claude/settings.json` and pick out hook commands
//! whose event name matches the current mode and whose matcher (if any)
//! accepts the tool name on the envelope.
//!
//! The Claude Code settings shape, abbreviated:
//! ```json
//! {
//!   "hooks": {
//!     "PreToolUse": [
//!       { "matcher": "Edit|Write", "hooks": [
//!           { "type": "command", "command": "..." }
//!       ]}
//!     ]
//!   }
//! }
//! ```
//! `matcher` is either omitted/empty (matches any tool), `"*"` (matches
//! any tool), or a regex/alternation on tool name.

use serde::Deserialize;
use std::fs;
use std::path::Path;

/// Whole settings document; we only care about `hooks`.
#[derive(Debug, Deserialize, Default)]
pub struct Settings {
    #[serde(default)]
    pub hooks: std::collections::BTreeMap<String, Vec<HookGroup>>,
}

/// A single `{matcher, hooks: [...]}` entry under e.g. `PreToolUse`.
#[derive(Debug, Deserialize, Clone)]
pub struct HookGroup {
    #[serde(default)]
    pub matcher: Option<String>,
    #[serde(default)]
    pub hooks: Vec<HookCommand>,
}

/// One concrete hook to spawn.
#[derive(Debug, Deserialize, Clone)]
pub struct HookCommand {
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    pub command: String,
    #[serde(default)]
    pub timeout: Option<u64>,
}

impl Settings {
    /// Load from a `.claude/settings.json` path. Returns `Ok(None)` if
    /// the file is absent — that's the normal "no hooks here" path.
    pub fn load(path: &Path) -> anyhow::Result<Option<Self>> {
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(path)?;
        let parsed: Settings = serde_json::from_str(&raw)?;
        Ok(Some(parsed))
    }

    /// All hook commands registered for `event` whose matcher accepts
    /// `tool_name` (or have no matcher).
    pub fn hooks_for_event(&self, event: &str, tool_name: Option<&str>) -> Vec<HookCommand> {
        let Some(groups) = self.hooks.get(event) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for g in groups {
            if matches_tool(g.matcher.as_deref(), tool_name) {
                out.extend(g.hooks.iter().cloned());
            }
        }
        out
    }
}

/// Tool-name matcher.
///
/// * `None` / empty / `"*"` → match anything.
/// * Otherwise the matcher is parsed as a `|`-separated alternation of
///   plain tool names. (Claude Code accepts regex here; we implement the
///   common simple-alternation case and treat the rest as a literal
///   match.) Spaces around `|` are tolerated.
pub fn matches_tool(matcher: Option<&str>, tool_name: Option<&str>) -> bool {
    let pattern = matcher.unwrap_or("").trim();
    if pattern.is_empty() || pattern == "*" {
        return true;
    }
    let Some(name) = tool_name else {
        return false;
    };
    for part in pattern.split('|') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if part == "*" || part == name {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn matcher_wildcard_accepts_anything() {
        assert!(matches_tool(None, Some("Edit")));
        assert!(matches_tool(Some(""), Some("Edit")));
        assert!(matches_tool(Some("*"), Some("Edit")));
        assert!(matches_tool(Some("*"), None));
    }

    #[test]
    fn matcher_alternation() {
        assert!(matches_tool(Some("Edit|Write"), Some("Edit")));
        assert!(matches_tool(Some("Edit|Write"), Some("Write")));
        assert!(!matches_tool(Some("Edit|Write"), Some("Bash")));
    }

    #[test]
    fn matcher_literal_no_match_when_tool_missing() {
        assert!(!matches_tool(Some("Edit"), None));
    }

    #[test]
    fn load_returns_none_when_absent() {
        let tmp = tempdir().unwrap();
        let p = tmp.path().join("nope.json");
        let got = Settings::load(&p).unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn load_parses_and_filters() {
        let tmp = tempdir().unwrap();
        let p = tmp.path().join("settings.json");
        fs::write(
            &p,
            r#"{
              "hooks": {
                "PreToolUse": [
                  {"matcher": "Edit|Write", "hooks": [
                    {"type": "command", "command": "echo pre"}
                  ]},
                  {"matcher": "Bash", "hooks": [
                    {"type": "command", "command": "echo bash"}
                  ]}
                ],
                "PostToolUse": [
                  {"hooks": [
                    {"type": "command", "command": "echo post"}
                  ]}
                ]
              }
            }"#,
        )
        .unwrap();

        let s = Settings::load(&p).unwrap().unwrap();
        let pre_edit = s.hooks_for_event("PreToolUse", Some("Edit"));
        assert_eq!(pre_edit.len(), 1);
        assert_eq!(pre_edit[0].command, "echo pre");

        let pre_bash = s.hooks_for_event("PreToolUse", Some("Bash"));
        assert_eq!(pre_bash.len(), 1);
        assert_eq!(pre_bash[0].command, "echo bash");

        let post_any = s.hooks_for_event("PostToolUse", Some("Anything"));
        assert_eq!(post_any.len(), 1);
        assert_eq!(post_any[0].command, "echo post");

        let unknown_event = s.hooks_for_event("UserPromptSubmit", None);
        assert!(unknown_event.is_empty());
    }

    #[test]
    fn load_rejects_invalid_json() {
        let tmp = tempdir().unwrap();
        let p = tmp.path().join("bad.json");
        fs::write(&p, "{ not json").unwrap();
        assert!(Settings::load(&p).is_err());
    }
}
