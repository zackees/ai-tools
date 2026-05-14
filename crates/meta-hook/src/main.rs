//! `meta-hook` — native hook delegator for Claude Code / Codex sub-repos.
//!
//! This is the scaffolded entry point produced by issue #2 (repo bootstrap).
//! The actual delegation logic — walking up to the enclosing `.git` directory
//! that owns the touched file and re-firing that sub-repo's hook command —
//! is tracked in issue #1 and intentionally **not** implemented here.
//!
//! For now every mode is a no-op stub that prints the parsed mode + a
//! "hello" line and exits 0, so:
//!   * the binary is buildable and installable end-to-end,
//!   * CI has something concrete to compile, test, and release,
//!   * downstream consumers can drop the binary into `settings.json` and
//!     verify wiring without the binary doing anything destructive yet.

use clap::Parser;
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(
    name = "meta-hook",
    about = "Native hook delegator for Claude Code / Codex sub-repos (stub).",
    long_about = None,
    version,
)]
struct Cli {
    /// Dispatch as a PreToolUse hook.
    #[arg(long, group = "mode")]
    pre_tool: bool,

    /// Dispatch as a PostToolUse hook.
    #[arg(long, group = "mode")]
    post_tool: bool,

    /// Dispatch before an Edit/Write/NotebookEdit tool call.
    #[arg(long, group = "mode")]
    pre_edit: bool,

    /// Dispatch after an Edit/Write/NotebookEdit tool call.
    #[arg(long, group = "mode")]
    post_edit: bool,

    /// Dispatch as a UserPromptSubmit hook.
    #[arg(long, group = "mode")]
    user_prompt: bool,

    /// Dispatch as a Stop / SubagentStop hook.
    #[arg(long, group = "mode")]
    stop: bool,

    /// Print the resolved delegation plan as JSON and exit (no execution).
    #[arg(long)]
    dry_run: bool,
}

/// Symbolic name for each hook mode the binary understands.
///
/// Kept as a separate enum (instead of leaning on the `clap` boolean group)
/// so the resolution logic is unit-testable without instantiating clap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    PreTool,
    PostTool,
    PreEdit,
    PostEdit,
    UserPrompt,
    Stop,
    /// No mode flag was passed — caller invoked `meta-hook` bare. The
    /// resolver returns this so `main` can print help + exit 0 rather
    /// than failing loudly. (Real hook events always pass a mode flag.)
    None,
}

impl Mode {
    fn as_str(self) -> &'static str {
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
}

/// Map the boolean flags on the parsed CLI struct to a single `Mode`.
///
/// Clap's `group = "mode"` enforces mutual exclusion at parse time, so we
/// will see at most one `true` here. This is split out from `main` so the
/// mapping itself is covered by a unit test.
fn mode_of(cli: &Cli) -> Mode {
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

fn main() -> ExitCode {
    let cli = Cli::parse();
    let mode = mode_of(&cli);

    if cli.dry_run {
        // Hand-rolled tiny JSON so we don't have to pull in serde just for
        // a 3-field stub. Keeps the binary small and the dep graph minimal.
        println!(
            "{{\"mode\":\"{}\",\"dry_run\":true,\"status\":\"stub\"}}",
            mode.as_str()
        );
        return ExitCode::SUCCESS;
    }

    // Stub behavior: announce ourselves and exit 0 so we are transparent
    // to whichever harness is wiring us in. Real delegation lives in #1.
    println!("meta-hook stub: mode={} hello", mode.as_str());
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    /// Sanity: the clap definition compiles + validates. Catches future
    /// breakage where a flag rename or group constraint goes wrong.
    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    /// Each flag maps to exactly one `Mode` value.
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

    /// No flag at all resolves to `Mode::None` (graceful, not a panic).
    #[test]
    fn no_flag_resolves_to_none() {
        let cli = Cli::parse_from(["meta-hook"]);
        assert_eq!(mode_of(&cli), Mode::None);
        assert!(!cli.dry_run);
    }

    /// `--dry-run` is orthogonal to mode and can be combined with any mode.
    #[test]
    fn dry_run_combines_with_mode() {
        let cli = Cli::parse_from(["meta-hook", "--pre-tool", "--dry-run"]);
        assert_eq!(mode_of(&cli), Mode::PreTool);
        assert!(cli.dry_run);
    }

    /// Conflicting mode flags are rejected by clap's `group = "mode"`.
    #[test]
    fn conflicting_modes_are_rejected() {
        let result = Cli::try_parse_from(["meta-hook", "--pre-tool", "--post-tool"]);
        assert!(result.is_err(), "clap should reject two mode flags");
    }
}
