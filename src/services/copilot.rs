// src/services/copilot.rs
//
// WeaveLang Co-pilot agent — routes `$` prefixed terminal messages to an LLM
// and can autonomously execute plans from the workspace copilot/ directory.

use crate::services::llm_client::LlmService;
use std::path::{Path, PathBuf};

/// Hardcoded system prompt for the co-pilot agent.
/// This is intentionally built into the binary — it defines the agent's identity
/// and operational boundaries. The runbook (workspace file) provides domain knowledge.
pub const COPILOT_SYSTEM_PROMPT: &str = r#"You are the WeaveLang Co-pilot, an autonomous production agent embedded in the WeaveLang Studio application. You help the user produce language-learning content through WeaveLang's terminal command interface.

## Your Capabilities
- You execute WeaveLang terminal commands by outputting them on lines prefixed with `CMD:`.
- You read workspace files (goal, plan, journal, runbook) to understand what to do.
- You report status and results in plain text.
- You can ask the user clarifying questions when needed.

## Command Execution Protocol
To execute a terminal command, output a line starting with `CMD:` followed by the command:
    CMD: av status
    CMD: generate_weave all
    CMD: av generate audio next

Each CMD: line will be executed and you will see the output in the next turn.
You may output multiple CMD: lines in a single response — they execute sequentially.

## Rules
1. Only use commands documented in the runbook. Never invent commands.
2. If you are unsure about the exact syntax, run `CMD: help` or `CMD: av help` first to get the current command list from the app.
3. Never delete files or data. Use mark/unmark, not destructive operations.
4. If a command fails, analyze the error. Try `help` to discover the correct command syntax, then retry. If still failing, ask the user.
5. After executing commands, report what happened concisely.
6. If you have nothing to do, say so. Don't fabricate tasks.
7. When running autonomously from a goal file, update the journal with progress.
8. Stop and ask the user if anything is ambiguous or risky.
9. Log significant milestones to the journal: `CMD: copilot journal <text>`. Examples: completing a pipeline step, errors encountered, tasks finished. This survives restarts and helps you pick up where you left off.
10. Your conversation history is automatically saved and restored across restarts. On restart, review the history and journal to understand where you left off.

## Context
You are running inside the WeaveLang terminal. The user communicates with you using the `$` prefix. Your responses appear in the terminal as `[copilot]` lines.
"#;

/// Build a context message from the workspace copilot files.
/// Reads _runbook.md, _goal.toml, _plan.toml, and _journal.md (last 50 lines).
pub fn build_workspace_context(workspace_dir: &Path) -> String {
    let copilot_dir = workspace_dir.join("copilot");
    let mut context = String::new();

    // Runbook
    let runbook_path = copilot_dir.join("_runbook.md");
    if let Ok(content) = std::fs::read_to_string(&runbook_path) {
        context.push_str("## Runbook\n");
        context.push_str(&content);
        context.push_str("\n\n");
    }

    // Goal
    let goal_path = copilot_dir.join("_goal.toml");
    if let Ok(content) = std::fs::read_to_string(&goal_path) {
        context.push_str("## Current Goal (_goal.toml)\n```toml\n");
        context.push_str(&content);
        context.push_str("\n```\n\n");
    }

    // Plan
    let plan_path = copilot_dir.join("_plan.toml");
    if let Ok(content) = std::fs::read_to_string(&plan_path) {
        context.push_str("## Current Plan (_plan.toml)\n```toml\n");
        context.push_str(&content);
        context.push_str("\n```\n\n");
    }

    // Journal (last 50 lines to keep context manageable)
    let journal_path = copilot_dir.join("_journal.md");
    if let Ok(content) = std::fs::read_to_string(&journal_path) {
        let lines: Vec<&str> = content.lines().collect();
        let start = if lines.len() > 50 { lines.len() - 50 } else { 0 };
        context.push_str("## Recent Journal (last 50 lines)\n");
        for line in &lines[start..] {
            context.push_str(line);
            context.push('\n');
        }
        context.push('\n');
    }

    context
}

/// Check whether _goal.toml has any actionable content (non-empty chapters, any production step enabled).
pub fn has_pending_goal(workspace_dir: &Path) -> bool {
    let goal_path = workspace_dir.join("copilot").join("_goal.toml");
    let content = match std::fs::read_to_string(&goal_path) {
        Ok(c) => c,
        Err(_) => return false,
    };

    // Quick heuristic checks:
    // 1. chapters must be non-empty (not just `[]`)
    // 2. At least one production step must be true
    let has_chapters = content.contains("chapters = [\"") || content.contains("chapters = ['");
    let has_steps = content.contains("= true");

    has_chapters && has_steps
}

/// Extract CMD: lines from a copilot LLM response.
pub fn extract_commands(response: &str) -> Vec<String> {
    response
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("CMD:") {
                Some(trimmed[4..].trim().to_string())
            } else {
                None
            }
        })
        .collect()
}

/// Send a message to the copilot LLM and get a response.
/// `history` is the conversation so far: Vec<(role, text)> where role is "user" or "assistant".
/// The workspace context is prepended to the system prompt.
pub fn send_copilot_message(
    llm: &LlmService,
    model_alias: &str,
    system_prompt: &str,
    workspace_context: &str,
    history: &[(String, String)],
    user_message: &str,
) -> Result<String, String> {
    // Build full system prompt with workspace context
    let full_system = if workspace_context.is_empty() {
        system_prompt.to_string()
    } else {
        format!("{}\n\n---\n\n# Workspace Context\n\n{}", system_prompt, workspace_context)
    };

    // Build conversation as a single user message (multi-turn via message concatenation
    // since the LLM client has a simple complete(model, system, user) interface).
    //
    // Compaction strategy: keep the last RECENT_WINDOW entries at full fidelity.
    // Older entries get their content truncated to MAX_OLD_CHARS to prevent context explosion.
    const RECENT_WINDOW: usize = 10;
    const MAX_OLD_CHARS: usize = 300;

    let mut conversation = String::new();
    let total = history.len();
    let old_cutoff = if total > RECENT_WINDOW { total - RECENT_WINDOW } else { 0 };

    for (i, (role, text)) in history.iter().enumerate() {
        let display_text = if i < old_cutoff && text.len() > MAX_OLD_CHARS {
            let truncated: String = text.chars().take(MAX_OLD_CHARS).collect();
            format!("{}... [truncated]", truncated)
        } else {
            text.clone()
        };

        if role == "user" {
            conversation.push_str(&format!("[User]: {}\n\n", display_text));
        } else {
            conversation.push_str(&format!("[Assistant]: {}\n\n", display_text));
        }
    }
    conversation.push_str(&format!("[User]: {}\n", user_message));

    llm.complete(model_alias, &full_system, &conversation)
}

/// Get the copilot directory path for a workspace.
pub fn copilot_dir(workspace_dir: &Path) -> PathBuf {
    workspace_dir.join("copilot")
}

// ---------------------------------------------------------------------------
// Session persistence — save/load copilot_history across app restarts
// ---------------------------------------------------------------------------

const SESSION_FILE: &str = "_session.json";

/// Save the copilot conversation history and turn counter to disk.
pub fn save_session(
    workspace_dir: &Path,
    history: &[(String, String)],
    turns: u32,
) {
    if history.is_empty() {
        return;
    }
    let session_path = copilot_dir(workspace_dir).join(SESSION_FILE);
    let data = serde_json::json!({
        "history": history,
        "turns": turns,
    });
    if let Ok(json) = serde_json::to_string_pretty(&data) {
        let _ = std::fs::write(&session_path, json);
    }
}

/// Load a previously saved copilot session. Returns (history, turns).
pub fn load_session(workspace_dir: &Path) -> (Vec<(String, String)>, u32) {
    let session_path = copilot_dir(workspace_dir).join(SESSION_FILE);
    let content = match std::fs::read_to_string(&session_path) {
        Ok(c) => c,
        Err(_) => return (Vec::new(), 0),
    };
    let val: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return (Vec::new(), 0),
    };

    let history = val["history"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let pair = item.as_array()?;
                    Some((
                        pair.get(0)?.as_str()?.to_string(),
                        pair.get(1)?.as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    let turns = val["turns"].as_u64().unwrap_or(0) as u32;
    (history, turns)
}

// ---------------------------------------------------------------------------
// Journal append
// ---------------------------------------------------------------------------

/// Append a timestamped entry to _journal.md.
pub fn append_journal(workspace_dir: &Path, text: &str) {
    let journal_path = copilot_dir(workspace_dir).join("_journal.md");
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let entry = format!("\n- [{}] {}\n", timestamp, text);
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&journal_path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, entry.as_bytes()));
}
