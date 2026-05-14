//! clap definitions, mode enum, and mode → Claude-event-name mapping.
//!
//! The CLI surface here matches the original scaffold so existing
//! `settings.json` wiring keeps working unchanged.

use clap::Parser;

/// Top-level CLI for `meta-hook`.
#[derive(Parser, Debug)]
#[command(
    name = "meta-hook",
    about = "Native hook delegator for Claude Code / Codex sub-repos.",
    long_about = None,
    version,
)]
pub struct Cli {
    /// Dispatch as a PreToolUse hook.
    #[arg(long, group = "mode")]
    pub pre_tool: bool,

    /// Dispatch as a PostToolUse hook.
    #[arg(long, group = "mode")]
    pub post_tool: bool,

    /// Dispatch before an Edit/Write/NotebookEdit tool call.
    #[arg(long, group = "mode")]
    pub pre_edit: bool,

    /// Dispatch after an Edit/Write/NotebookEdit tool call.
    #[arg(long, group = "mode")]
    pub post_edit: bool,

    /// Dispatch as a UserPromptSubmit hook.
    #[arg(long, group = "mode")]
    pub user_prompt: bool,

    /// Dispatch as a Stop / SubagentStop hook.
    #[arg(long, group = "mode")]
    pub stop: bool,

    /// Print the resolved delegation plan as JSON and exit (no execution).
    #[arg(long)]
    pub dry_run: bool,
}

/// Symbolic name for each hook mode the binary understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    PreTool,
    PostTool,
    PreEdit,
    PostEdit,
    UserPrompt,
    Stop,
    /// No mode flag was passed — caller invoked `meta-hook` bare.
    None,
}

impl Mode {
    /// CLI-flag-style name (matches the dashed flag name).
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::PreTool => "pre-tool",
            Mode::PostTool => "post-tool",
            Mode::PreEdit => "pre-edit",
            Mode::PostEdit => "post-edit",
            Mode::UserPrompt => "user-prompt",
            Mode::Stop => "stop",
            Mode::None => "none",
        }
    }

    /// Claude Code hook-event name that this mode dispatches under.
    ///
    /// Used to filter `.claude/settings.json` `hooks.<EventName>` entries
    /// when re-firing the delegated hook. `None` for the bare invocation
    /// (we just exit 0 in main).
    pub fn event_name(self) -> Option<&'static str> {
        match self {
            Mode::PreTool | Mode::PreEdit => Some("PreToolUse"),
            Mode::PostTool | Mode::PostEdit => Some("PostToolUse"),
            Mode::UserPrompt => Some("UserPromptSubmit"),
            Mode::Stop => Some("Stop"),
            Mode::None => None,
        }
    }

    /// Modes that operate on a tool-input payload (have a target path).
    /// `UserPrompt` and `Stop` carry no path; we short-circuit in main.
    pub fn is_tool_mode(self) -> bool {
        matches!(
            self,
            Mode::PreTool | Mode::PostTool | Mode::PreEdit | Mode::PostEdit
        )
    }
}

/// Map the boolean flags on the parsed CLI struct to a single `Mode`.
///
/// Clap's `group = "mode"` enforces mutual exclusion at parse time, so we
/// will see at most one `true` here.
pub fn mode_of(cli: &Cli) -> Mode {
    if cli.pre_tool {
        Mode::PreTool
    } else if cli.post_tool {
        Mode::PostTool
    } else if cli.pre_edit {
        Mode::PreEdit
    } else if cli.post_edit {
        Mode::PostEdit
    } else if cli.user_prompt {
        Mode::UserPrompt
    } else if cli.stop {
        Mode::Stop
    } else {
        Mode::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn each_flag_maps_to_its_mode() {
        let cases = [
            (
                "pre-tool",
                Cli::parse_from(["meta-hook", "--pre-tool"]),
                Mode::PreTool,
            ),
            (
                "post-tool",
                Cli::parse_from(["meta-hook", "--post-tool"]),
                Mode::PostTool,
            ),
            (
                "pre-edit",
                Cli::parse_from(["meta-hook", "--pre-edit"]),
                Mode::PreEdit,
            ),
            (
                "post-edit",
                Cli::parse_from(["meta-hook", "--post-edit"]),
                Mode::PostEdit,
            ),
            (
                "user-prompt",
                Cli::parse_from(["meta-hook", "--user-prompt"]),
                Mode::UserPrompt,
            ),
            ("stop", Cli::parse_from(["meta-hook", "--stop"]), Mode::Stop),
        ];

        for (label, cli, expected) in cases {
            assert_eq!(
                mode_of(&cli),
                expected,
                "flag {label} should map to {expected:?}"
            );
            assert_eq!(mode_of(&cli).as_str(), label);
        }
    }

    #[test]
    fn no_flag_resolves_to_none() {
        let cli = Cli::parse_from(["meta-hook"]);
        assert_eq!(mode_of(&cli), Mode::None);
        assert!(!cli.dry_run);
    }

    #[test]
    fn dry_run_combines_with_mode() {
        let cli = Cli::parse_from(["meta-hook", "--pre-tool", "--dry-run"]);
        assert_eq!(mode_of(&cli), Mode::PreTool);
        assert!(cli.dry_run);
    }

    #[test]
    fn conflicting_modes_are_rejected() {
        let result = Cli::try_parse_from(["meta-hook", "--pre-tool", "--post-tool"]);
        assert!(result.is_err(), "clap should reject two mode flags");
    }

    #[test]
    fn event_names_match_spec() {
        assert_eq!(Mode::PreTool.event_name(), Some("PreToolUse"));
        assert_eq!(Mode::PreEdit.event_name(), Some("PreToolUse"));
        assert_eq!(Mode::PostTool.event_name(), Some("PostToolUse"));
        assert_eq!(Mode::PostEdit.event_name(), Some("PostToolUse"));
        assert_eq!(Mode::UserPrompt.event_name(), Some("UserPromptSubmit"));
        assert_eq!(Mode::Stop.event_name(), Some("Stop"));
        assert_eq!(Mode::None.event_name(), None);
    }

    #[test]
    fn is_tool_mode_classification() {
        assert!(Mode::PreTool.is_tool_mode());
        assert!(Mode::PostTool.is_tool_mode());
        assert!(Mode::PreEdit.is_tool_mode());
        assert!(Mode::PostEdit.is_tool_mode());
        assert!(!Mode::UserPrompt.is_tool_mode());
        assert!(!Mode::Stop.is_tool_mode());
        assert!(!Mode::None.is_tool_mode());
    }
}
