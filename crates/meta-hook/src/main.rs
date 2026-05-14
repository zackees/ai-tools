//! `meta-hook` — native hook delegator for Claude Code / Codex sub-repos.
//!
//! Reads a hook-event envelope on stdin, walks up from the target path to
//! the enclosing `.git` root, loads that sub-repo's `.claude/settings.json`,
//! and runs any matching hook commands from inside the sub-repo root.
//!
//! See issue #1 for the full design + safety story.

use clap::Parser;
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use meta_hook::cli::{mode_of, Cli, Mode};
use meta_hook::discover::{enclosing_git_root_cached, is_within_session, DiscoverCache};
use meta_hook::dispatch::{normalize_envelope, run_all};
use meta_hook::envelope::{extract_target, Envelope};
use meta_hook::settings::{HookCommand, Settings};
use meta_hook::trust::{warning_for, TrustFile};

fn main() -> ExitCode {
    let cli = Cli::parse();
    let mode = mode_of(&cli);

    match run(&cli, mode) {
        Ok(code) => ExitCode::from(code as u8),
        Err(e) => {
            // Surface errors to stderr but stay out of the way of the
            // user's workflow: transparent no-op semantics mean we never
            // want to break a tool call because meta-hook itself blew up.
            eprintln!("meta-hook: error: {e}");
            ExitCode::SUCCESS
        }
    }
}

/// Top-level orchestration, factored out so it can return `Result`.
fn run(cli: &Cli, mode: Mode) -> anyhow::Result<i32> {
    // Bare invocation: print a one-liner and exit 0 (matches the previous
    // stub's behavior so existing wiring isn't surprised).
    if matches!(mode, Mode::None) {
        if cli.dry_run {
            println!("{{\"mode\":\"none\",\"dry_run\":true,\"status\":\"no-mode\"}}");
        } else {
            println!("meta-hook: no mode flag passed; exiting 0");
        }
        return Ok(0);
    }

    // UserPrompt / Stop: no target path, no delegation. Exit 0 transparently.
    if !mode.is_tool_mode() {
        if cli.dry_run {
            println!(
                "{{\"mode\":\"{}\",\"dry_run\":true,\"status\":\"no-target-mode\"}}",
                mode.as_str()
            );
        }
        return Ok(0);
    }

    // Read stdin (envelope JSON). Empty stdin → transparent no-op.
    let mut raw = String::new();
    std::io::stdin().read_to_string(&mut raw)?;
    if raw.trim().is_empty() {
        if cli.dry_run {
            println!(
                "{{\"mode\":\"{}\",\"dry_run\":true,\"status\":\"empty-stdin\"}}",
                mode.as_str()
            );
        }
        return Ok(0);
    }
    let env = match Envelope::from_json(&raw) {
        Ok(e) => e,
        Err(e) => {
            // Bad envelope is the harness's problem, not ours. Stay out
            // of the way: warn to stderr and exit 0.
            eprintln!("meta-hook: failed to parse envelope: {e}");
            return Ok(0);
        }
    };

    let Some(target_path) = extract_target(&env, mode) else {
        if cli.dry_run {
            println!(
                "{{\"mode\":\"{}\",\"dry_run\":true,\"status\":\"no-target-path\"}}",
                mode.as_str()
            );
        }
        return Ok(0);
    };

    // Resolve session cwd: env var if set (Claude provides this), else process cwd.
    let session_cwd = std::env::var_os("CLAUDE_PROJECT_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));

    // Walk up from the target path to its enclosing .git root.
    let cache = DiscoverCache::new();
    let Some(sub_root) = enclosing_git_root_cached(&cache, &target_path) else {
        emit_dry_run_no_delegation(cli, mode, &target_path, "no-git-root");
        return Ok(0);
    };

    // Safety: refuse to delegate outside the session subtree.
    if !is_within_session(&sub_root, &session_cwd) {
        emit_dry_run_no_delegation(cli, mode, &target_path, "outside-session-cwd");
        return Ok(0);
    }

    // Also: if the sub-repo *is* the session cwd, there is no nesting
    // — the parent harness already runs the top-level settings.json
    // hooks, so re-firing them would cause an infinite loop. Skip.
    let session_canon = session_cwd
        .canonicalize()
        .unwrap_or_else(|_| session_cwd.clone());
    let sub_canon = sub_root.canonicalize().unwrap_or_else(|_| sub_root.clone());
    if session_canon == sub_canon {
        emit_dry_run_no_delegation(cli, mode, &target_path, "session-is-subrepo");
        return Ok(0);
    }

    // Load the sub-repo's .claude/settings.json.
    let settings_path = sub_root.join(".claude").join("settings.json");
    let settings = match Settings::load(&settings_path)? {
        Some(s) => s,
        None => {
            emit_dry_run_no_delegation(cli, mode, &target_path, "no-settings-json");
            return Ok(0);
        }
    };

    let Some(event) = mode.event_name() else {
        return Ok(0);
    };
    let hooks = settings.hooks_for_event(event, env.tool_name_str());
    if hooks.is_empty() {
        emit_dry_run_no_delegation(cli, mode, &target_path, "no-matching-hooks");
        return Ok(0);
    }

    // Trust check (warn-only in v1).
    if let Some(trust_path) = TrustFile::default_path() {
        if let Ok(Some(trust)) = TrustFile::load(&trust_path) {
            if let Some(msg) = warning_for(Some(&trust), &sub_root) {
                eprintln!("{msg}");
            }
        }
    }

    // Build the envelope to send downstream. For Edit/Write/NotebookEdit
    // we rewrite tool_input.file_path to be relative to sub_root.
    let downstream_envelope = normalize_envelope(&env, &raw, &sub_root)?;

    if cli.dry_run {
        emit_dry_run_plan(mode, event, &sub_root, &hooks);
        return Ok(0);
    }

    run_all(&hooks, &sub_root, &downstream_envelope)
}

fn emit_dry_run_no_delegation(cli: &Cli, mode: Mode, target: &std::path::Path, status: &str) {
    if !cli.dry_run {
        return;
    }
    let v = serde_json::json!({
        "mode": mode.as_str(),
        "dry_run": true,
        "status": status,
        "target_path": target.display().to_string(),
        "delegation": null,
    });
    println!("{v}");
}

fn emit_dry_run_plan(mode: Mode, event: &str, sub_root: &std::path::Path, hooks: &[HookCommand]) {
    let commands: Vec<&str> = hooks.iter().map(|h| h.command.as_str()).collect();
    let v = serde_json::json!({
        "mode": mode.as_str(),
        "dry_run": true,
        "status": "would-delegate",
        "event": event,
        "target": sub_root.display().to_string(),
        "hooks": commands,
        "would_run": commands,
    });
    println!("{v}");
}
