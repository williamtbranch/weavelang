// src/app/terminal.rs
//
// Terminal command parsing and execution — shared by the interactive REPL and the API server.
// All output is returned as String rather than printed, so both frontends can use it.

use crate::app::commands::{AppCommand, AvTarget, TerminalCommand};
use crate::app::engine::Engine;
use crate::domain::mapping_logic::apply_llm_mapping;
use crate::domain::mapping::TierMapping;
use crate::domain::sentence::Sentence;
use crate::simulation::frequency_manager;

/// Format a lemma with its frequency rank: `viejo<453>` or `viejo<->` if not found.
fn format_lemma_with_rank(lemma: &str) -> String {
    match frequency_manager::get_rank_for_lemma(lemma) {
        Some(rank) => format!("{}<{}>", lemma, rank),
        None => format!("{}<->", lemma),
    }
}

/// Heuristic: does this text look like a chapter/section heading?
fn is_likely_heading(text: &str) -> bool {
    let len = text.chars().count();
    // Must be reasonably short
    if len > 100 {
        return false;
    }
    let lower = text.to_lowercase();
    // Explicit chapter/part markers
    if lower.starts_with("chapter ")
        || lower.starts_with("part ")
        || lower.starts_with("book ")
        || lower.starts_with("volume ")
        || lower.starts_with("prologue")
        || lower.starts_with("epilogue")
        || lower.starts_with("preface")
        || lower.starts_with("introduction")
        || lower.starts_with("appendix")
    {
        return true;
    }
    // All-caps short line (e.g. "THE OLD WOMAN" as a section title)
    if len <= 60 && len >= 2 {
        let alpha_chars: Vec<char> = text.chars().filter(|c| c.is_alphabetic()).collect();
        if !alpha_chars.is_empty() && alpha_chars.iter().all(|c| c.is_uppercase()) {
            return true;
        }
    }
    // Roman numeral lines: I, II, III, IV, ... possibly with a period or title after
    if len <= 40 {
        let first_word = text.split_whitespace().next().unwrap_or("");
        let stripped = first_word.trim_end_matches('.');
        if is_roman_numeral(stripped) {
            return true;
        }
    }
    false
}

/// Check if a string is a valid Roman numeral (I through L or so).
fn is_roman_numeral(s: &str) -> bool {
    if s.is_empty() || s.len() > 10 {
        return false;
    }
    s.chars().all(|c| matches!(c, 'I' | 'V' | 'X' | 'L' | 'C' | 'D' | 'M'))
}

/// Format a slice of lemmas with ranks, joined by ", ".
fn format_lemmas_with_ranks(lemmas: &[String]) -> String {
    lemmas.iter().map(|l| format_lemma_with_rank(l)).collect::<Vec<_>>().join(", ")
}

/// Resolve short tier aliases to canonical tier IDs.
fn resolve_tier_alias(alias: &str) -> String {
    match alias {
        "adv" => "advanced_target".to_string(),
        "mod" => "moderate_target".to_string(),
        "bas_t" | "basic_t" => "basic_target".to_string(),
        "bas_b" | "basic_b" => "basic_base".to_string(),
        "source" => "base".to_string(),
        other => other.to_string(),
    }
}

/// Compact display alias for a canonical tier ID.
pub fn tier_display_alias(tier_id: &str) -> &str {
    match tier_id {
        "advanced_target" => "adv",
        "moderate_target" => "mod",
        "basic_target" => "bas_t",
        "basic_base" => "bas_b",
        "base" => "base",
        other => other,
    }
}

/// Extract a quoted name from command parts.
/// Handles both `"Name Here"` (quoted) and `Name` (single unquoted word) forms.
fn parse_quoted_name(parts: &[&str]) -> String {
    let joined = parts.join(" ");
    if joined.starts_with('"') {
        // Find closing quote
        if let Some(end) = joined[1..].find('"') {
            return joined[1..1 + end].to_string();
        }
    }
    // Fall back to first part as an unquoted name
    if let Some(first) = parts.first() {
        first.to_string()
    } else {
        String::new()
    }
}

/// Parse `new chapter "Name Here" 66 150`
fn parse_new_chapter_command(input: &str) -> Result<TerminalCommand, String> {
    // Skip "new chapter "
    let rest = input.trim();
    let after_new_chapter = if let Some(pos) = rest.find("chapter") {
        rest[pos + 7..].trim()
    } else {
        return Err("Usage: new chapter \"<name>\" <start> <end>".to_string());
    };

    // Extract quoted name and remaining text
    let (name, remainder) = if after_new_chapter.starts_with('"') {
        if let Some(end) = after_new_chapter[1..].find('"') {
            let name = after_new_chapter[1..1 + end].to_string();
            let rest = after_new_chapter[2 + end..].trim().to_string();
            (name, rest)
        } else {
            return Err("Unclosed quote in chapter name.".to_string());
        }
    } else {
        // Unquoted: take first token as name
        let parts: Vec<&str> = after_new_chapter.split_whitespace().collect();
        if parts.is_empty() {
            return Err("Usage: new chapter \"<name>\" <start> <end>".to_string());
        }
        let name = parts[0].to_string();
        let rest = parts[1..].join(" ");
        (name, rest)
    };

    if name.is_empty() {
        return Err("Chapter name cannot be empty.".to_string());
    }

    let nums: Vec<&str> = remainder.split_whitespace().collect();
    if nums.len() < 2 {
        return Err("Usage: new chapter \"<name>\" <start> <end>".to_string());
    }
    let start = nums[0].parse::<usize>().map_err(|_| "Invalid start number")?;
    let end = nums[1].parse::<usize>().map_err(|_| "Invalid end number")?;
    if start == 0 || end == 0 {
        return Err("Sentence numbers start at 1".to_string());
    }
    if start > end {
        return Err("Start must be <= end.".to_string());
    }

    Ok(TerminalCommand::App(AppCommand::NewChapter { name, start, end }))
}

/// Parse a raw terminal input line into a TerminalCommand.
pub fn parse_command(input: &str) -> Result<TerminalCommand, String> {
    let parts: Vec<&str> = input.split_whitespace().collect();
    if parts.is_empty() {
        return Err("Empty command".to_string());
    }

    match parts[0] {
        "exit" | "quit" => Ok(TerminalCommand::Exit),
        "help" => Ok(TerminalCommand::Help),
        "clear" => Ok(TerminalCommand::Clear),
        "list" => {
            if parts.len() > 1 && parts[1] == "nav" {
                // list nav --around <N>
                if parts.len() >= 4 && parts[2] == "--around" {
                    let n = parts[3].parse::<usize>().map_err(|_| "Invalid sentence number".to_string())?;
                    if n == 0 { return Err("Sentence numbers start at 1".to_string()); }
                    Ok(TerminalCommand::ListNavAround { center: n - 1 })
                } else {
                    let start_index = if parts.len() > 2 {
                        parts[2].parse::<usize>().ok().map(|n| n.saturating_sub(1))
                    } else {
                        None
                    };
                    Ok(TerminalCommand::ListNav { start_index })
                }
            } else if parts.len() > 2 && parts[1] == "pn_lemmas" {
                let n = parts[2].parse::<usize>().map_err(|_| "Invalid sentence number")?;
                if n == 0 { return Err("Sentence numbers start at 1".to_string()); }
                Ok(TerminalCommand::App(AppCommand::ListPnLemmas { index: n - 1 }))
            } else if parts.len() > 1 && parts[1] == "chapters" {
                Ok(TerminalCommand::App(AppCommand::ListChapters))
            } else if parts.len() > 1 && parts[1] == "headings" {
                Ok(TerminalCommand::ListHeadings)
            } else {
                Err("Unknown list command. Try 'list nav', 'list headings', 'list pn_lemmas <N>', or 'list chapters'".to_string())
            }
        },
        "load" => {
            if parts.len() > 2 && parts[1] == "project" {
                Ok(TerminalCommand::App(AppCommand::LoadProject { path: parts[2..].join(" ") }))
            } else {
                Err("Usage: load project <path>".to_string())
            }
        },
        "search" => {
            // search "<text>" or search <text...>
            // Extract the query: join remaining parts, strip surrounding quotes if present
            if parts.len() < 2 {
                return Err("Usage: search <text>".to_string());
            }
            let raw = parts[1..].join(" ");
            let query = raw.trim_matches('"').trim_matches('\'').to_string();
            if query.is_empty() {
                return Err("Usage: search <text>".to_string());
            }
            Ok(TerminalCommand::SearchText { query })
        },
        "add" => {
            if parts.len() > 2 && parts[1] == "sentence" {
                // add sentence <text...> — add with initial base text
                let text = parts[2..].join(" ");
                Ok(TerminalCommand::App(AppCommand::AddSentenceWithText { text }))
            } else if parts.len() == 2 && parts[1] == "sentence" {
                Ok(TerminalCommand::App(AppCommand::AddSentence))
            } else if parts.len() > 3 && parts[1] == "pn_lemma" {
                let n = parts[2].parse::<usize>().map_err(|_| "Invalid sentence number")?;
                if n == 0 { return Err("Sentence numbers start at 1".to_string()); }
                let lemma = parts[3..].join(" ");
                Ok(TerminalCommand::App(AppCommand::AddPnLemma { index: n - 1, lemma }))
            } else if parts.len() > 5 && parts[1] == "seg" {
                let n = parts[2].parse::<usize>().map_err(|_| "Invalid sentence number")?;
                if n == 0 { return Err("Sentence numbers start at 1".to_string()); }
                let tier_id = resolve_tier_alias(parts[3]);
                let after_seg_id = parts[4].to_string();
                let new_text = parts[5..].join(" ");
                Ok(TerminalCommand::App(AppCommand::AddSegment { index: n - 1, tier_id, after_seg_id, new_text }))
            } else {
                Err("Usage: add sentence | add pn_lemma <N> <lemma> | add seg <N> <tier> <after_seg_id> <text...>".to_string())
            }
        },
        "select" => {
            if parts.len() > 2 && parts[1] == "chapter" {
                let name = parse_quoted_name(&parts[2..]);
                if name.is_empty() { return Err("Usage: select chapter \"<name>\"".to_string()); }
                Ok(TerminalCommand::App(AppCommand::SelectChapter { name }))
            } else if parts.len() > 2 && parts[1] == "sentence" {
                let val = parts[2];
                if val.starts_with('S') {
                    Ok(TerminalCommand::App(AppCommand::SelectSentence { id: Some(val.to_string()), index: None }))
                } else {
                    match val.parse::<usize>() {
                        Ok(0) => Err("Sentence numbers start at 1".to_string()),
                        Ok(n) => Ok(TerminalCommand::App(AppCommand::SelectSentence { id: None, index: Some(n - 1) })),
                        Err(_) => Err("Invalid sentence number".to_string())
                    }
                }
            } else if parts.len() > 2 && parts[1] == "tier" {
                let tier_id = resolve_tier_alias(parts[2]);
                Ok(TerminalCommand::App(AppCommand::SelectTier { tier_id }))
            } else {
                Err("Usage: select sentence <id|number> | select tier <tier>".to_string())
            }
        },
        "import" => {
            if parts.len() > 2 && parts[1] == "source" {
                let path = parts[2..].join(" ");
                Ok(TerminalCommand::App(AppCommand::ImportSource { path }))
            } else if parts.len() > 2 && parts[1] == "json" {
                let path = parts[2..].join(" ");
                Ok(TerminalCommand::App(AppCommand::ImportJson { path }))
            } else if parts.len() > 2 && parts[1] == "level_map" {
                let path = parts[2..].join(" ");
                Ok(TerminalCommand::App(AppCommand::ImportLevelMap { path }))
            } else {
                Err("Usage: import source <path> | import json <path> | import level_map <path>".to_string())
            }
        },
        "save" => {
            if parts.len() > 1 && parts[1] == "project" {
                let path = if parts.len() > 2 { Some(parts[2..].join(" ")) } else { None };
                Ok(TerminalCommand::App(AppCommand::SaveProject { path }))
            } else {
                Err("Usage: save project [path]".to_string())
            }
        },
        "export" => {
            if parts.len() > 2 && parts[1] == "json" {
                let path = parts[2..].join(" ");
                Ok(TerminalCommand::App(AppCommand::ExportJson { path }))
            } else if parts.len() >= 2 && parts[1] == "level_map" {
                let path = if parts.len() > 2 { parts[2..].join(" ") } else { ".".to_string() };
                Ok(TerminalCommand::App(AppCommand::ExportLevelMap { path }))
            } else {
                Err("Usage: export json <path> | export level_map [path]".to_string())
            }
        },
        "update" => {
            if parts.len() > 4 && parts[1] == "text" {
                let val = parts[2];
                let tier_id = parts[3].to_string();
                let new_text = parts[4..].join(" ");
                if val.starts_with('S') {
                    Ok(TerminalCommand::App(AppCommand::UpdateText { sentence_id: Some(val.to_string()), index: None, tier_id, new_text }))
                } else {
                    match val.parse::<usize>() {
                        Ok(0) => Err("Sentence numbers start at 1".to_string()),
                        Ok(n) => Ok(TerminalCommand::App(AppCommand::UpdateText { sentence_id: None, index: Some(n - 1), tier_id, new_text })),
                        Err(_) => Err("Invalid sentence number".to_string())
                    }
                }
            } else {
                Err("Usage: update text <id|number> <tier_id> <new_text>  (numbers are 1-based)".to_string())
            }
        },
        "approve" => {
            if parts.len() == 1 {
                // Bare 'approve' — use current selection (sentinel usize::MAX resolved in engine)
                Ok(TerminalCommand::App(AppCommand::ApproveTier { index: usize::MAX, tier_id: String::new() }))
            } else if parts.len() > 3 && parts[1] == "tier" {
                let n = parts[2].parse::<usize>().map_err(|_| "Invalid sentence number")?;
                if n == 0 { return Err("Sentence numbers start at 1".to_string()); }
                let tier_id = resolve_tier_alias(parts[3]);
                Ok(TerminalCommand::App(AppCommand::ApproveTier { index: n - 1, tier_id }))
            } else if parts.len() > 3 && parts[1] == "edits" {
                let val = parts[2];
                let tier_id = parts[3].to_string();
                if val.starts_with('S') {
                    Ok(TerminalCommand::App(AppCommand::ApproveEdits { sentence_id: Some(val.to_string()), index: None, tier_id }))
                } else {
                    match val.parse::<usize>() {
                        Ok(0) => Err("Sentence numbers start at 1".to_string()),
                        Ok(n) => Ok(TerminalCommand::App(AppCommand::ApproveEdits { sentence_id: None, index: Some(n - 1), tier_id })),
                        Err(_) => Err("Invalid sentence number".to_string())
                    }
                }
            } else {
                Err("Usage: approve | approve tier <N> <tier>".to_string())
            }
        },
        "run" => {
             if parts.len() > 4 && parts[1] == "generate" {
                 // run generate <StageName|tier_alias> <start> <end>  (1-based inclusive)
                 // Accept both full stage names and short tier aliases.
                 let raw = parts[2];
                 let stage_name = match raw.to_lowercase().as_str() {
                     "adv" | "adv_t" | "advanced" | "advanced_target" => "GenerateAdvancedTarget",
                     "mod" | "mod_t" | "moderate" | "moderate_target" => "GenerateModerateTarget",
                     "bas_b" | "basic_base"                           => "GenerateBasicBase",
                     "bas_t" | "basic_target"                         => "GenerateBasicTarget",
                     "phrase_map" | "fwd_map"                         => "GeneratePhraseMap",
                     "inv_map" | "inverse_phrase_map"                 => "GenerateInversePhraseMap",
                     _ => raw,  // pass through as-is (full stage name)
                 }.to_string();
                 let start_num = parts[3].parse::<usize>().map_err(|_| "Invalid start number")?;
                 let end_num = parts[4].parse::<usize>().map_err(|_| "Invalid end number")?;
                 if start_num == 0 || end_num == 0 {
                     return Err("Sentence numbers start at 1".to_string());
                 }
                 let no_followup = parts.get(5).map(|s| *s == "--no-followup").unwrap_or(false);
                 Ok(TerminalCommand::App(AppCommand::GenerateStage { stage_name, start_index: start_num - 1, end_index: end_num - 1, no_followup }))
             } else {
                 Err("Usage: run generate <StageName> <start> <end>  (1-based)".to_string())
             }
        },
        "config" => {
            if parts.len() > 3 && parts[1] == "set" {
                let key = parts[2].to_string();
                let value = parts[3..].join(" ");
                Ok(TerminalCommand::App(AppCommand::ConfigSet { key, value }))
            } else if parts.len() > 1 && (parts[1] == "list" || parts[1] == "show") {
                Ok(TerminalCommand::App(AppCommand::ConfigList))
            } else if parts.len() > 2 && parts[1] == "add_model" {
                let alias = parts[2].to_string();
                Ok(TerminalCommand::App(AppCommand::ConfigAddModel { alias }))
            } else if parts.len() > 2 && parts[1] == "remove_model" {
                let alias = parts[2].to_string();
                Ok(TerminalCommand::App(AppCommand::ConfigRemoveModel { alias }))
            } else if parts.len() > 3 && parts[1] == "rename_model" {
                let old_alias = parts[2].to_string();
                let new_alias = parts[3].to_string();
                Ok(TerminalCommand::App(AppCommand::ConfigRenameModel { old_alias, new_alias }))
            } else {
                 Err("Usage: config set <key> <value> | config list | config add_model <alias> | config remove_model <alias> | config rename_model <old> <new>".to_string())
            }
        },
        "status" => {
             Ok(TerminalCommand::App(AppCommand::CheckStatus))
        },
        "set" => {
            if parts.len() > 2 && parts[1] == "right_view" {
                Ok(TerminalCommand::App(AppCommand::SetRightView { view: parts[2].to_string() }))
            } else if parts.len() > 2 && parts[1] == "left_view" {
                Ok(TerminalCommand::App(AppCommand::SetLeftView { view: parts[2].to_string() }))
            } else if parts.len() > 2 && parts[1] == "output_dir" {
                let path = parts[2..].join(" ");
                Ok(TerminalCommand::App(AppCommand::SetOutputDir { path }))
            } else if parts.len() > 3 && parts[1] == "key" {
                let provider = parts[2].to_string();
                let value = parts[3..].join(" ");
                Ok(TerminalCommand::App(AppCommand::SetKey { provider, value }))
            } else if parts.len() > 3 && parts[1] == "languages" {
                let source = parts[2].to_string();
                let target = parts[3].to_string();
                Ok(TerminalCommand::App(AppCommand::SetLanguages { source, target }))
            } else if parts.len() > 2 && parts[1] == "book_name" {
                let name = parts[2..].join(" ");
                Ok(TerminalCommand::App(AppCommand::SetBookName { name }))
            } else if parts.len() > 2 && parts[1] == "chapter_mode" {
                match parts[2] {
                    "true" | "on" | "1" => Ok(TerminalCommand::App(AppCommand::SetChapterMode { enabled: true })),
                    "false" | "off" | "0" => Ok(TerminalCommand::App(AppCommand::SetChapterMode { enabled: false })),
                    _ => Err("Usage: set chapter_mode true|false".to_string()),
                }
            } else if parts.len() > 2 && parts[1] == "frontier" {
                match parts[2] {
                    "true" | "on" | "1" => Ok(TerminalCommand::App(AppCommand::SetFrontierEnabled { enabled: true })),
                    "false" | "off" | "0" => Ok(TerminalCommand::App(AppCommand::SetFrontierEnabled { enabled: false })),
                    _ => Err("Usage: set frontier on|off".to_string()),
                }
            } else if parts.len() > 2 && parts[1] == "frontier_pct" {
                let pct = parts[2].parse::<f32>().map_err(|_| format!("Invalid pct '{}'. Use a number.", parts[2]))?;
                Ok(TerminalCommand::App(AppCommand::SetFrontierPct { pct }))
            } else if parts.len() > 2 && parts[1] == "frontier_seed" {
                let seed = parts[2].parse::<u64>().map_err(|_| format!("Invalid seed '{}'. Use a number.", parts[2]))?;
                Ok(TerminalCommand::App(AppCommand::SetFrontierSeed { seed }))
            } else {
                Err("Usage: set right_view <v> | set left_view <v> | set output_dir <p> | set key <anthropic|google> <value> | set languages <source> <target> | set book_name <name> | set chapter_mode true|false | set frontier on|off | set frontier_pct <n> | set frontier_seed <n>".to_string())
            }
        },
        "show" => {
            if parts.len() > 1 && parts[1] == "detail" {
                Ok(TerminalCommand::ShowDetail)
            } else if parts.len() > 1 && parts[1] == "mapping" {
                Ok(TerminalCommand::ShowMapping)
            } else if parts.len() > 1 && parts[1] == "tokens" {
                Ok(TerminalCommand::ShowTokens)
            } else if parts.len() > 1 && (parts[1] == "level_map" || parts[1] == "levelmap") {
                let level = if parts.len() > 2 {
                    Some(parts[2].parse::<u32>().map_err(|_| format!("Invalid level '{}'. Use a number.", parts[2]))?)
                } else {
                    None
                };
                Ok(TerminalCommand::App(AppCommand::ShowLevelMap { level }))
            } else {
                Err("Usage: show detail | show mapping | show tokens | show level_map [level]".to_string())
            }
        },
        "print" => {
            let tier = if parts.len() > 1 {
                Some(resolve_tier_alias(parts[1]))
            } else {
                None
            };
            Ok(TerminalCommand::Print { tier })
        },
        "measure_avd" => {
            if parts.len() > 1 {
                let path = parts[1..].join(" ");
                Ok(TerminalCommand::App(AppCommand::MeasureAvd { path }))
            } else {
                Err("Usage: measure_avd <filepath>".to_string())
            }
        },
        "measure_user_score" => {
            if parts.len() > 1 {
                let path = parts[1..].join(" ");
                Ok(TerminalCommand::App(AppCommand::MeasureUserScore { path }))
            } else {
                Err("Usage: measure_user_score <filepath>".to_string())
            }
        },
        "debug_dump" => {
            if parts.len() > 2 {
                let start = parts[1].parse::<usize>().map_err(|_| "Invalid start number")?;
                let end = parts[2].parse::<usize>().map_err(|_| "Invalid end number")?;
                if start == 0 || end == 0 {
                    return Err("Sentence numbers start at 1".to_string());
                }
                let path = if parts.len() > 3 { Some(parts[3..].join(" ")) } else { None };
                Ok(TerminalCommand::App(AppCommand::DebugDump { start_index: start - 1, end_index: end - 1, path }))
            } else {
                Err("Usage: debug_dump <start> <end> [filepath]  (1-based)".to_string())
            }
        },
        "watch_job" => Ok(TerminalCommand::WatchJob),
        "job_status" => Ok(TerminalCommand::JobStatus),
        "calibrate" => {
            if parts.len() > 1 && parts[1] == "info" {
                Ok(TerminalCommand::CalibrationInfo)
            } else {
                let max_level = if parts.len() > 1 {
                    Some(parts[1].parse::<u32>().map_err(|_| "Invalid max level number")?)
                } else {
                    None
                };
                Ok(TerminalCommand::App(AppCommand::Calibrate { max_level }))
            }
        },
        "generate_weave" => {
            if parts.len() <= 1 {
                return Err("Usage: generate_weave <level|all|b|m|a|i|r> [--force] [--frontier|--no-frontier] [--frontier-pct N] [--frontier-seed N] [--frontier-test|--no-frontier-test] [--frontier-familiar-n N]".to_string());
            }

            let mut level: Option<String> = None;
            let mut force = false;
            let mut frontier_enabled_override: Option<bool> = None;
            let mut frontier_target_pct_override: Option<f32> = None;
            let mut frontier_seed_override: Option<u64> = None;
            let mut frontier_test_mode_override: Option<bool> = None;
            let mut frontier_familiar_lemma_exclude_count_override: Option<usize> = None;

            let mut i = 1;
            while i < parts.len() {
                match parts[i] {
                    "--force" => {
                        force = true;
                        i += 1;
                    }
                    "--frontier" => {
                        frontier_enabled_override = Some(true);
                        i += 1;
                    }
                    "--no-frontier" => {
                        frontier_enabled_override = Some(false);
                        i += 1;
                    }
                    "--frontier-test" => {
                        frontier_test_mode_override = Some(true);
                        i += 1;
                    }
                    "--no-frontier-test" => {
                        frontier_test_mode_override = Some(false);
                        i += 1;
                    }
                    "--frontier-pct" => {
                        if i + 1 >= parts.len() {
                            return Err("Missing value for --frontier-pct".to_string());
                        }
                        frontier_target_pct_override = Some(
                            parts[i + 1]
                                .parse::<f32>()
                                .map_err(|_| "Invalid number for --frontier-pct")?,
                        );
                        i += 2;
                    }
                    "--frontier-seed" => {
                        if i + 1 >= parts.len() {
                            return Err("Missing value for --frontier-seed".to_string());
                        }
                        frontier_seed_override = Some(
                            parts[i + 1]
                                .parse::<u64>()
                                .map_err(|_| "Invalid integer for --frontier-seed")?,
                        );
                        i += 2;
                    }
                    "--frontier-familiar-n" => {
                        if i + 1 >= parts.len() {
                            return Err("Missing value for --frontier-familiar-n".to_string());
                        }
                        frontier_familiar_lemma_exclude_count_override = Some(
                            parts[i + 1]
                                .parse::<usize>()
                                .map_err(|_| "Invalid integer for --frontier-familiar-n")?,
                        );
                        i += 2;
                    }
                    arg if arg.starts_with("--") => {
                        return Err(format!("Unknown generate_weave flag '{}'.", arg));
                    }
                    arg => {
                        if level.is_none() {
                            level = Some(arg.to_string());
                            i += 1;
                        } else {
                            return Err(format!("Unexpected argument '{}'.", arg));
                        }
                    }
                }
            }

            let level = level.ok_or_else(|| {
                "Missing level argument. Usage: generate_weave <level|all|b|m|a|i|r> [flags]"
                    .to_string()
            })?;

            Ok(TerminalCommand::App(AppCommand::GenerateWeave {
                level,
                force,
                frontier_enabled_override,
                frontier_target_pct_override,
                frontier_seed_override,
                frontier_test_mode_override,
                frontier_familiar_lemma_exclude_count_override,
            }))
        },
        "drc" => {
            if parts.len() > 1 {
                let tier_id = resolve_tier_alias(parts[1]);
                // Optional third arg: a number or 'all'
                let limit: Option<usize> = if parts.len() > 2 {
                    if parts[2] == "all" {
                        None
                    } else {
                        Some(parts[2].parse::<usize>().map_err(|_| "Usage: drc <tier> [N|all]".to_string())?)
                    }
                } else {
                    Some(10) // default show first 10
                };
                Ok(TerminalCommand::App(AppCommand::DrcTier { tier_id, limit }))
            } else {
                Ok(TerminalCommand::App(AppCommand::Drc))
            }
        },
        "audit" => {
            Ok(TerminalCommand::App(AppCommand::Audit))
        },
        "open" => {
            if parts.len() > 2 && parts[1] == "workspace" {
                let path = parts[2..].join(" ");
                Ok(TerminalCommand::App(AppCommand::OpenWorkspace { path }))
            } else {
                Err("Usage: open workspace <path>".to_string())
            }
        },
        "rm" => {
            if parts.len() > 3 && parts[1] == "pn_lemma" {
                let n = parts[2].parse::<usize>().map_err(|_| "Invalid sentence number")?;
                if n == 0 { return Err("Sentence numbers start at 1".to_string()); }
                let lemma = parts[3..].join(" ");
                Ok(TerminalCommand::App(AppCommand::RemovePnLemma { index: n - 1, lemma }))
            } else if parts.len() > 3 && parts[1] == "seg" {
                let n = parts[2].parse::<usize>().map_err(|_| "Invalid sentence number")?;
                if n == 0 { return Err("Sentence numbers start at 1".to_string()); }
                let tier_id = resolve_tier_alias(parts[3]);
                let seg_id = if parts.len() > 4 { parts[4].to_string() } else { return Err("Usage: rm seg <N> <tier> <seg_id>".to_string()); };
                Ok(TerminalCommand::App(AppCommand::RemoveSegment { index: n - 1, tier_id, seg_id }))
            } else {
                Err("Usage: rm pn_lemma <N> <lemma> | rm seg <N> <tier> <seg_id>".to_string())
            }
        },
        "delete" => {
            if parts.len() > 2 && parts[1] == "chapter" {
                let name = parse_quoted_name(&parts[2..]);
                if name.is_empty() { return Err("Usage: delete chapter \"<name>\"".to_string()); }
                Ok(TerminalCommand::App(AppCommand::DeleteChapter { name }))
            } else if parts.len() > 2 && parts[1] == "key" {
                Ok(TerminalCommand::App(AppCommand::DeleteKey { provider: parts[2].to_string() }))
            } else if parts.len() > 1 {
                // delete <N>  — delete word token at 1-based word index on selected sent/tier
                let n = parts[1].parse::<usize>().map_err(|_| "Invalid word index")?;
                if n == 0 { return Err("Word indices start at 1".to_string()); }
                Ok(TerminalCommand::App(AppCommand::DeleteToken { word_index: n }))
            } else {
                Err("Usage: delete key <provider> | delete <word_index>".to_string())
            }
        },
        "key" => {
            if parts.len() > 1 && parts[1] == "status" {
                Ok(TerminalCommand::App(AppCommand::KeyStatus))
            } else {
                Err("Usage: key status".to_string())
            }
        },
        "weave" => {
            if parts.len() > 1 && parts[1] == "status" {
                Ok(TerminalCommand::App(AppCommand::WeaveStatus))
            } else {
                Err("Usage: weave status".to_string())
            }
        },
        "report" => {
            if parts.len() > 1 && parts[1] == "sentences" {
                if parts.len() > 2 && parts[2] == "incomplete" {
                    let limit = if parts.len() > 3 { parts[3].parse::<usize>().ok() } else { None };
                    Ok(TerminalCommand::App(AppCommand::ReportSentencesIncomplete { limit }))
                } else if parts.len() > 2 && parts[2] == "complete" {
                    let limit = if parts.len() > 3 { parts[3].parse::<usize>().ok() } else { None };
                    Ok(TerminalCommand::App(AppCommand::ReportSentencesComplete { limit }))
                } else {
                    Err("Usage: report sentences incomplete [N] | report sentences complete [N]".to_string())
                }
            } else if parts.len() > 2 && parts[1] == "sentence" {
                // report sentence <N> or report sentence <N>-<M> (1-based)
                let range_str = parts[2];
                if range_str.contains('-') {
                    let range_parts: Vec<&str> = range_str.splitn(2, '-').collect();
                    let start = range_parts[0].parse::<usize>().map_err(|_| "Invalid start number")?;
                    let end = range_parts[1].parse::<usize>().map_err(|_| "Invalid end number")?;
                    if start == 0 || end == 0 {
                        return Err("Sentence numbers start at 1".to_string());
                    }
                    Ok(TerminalCommand::App(AppCommand::ReportSentence { start_index: start - 1, end_index: end - 1 }))
                } else {
                    let n = range_str.parse::<usize>().map_err(|_| "Invalid sentence number")?;
                    if n == 0 {
                        return Err("Sentence numbers start at 1".to_string());
                    }
                    Ok(TerminalCommand::App(AppCommand::ReportSentence { start_index: n - 1, end_index: n - 1 }))
                }
            } else {
                Err("Usage: report sentences incomplete | report sentences complete | report sentence <N> | report sentence <N>-<M>".to_string())
            }
        },
        "edit" => {
            // edit seg <N> <tier> <seg_id> <text...>
            if parts.len() > 5 && parts[1] == "seg" {
                let n = parts[2].parse::<usize>().map_err(|_| "Invalid sentence number")?;
                if n == 0 { return Err("Sentence numbers start at 1".to_string()); }
                let tier_id = resolve_tier_alias(parts[3]);
                let seg_id = parts[4].to_string();
                let new_text = parts[5..].join(" ");
                Ok(TerminalCommand::App(AppCommand::EditSegment { index: n - 1, tier_id, seg_id, new_text }))
            } else if parts.len() > 1 && parts[1] != "seg" {
                // edit <text...>  — edit selected sentence/tier text
                let new_text = parts[1..].join(" ");
                Ok(TerminalCommand::App(AppCommand::EditText { new_text }))
            } else {
                Err("Usage: edit <text...> | edit seg <N> <tier> <seg_id> <text...>".to_string())
            }
        },
        "lemmatize" => {
            // lemmatize <N> <tier>
            if parts.len() > 2 {
                let n = parts[1].parse::<usize>().map_err(|_| "Invalid sentence number")?;
                if n == 0 { return Err("Sentence numbers start at 1".to_string()); }
                let tier_id = resolve_tier_alias(parts[2]);
                Ok(TerminalCommand::App(AppCommand::LemmatizeTier { index: n - 1, tier_id }))
            } else {
                Err("Usage: lemmatize <N> <tier>".to_string())
            }
        },
        "validate" => {
            // validate <N> <tier>
            if parts.len() > 2 {
                let n = parts[1].parse::<usize>().map_err(|_| "Invalid sentence number")?;
                if n == 0 { return Err("Sentence numbers start at 1".to_string()); }
                let tier_id = resolve_tier_alias(parts[2]);
                Ok(TerminalCommand::App(AppCommand::ValidateTier { index: n - 1, tier_id }))
            } else {
                Err("Usage: validate <N> <tier>".to_string())
            }
        },
        "split" => {
            // split <word_index> — split word token at 1-based index
            if parts.len() > 1 {
                let n = parts[1].parse::<usize>().map_err(|_| "Invalid word index")?;
                if n == 0 { return Err("Word indices start at 1".to_string()); }
                Ok(TerminalCommand::App(AppCommand::SplitToken { word_index: n }))
            } else {
                Err("Usage: split <word_index>".to_string())
            }
        },
        "merge" => {
            // merge <start>-<end>  or  merge <start> <end>  (1-based word indices)
            if parts.len() == 3 {
                // "merge 1 2" form
                let start = parts[1].parse::<usize>().map_err(|_| "Invalid start index")?;
                let end = parts[2].parse::<usize>().map_err(|_| "Invalid end index")?;
                if start == 0 || end == 0 { return Err("Word indices start at 1".to_string()); }
                if start > end { return Err("Start must be <= end".to_string()); }
                Ok(TerminalCommand::App(AppCommand::MergeTokens { word_start: start, word_end: end }))
            } else if parts.len() == 2 {
                // "merge 1-2" form
                let range_str = parts[1];
                if let Some((s, e)) = range_str.split_once('-') {
                    let start = s.parse::<usize>().map_err(|_| "Invalid start index")?;
                    let end = e.parse::<usize>().map_err(|_| "Invalid end index")?;
                    if start == 0 || end == 0 { return Err("Word indices start at 1".to_string()); }
                    if start > end { return Err("Start must be <= end".to_string()); }
                    Ok(TerminalCommand::App(AppCommand::MergeTokens { word_start: start, word_end: end }))
                } else {
                    Err("Usage: merge <start>-<end> | merge <start> <end>".to_string())
                }
            } else {
                Err("Usage: merge <start>-<end> | merge <start> <end>".to_string())
            }
        },
        "insert" => {
            // insert <word_index>
            if parts.len() > 1 {
                let n = parts[1].parse::<usize>().map_err(|_| "Invalid word index")?;
                if n == 0 { return Err("Word indices start at 1".to_string()); }
                Ok(TerminalCommand::App(AppCommand::InsertToken { word_index: n }))
            } else {
                Err("Usage: insert <word_index>".to_string())
            }
        },
        "edit_b" => {
            // edit_b <word_index> <text>
            // The text after word_index can contain spaces and quotes
            if parts.len() > 2 {
                let n = parts[1].parse::<usize>().map_err(|_| "Invalid word index")?;
                if n == 0 { return Err("Word indices start at 1".to_string()); }
                // Join the rest and strip surrounding quotes if present
                let raw = parts[2..].join(" ");
                let new_text = raw.trim_matches('"').to_string();
                Ok(TerminalCommand::App(AppCommand::EditBackground { word_index: n, new_text }))
            } else {
                Err("Usage: edit_b <word_index> \"<text>\"".to_string())
            }
        },
        "edit_word" => {
            // edit_word <word_index> <text...>
            if parts.len() > 2 {
                let n = parts[1].parse::<usize>().map_err(|_| "Invalid word index")?;
                if n == 0 { return Err("Word indices start at 1".to_string()); }
                let new_text = parts[2..].join(" ");
                Ok(TerminalCommand::App(AppCommand::EditWord { word_index: n, new_text }))
            } else {
                Err("Usage: edit_word <word_index> <text>".to_string())
            }
        },
        "edit_target" => {
            // edit_target <word_index> <text...>
            if parts.len() > 2 {
                let n = parts[1].parse::<usize>().map_err(|_| "Invalid word index")?;
                if n == 0 { return Err("Word indices start at 1".to_string()); }
                let new_text = parts[2..].join(" ");
                Ok(TerminalCommand::App(AppCommand::EditTarget { word_index: n, new_text }))
            } else {
                Err("Usage: edit_target <word_index> <text...>".to_string())
            }
        },
        "edit_targets" => {
            // edit_targets 1:La 2:vieja 3:puerta ...
            if parts.len() > 1 {
                let mut pairs = Vec::new();
                for token in &parts[1..] {
                    let (idx_str, text) = token.split_once(':')
                        .ok_or_else(|| format!("Invalid pair '{}'. Expected N:text", token))?;
                    let n = idx_str.parse::<usize>().map_err(|_| format!("Invalid index in '{}'", token))?;
                    if n == 0 { return Err("Word indices start at 1".to_string()); }
                    pairs.push((n, text.to_string()));
                }
                Ok(TerminalCommand::App(AppCommand::EditTargets { pairs }))
            } else {
                Err("Usage: edit_targets 1:La 2:vieja 3:puerta ...".to_string())
            }
        },
        "accept" => {
            // accept map — validate/accept the mapping for selected tier
            // accept map <start> <end> — bulk accept for stale sentences in range
            if parts.len() > 1 && parts[1] == "map" {
                if parts.len() >= 4 {
                    let start = parts[2].parse::<usize>().map_err(|_| "Invalid start number".to_string())?;
                    let end = parts[3].parse::<usize>().map_err(|_| "Invalid end number".to_string())?;
                    Ok(TerminalCommand::App(AppCommand::AcceptMapRange { start, end }))
                } else {
                    Ok(TerminalCommand::App(AppCommand::AcceptMap))
                }
            } else {
                Err("Usage: accept map [<start> <end>]".to_string())
            }
        },
        "init" => {
            if parts.len() > 1 && parts[1] == "mapping" {
                Ok(TerminalCommand::App(AppCommand::InitMapping))
            } else if parts.len() > 1 && parts[1] == "media" {
                Ok(TerminalCommand::App(AppCommand::InitMediaWorkspace))
            } else {
                Err("Usage: init mapping | init media".to_string())
            }
        },
        "server" => {
            if parts.len() > 1 && parts[1] == "info" {
                Ok(TerminalCommand::ServerInfo)
            } else {
                Err("Usage: server info".to_string())
            }
        },
        "copilot" => {
            if parts.len() >= 3 && parts[1] == "journal" {
                let text = parts[2..].join(" ");
                Ok(TerminalCommand::App(AppCommand::CopilotJournal { text }))
            } else if parts.len() >= 2 && parts[1] == "reset" {
                Ok(TerminalCommand::App(AppCommand::CopilotReset))
            } else {
                Err("Usage: copilot journal <text> | copilot reset".to_string())
            }
        },
        "new" => {
            if parts.len() > 2 && parts[1] == "project" {
                let name = parts[2..].join(" ");
                Ok(TerminalCommand::App(AppCommand::NewProject { name }))
            } else if parts.len() > 1 && parts[1] == "chapter" {
                // new chapter "Name Here" 66 150
                parse_new_chapter_command(input)
            } else {
                Err("Usage: new project <name> | new chapter \"<name>\" <start> <end>".to_string())
            }
        },
        "close" => {
            if parts.len() > 1 && parts[1] == "project" {
                Ok(TerminalCommand::App(AppCommand::CloseProject))
            } else {
                Err("Usage: close project".to_string())
            }
        },
        "remove" => {
            if parts.len() > 2 && parts[1] == "sentence" {
                let n = parts[2].parse::<usize>().map_err(|_| "Invalid sentence number")?;
                if n == 0 { return Err("Sentence numbers start at 1".to_string()); }
                Ok(TerminalCommand::App(AppCommand::RemoveSentence { index: n - 1 }))
            } else {
                Err("Usage: remove sentence <N>".to_string())
            }
        },
        "av" => parse_av_command(&parts[1..]),
        _ => Err(format!("Unknown command: {}", parts[0])),
    }
}

/// Parse `av` subcommands.
fn parse_av_command(parts: &[&str]) -> Result<TerminalCommand, String> {
    if parts.is_empty() {
        return Err("Usage: av <subcommand>. Try 'av help' for details.".to_string());
    }
    match parts[0] {
        "init" => Ok(TerminalCommand::App(AppCommand::AvInit)),
        "status" | "scan" => Ok(TerminalCommand::App(AppCommand::AvStatus)),
        "mark" => {
            if parts.len() < 2 {
                return Err("Usage: av mark <stem> [stem2 ...]".to_string());
            }
            let stems = parts[1..].iter().map(|s| s.to_string()).collect();
            Ok(TerminalCommand::App(AppCommand::AvMark { stems }))
        }
        "unmark" => {
            if parts.len() < 2 {
                return Err("Usage: av unmark <stem> [stem2 ...]".to_string());
            }
            let stems = parts[1..].iter().map(|s| s.to_string()).collect();
            Ok(TerminalCommand::App(AppCommand::AvUnmark { stems }))
        }
        "mark-all" => Ok(TerminalCommand::App(AppCommand::AvMarkAll)),
        "clear-marks" => Ok(TerminalCommand::App(AppCommand::AvClearMarks)),
        "generate" => {
            if parts.len() < 2 {
                return Err("Usage: av generate audio|video|characters|prompts|illustrations [stem|next|all]".to_string());
            }
            let target = if parts.len() >= 3 {
                match parts[2] {
                    "next" => AvTarget::Next,
                    "all" => AvTarget::All,
                    stem => AvTarget::Stem(stem.to_string()),
                }
            } else {
                AvTarget::Next
            };
            match parts[1] {
                "audio" => Ok(TerminalCommand::App(AppCommand::AvGenerateAudio { target })),
                "video" => Ok(TerminalCommand::App(AppCommand::AvGenerateVideo { target })),
                "characters" => Ok(TerminalCommand::App(AppCommand::AvGenerateCharacters)),
                "prompts" => Ok(TerminalCommand::App(AppCommand::AvGeneratePrompts)),
                "illustrations" => Ok(TerminalCommand::App(AppCommand::AvGenerateIllustrations)),
                _ => Err("Usage: av generate audio|video|characters|prompts|illustrations [stem|next|all]".to_string()),
            }
        }
        "config" => {
            if parts.len() < 2 {
                return Err("Usage: av config show | av config tts|video|voices <key> <value>".to_string());
            }
            match parts[1] {
                "show" => Ok(TerminalCommand::App(AppCommand::AvConfigShow)),
                "tts" => {
                    if parts.len() < 4 {
                        return Err("Usage: av config tts <key> <value>".to_string());
                    }
                    let key = parts[2].to_string();
                    let value = parts[3..].join(" ");
                    Ok(TerminalCommand::App(AppCommand::AvConfigTts { key, value }))
                }
                "video" => {
                    if parts.len() < 4 {
                        return Err("Usage: av config video <key> <value>".to_string());
                    }
                    let key = parts[2].to_string();
                    let value = parts[3..].join(" ");
                    Ok(TerminalCommand::App(AppCommand::AvConfigVideo { key, value }))
                }
                "voices" => {
                    if parts.len() < 3 {
                        return Err("Usage: av config voices <v1> [v2 ...]".to_string());
                    }
                    let voices = parts[2..].iter().map(|s| s.to_string()).collect();
                    Ok(TerminalCommand::App(AppCommand::AvConfigVoices { voices }))
                }
                "illustrations" => {
                    if parts.len() < 4 {
                        return Err("Usage: av config illustrations <key> <value>".to_string());
                    }
                    let key = parts[2].to_string();
                    let value = parts[3..].join(" ");
                    Ok(TerminalCommand::App(AppCommand::AvConfigIllustrations { key, value }))
                }
                _ => Err("Usage: av config show | av config tts|video|illustrations|voices <key> <value>".to_string()),
            }
        }
        "open" => {
            if parts.len() < 2 {
                return Err("Usage: av open book-dir|audio-dir|video-dir".to_string());
            }
            let which = parts[1].to_string();
            Ok(TerminalCommand::App(AppCommand::AvOpenDir { which }))
        }
        "cancel" | "stop" => Ok(TerminalCommand::App(AppCommand::AvCancel)),
        "log" | "output" => {
            // av log [N]  — show last N lines of AV job output (default: all)
            let tail = parts.get(1).and_then(|s| s.parse::<usize>().ok());
            Ok(TerminalCommand::App(AppCommand::AvLog { tail }))
        }
        "reject" => {
            // av reject chunk <stem> <index>
            if parts.len() < 4 || parts[1] != "chunk" {
                return Err("Usage: av reject chunk <stem> <index>".to_string());
            }
            let stem = parts[2].to_string();
            let index: u32 = parts[3].parse().map_err(|_| "Chunk index must be a number.".to_string())?;
            Ok(TerminalCommand::App(AppCommand::AvRejectChunk { stem, index }))
        }
        "restore" => {
            // av restore chunk <stem> <index>
            if parts.len() < 4 || parts[1] != "chunk" {
                return Err("Usage: av restore chunk <stem> <index>".to_string());
            }
            let stem = parts[2].to_string();
            let index: u32 = parts[3].parse().map_err(|_| "Chunk index must be a number.".to_string())?;
            Ok(TerminalCommand::App(AppCommand::AvRestoreChunk { stem, index }))
        }
        "chunks" => {
            // av chunks <stem>
            if parts.len() < 2 {
                return Err("Usage: av chunks <stem>".to_string());
            }
            let stem = parts[1].to_string();
            Ok(TerminalCommand::App(AppCommand::AvChunkStatus { stem }))
        }
        "rebuild" => {
            // av rebuild audio <stem>
            if parts.len() < 3 || parts[1] != "audio" {
                return Err("Usage: av rebuild audio <stem>".to_string());
            }
            let stem = parts[2].to_string();
            Ok(TerminalCommand::App(AppCommand::AvRebuildAudio { stem }))
        }
        "youtube" | "yt" => {
            if parts.len() < 2 {
                return Err("Usage: av youtube init|auth|config|upload".to_string());
            }
            match parts[1] {
                "init" => Ok(TerminalCommand::App(AppCommand::AvYoutubeInit)),
                "auth" => Ok(TerminalCommand::App(AppCommand::AvYoutubeAuth)),
                "config" => {
                    if parts.len() < 3 {
                        return Err("Usage: av youtube config show | av youtube config <key> <value>".to_string());
                    }
                    if parts[2] == "show" {
                        Ok(TerminalCommand::App(AppCommand::AvYoutubeConfigShow))
                    } else {
                        if parts.len() < 4 {
                            return Err("Usage: av youtube config <key> <value>".to_string());
                        }
                        let key = parts[2].to_string();
                        let value = parts[3..].join(" ");
                        // Strip surrounding quotes if present (terminal doesn't shell-parse)
                        let value = if (value.starts_with('"') && value.ends_with('"'))
                            || (value.starts_with('\'') && value.ends_with('\''))
                        {
                            value[1..value.len()-1].to_string()
                        } else {
                            value
                        };
                        Ok(TerminalCommand::App(AppCommand::AvYoutubeConfig { key, value }))
                    }
                }
                "upload" => {
                    let target = if parts.len() >= 3 {
                        match parts[2] {
                            "next" => AvTarget::Next,
                            "all" => AvTarget::All,
                            stem => AvTarget::Stem(stem.to_string()),
                        }
                    } else {
                        AvTarget::Next
                    };
                    Ok(TerminalCommand::App(AppCommand::AvYoutubeUpload { target }))
                }
                _ => Err("Usage: av youtube init|auth|config|upload".to_string()),
            }
        }
        "help" => {
            Ok(TerminalCommand::AvHelp)
        }
        _ => Err(format!("Unknown av subcommand: '{}'. Try 'av help'.", parts[0])),
    }
}

/// Execute a TerminalCommand against the Engine and return the output as a String.
/// Returns None for Exit (caller should handle shutdown).
pub fn execute_command(engine: &mut Engine, cmd: TerminalCommand) -> Option<String> {
    let mut out = String::new();

    match cmd {
        TerminalCommand::Exit => return None,
        TerminalCommand::Help => {
            out.push_str("Available commands:\n");
            out.push_str("  new project <name>     - Create a new empty project\n");
            out.push_str("  close project          - Close the current project\n");
            out.push_str("  set languages <s> <t>  - Set source and target languages (e.g. en es)\n");
            out.push_str("  set book_name <name>   - Set or rename the book name\n");
            out.push_str("  list nav [N]           - List navigator sentences (N is 1-based)\n");
            out.push_str("  list nav --around <N>  - Show context around sentence N\n");
            out.push_str("  list headings          - Scan for chapter/section headings\n");
            out.push_str("  search <text>          - Find sentences containing text (case-insensitive)\n");
            out.push_str("  select sentence <N>    - Select sentence by number (1-based) or ID\n");
            out.push_str("  select tier <tier>     - Select active tier (source, bas_b, bas_t, etc.)\n");
            out.push_str("  add sentence [text]    - Add empty sentence, or with initial base text\n");
            out.push_str("  remove sentence <N>    - Remove sentence N (1-based)\n");
            out.push_str("  edit <text>            - Replace selected sentence/tier text\n");
            out.push_str("  import source <path>   - Import source text file\n");
            out.push_str("  open workspace <path>  - Open (or create) a workspace directory\n");
            out.push_str("  load project <path>    - Load a .wvl file\n");
            out.push_str("  save project [path]    - Save project\n");
            out.push_str("  config set <k> <v>     - Set config value\n");
            out.push_str("  config list            - List config\n");
            out.push_str("  export level_map [p]   - Export level map (.lm file)\n");
            out.push_str("  run generate <stage> <s> <e> - Run generation stage (s,e are 1-based)\n");
            out.push_str("        stage: adv|mod|bas_b|bas_t|phrase_map|inv_map or full name\n");
            out.push_str("  approve tier <N> <tier> - Approve tier as Valid (lemmatizes if bridge available)\n");
            out.push_str("  show detail            - Show selected sentence details\n");
            out.push_str("  show mapping           - Show mappings for selected sentence/tier (human-readable)\n");
            out.push_str("  show tokens            - Show tokenized words with indices for selected sentence/tier\n");
            out.push_str("  print [tier]           - Print current tier view (or specified tier without switching)\n");
            out.push_str("  weave status           - Check if document is ready for weave\n");
            out.push_str("  report sentences incomplete - List sentences not ready for weave\n");
            out.push_str("  report sentences complete   - List weave-ready sentences\n");
            out.push_str("  report sentence <N>    - Detailed status for sentence N (1-based)\n");
            out.push_str("  report sentence <N>-<M> - Detailed status for range N to M\n");
            out.push_str("  measure_avd <path>     - Measure AVD score of a plain text file\n");
            out.push_str("  measure_user_score <p> - Measure estimated user level of a plain text file\n");
            out.push_str("  debug_dump <s> <e> [p] - Dump debug state for sentences s..e to file or stdout\n");
            out.push_str("  set key <p> <value>    - Store API key in OS keychain (anthropic|google)\n");
            out.push_str("  delete key <p>         - Remove key from OS keychain\n");
            out.push_str("  key status             - Show which API keys are configured\n");
            out.push_str("  watch_job              - Block until current LLM job completes\n");
            out.push_str("  job_status             - Non-blocking: show LLM job progress (for copilot polling)\n");
            out.push_str("  import level_map <p>   - Import a .lm level map file\n");
            out.push_str("  set output_dir <p>     - Set output directory for weave files\n");
            out.push_str("  generate_weave <N|all|b|m|a|i|r> - Generate weave text file(s); r outputs raw source as ULr\n");
            out.push_str("  generate_weave <N|all> --force - Generate weave, skip DRC\n");
            out.push_str("  generate_weave ... [--frontier|--no-frontier] [--frontier-pct N] [--frontier-seed N]\n");
            out.push_str("                    [--frontier-test|--no-frontier-test] [--frontier-familiar-n N]\n");
            out.push_str("  drc                    - Run Design Rule Check on all sentences\n");
            out.push_str("  drc <tier> [N|all]     - DRC for one tier (default: first 10 violations)\n");
            out.push_str("  calibrate [max_level]  - Run calibration on loaded document (default max: 45)\n");
            out.push_str("  calibrate info         - Show calibration status (sentence count, stability)\n");
            out.push_str("  list pn_lemmas <N>     - List proper noun lemmas for sentence N (1-based)\n");
            out.push_str("  add pn_lemma <N> <L>   - Add lemma L to sentence N's PN list\n");
            out.push_str("  rm pn_lemma <N> <L>    - Remove lemma L from sentence N's PN list\n");
            out.push_str("\n--- Segment Editing (Adv/Mod) ---\n");
            out.push_str("  edit seg <N> <tier> <seg_id> <text>  - Edit segment text\n");
            out.push_str("  add seg <N> <tier> <after> <text>    - Add segment after <after>\n");
            out.push_str("  rm seg <N> <tier> <seg_id>           - Remove segment\n");
            out.push_str("  lemmatize <N> <tier>   - Re-lemmatize tier segments via SpaCy\n");
            out.push_str("  validate <N> <tier>    - Lemmatize + mark tier Valid\n");
            out.push_str("  audit                  - Demote Valid tiers that violate DRC rules\n");
            out.push_str("\n--- Token/Mapping Editing (Bas B / Bas T) ---\n");
            out.push_str("  split <N>              - Split word at index N into sub-tokens\n");
            out.push_str("  merge <N>-<M>          - Merge words N through M into one\n");
            out.push_str("  insert <N>             - Insert empty word at position N\n");
            out.push_str("  delete <N>             - Delete word at position N\n");
            out.push_str("  edit_b <N> \"<text>\"    - Edit background token before word N\n");
            out.push_str("  edit_target <N> <text>  - Edit mapping target for word N\n");
            out.push_str("  edit_targets N:t N:t .. - Batch edit mapping targets (e.g. 1:La 2:vieja)\n");
            out.push_str("  init mapping           - Init empty mapping on selected sentence/tier\n");
            out.push_str("  accept map             - Accept mapping, mark tier Valid\n");
            out.push_str("  accept map <s> <e>     - Bulk accept stale mappings in range\n");
            out.push_str("\n--- Chapter Mode ---\n");
            out.push_str("  new chapter \"<name>\" <start> <end> - Define a chapter (1-based sentence range)\n");
            out.push_str("  list chapters          - List all chapters with ranges and validity\n");
            out.push_str("  delete chapter \"<name>\" - Remove a chapter definition\n");
            out.push_str("  select chapter \"<name>\" - Select a chapter as current\n");
            out.push_str("  set chapter_mode true|false - Toggle chapter mode\n");
            out.push_str("  init media             - Create media workspace directory structure\n");
            out.push_str("\n--- Co-Pilot ---\n");
            out.push_str("  $ <message>            - Send message to copilot agent (LLM)\n");
            out.push_str("  copilot journal <text>  - Append timestamped entry to copilot journal\n");
            out.push_str("  copilot reset          - Clear copilot session history (start fresh)\n");
            out.push_str("  server info            - Show copilot server name and port\n");
            out.push_str("  config set copilot.model <alias> - Set copilot LLM model\n");
            out.push_str("  config set copilot.max_turns <N> - Set max turns per session\n");
            out.push_str("\n--- AV Production ---\n");
            out.push_str("  av status              - Show audio/video file status\n");
            out.push_str("  av mark/unmark <stem>  - Mark/unmark files for AV\n");
            out.push_str("  av generate audio|video|characters|prompts|illustrations - Generate media\n");
            out.push_str("  av cancel              - Cancel running AV generation\n");
            out.push_str("  av log [N]             - Show AV job output (last N lines)\n");
            out.push_str("  av chunks <stem>       - Show chunk status for a stem\n");
            out.push_str("  av rebuild audio <stem> - Rebuild final audio from chunks (volume-aware)\n");
            out.push_str("  av config show         - Show AV config\n");
            out.push_str("  av help                - Full AV command list\n");
            out.push_str("  exit                   - Exit");
        },
        TerminalCommand::AvHelp => {
            out.push_str("AV Production commands:\n");
            out.push_str("  av init                    - Create default AV manifest\n");
            out.push_str("  av status                  - Show file status table\n");
            out.push_str("  av mark <stem> [stem2...]   - Mark files for AV production\n");
            out.push_str("  av unmark <stem> [stem2...] - Unmark files\n");
            out.push_str("  av mark-all                - Mark all woven text files\n");
            out.push_str("  av clear-marks             - Remove all marks\n");
            out.push_str("  av generate audio [stem|next|all] - Generate audio\n");
            out.push_str("  av generate video [stem|next|all] - Generate video\n");
            out.push_str("  av generate characters          - Extract character bible from text\n");
            out.push_str("  av generate prompts            - Generate illustration prompts via LLM\n");
            out.push_str("  av generate illustrations       - Generate images from prompts\n");
            out.push_str("  av config show             - Show AV config\n");
            out.push_str("  av config tts <key> <val>  - Set TTS config value\n");
            out.push_str("  av config video <key> <val> - Set video config value\n");
            out.push_str("       keys: image_duration, frame_rate, max_sentences_per_video\n");
            out.push_str("  av config illustrations <key> <val> - Set illustration config\n");
            out.push_str("  av config voices <v1> ...  - Set voice list\n");
            out.push_str("  av open book-dir|audio-dir|video-dir|illustrations - Open folder\n");
            out.push_str("  av cancel                  - Cancel running AV generation\n");
            out.push_str("  av log [N]                 - Show AV job output (last N lines)\n");
            out.push_str("  av chunks <stem>           - Show chunk status for a stem\n");
            out.push_str("  av reject chunk <stem> <N> - Mark chunk N as bad (.wav.bad)\n");
            out.push_str("  av restore chunk <stem> <N> - Restore rejected chunk N\n");
            out.push_str("  av rebuild audio <stem>    - Concatenate good chunks into final audio (volume-aware)\n");
            out.push_str("  av youtube init            - Create default _youtube.toml\n");
            out.push_str("  av youtube auth            - Run OAuth flow (one-time browser consent)\n");
            out.push_str("  av youtube config show     - Show YouTube config\n");
            out.push_str("  av youtube config <k> <v>  - Set YouTube config value\n");
            out.push_str("  av youtube upload [stem|next|all] - Upload video to YouTube\n");
            out.push_str("  av help                    - This help text\n");
            out.push_str("\nPrerequisites:\n");
            out.push_str("  Audio:  Python + Google API key (set key google AIza...)\n");
            out.push_str("  Video:  Python + ffmpeg on PATH + illustrations in book dir\n");
            out.push_str("  Illustrations: Python + Google API key (for Gemini + Imagen)\n");
            out.push_str("  YouTube: Python + google-api-python-client + OAuth client secret\n");
        },
        TerminalCommand::Clear => {
            out.push_str("\x1B[2J\x1B[1;1H");
        },
        TerminalCommand::ListNav { start_index } => {
            if engine.state.document.is_empty() {
                out.push_str("No document loaded.");
                return Some(out);
            }
            // Default: start at the selected sentence so the user always sees context
            let start = start_index.unwrap_or(engine.state.selected_sentence_idx);
            let page_size = 10; // TODO: make configurable via settings
            let end = std::cmp::min(start + page_size, engine.state.document.len());
            out.push_str(&format!("--- Navigator (sentences {} to {}) ---\n", start + 1, end));
            for i in start..end {
                let s = &engine.state.document[i];
                let marker = if i == engine.state.selected_sentence_idx { ">" } else { " " };
                let base_text = s.get_tier("base")
                    .map(|t| t.full_text())
                    .unwrap_or_else(|| "(no base tier)".to_string());
                let preview: String = base_text.chars().take(60).collect();
                out.push_str(&format!("{} [{}] {}: {}", marker, i + 1, s.id, preview));
                if i < end - 1 {
                    out.push('\n');
                }
            }
        },
        TerminalCommand::ListNavAround { center } => {
            if engine.state.document.is_empty() {
                out.push_str("No document loaded.");
                return Some(out);
            }
            let half = 5usize;
            let doc_len = engine.state.document.len();
            let start = center.saturating_sub(half);
            let end = std::cmp::min(center + half + 1, doc_len);
            out.push_str(&format!("--- Context around sentence {} ({} total) ---\n", center + 1, doc_len));
            for i in start..end {
                let s = &engine.state.document[i];
                let marker = if i == center { ">>>" } else { "   " };
                let base_text = s.get_tier("base")
                    .map(|t| t.full_text())
                    .unwrap_or_else(|| "(no base tier)".to_string());
                let preview: String = base_text.chars().take(80).collect();
                let ellipsis = if base_text.chars().count() > 80 { "..." } else { "" };
                out.push_str(&format!("{} [{}] {}{}", marker, i + 1, preview, ellipsis));
                if i < end - 1 {
                    out.push('\n');
                }
            }
        },
        TerminalCommand::SearchText { query } => {
            if engine.state.document.is_empty() {
                out.push_str("No document loaded.");
                return Some(out);
            }
            let query_lower = query.to_lowercase();
            let mut matches = Vec::new();
            for (i, s) in engine.state.document.iter().enumerate() {
                let base_text = s.get_tier("base")
                    .map(|t| t.full_text())
                    .unwrap_or_default();
                if base_text.to_lowercase().contains(&query_lower) {
                    let preview: String = base_text.chars().take(70).collect();
                    let ellipsis = if base_text.chars().count() > 70 { "..." } else { "" };
                    matches.push(format!("[{}] {}{}", i + 1, preview, ellipsis));
                }
            }
            if matches.is_empty() {
                out.push_str(&format!("No sentences found containing \"{}\".", query));
            } else {
                out.push_str(&format!("Found {} sentence(s) containing \"{}\":\n", matches.len(), query));
                // Limit output to first 50 matches to avoid flooding
                let show = matches.len().min(50);
                for m in &matches[..show] {
                    out.push_str(m);
                    out.push('\n');
                }
                if matches.len() > 50 {
                    out.push_str(&format!("... and {} more matches.", matches.len() - 50));
                }
            }
        },
        TerminalCommand::CalibrationInfo => {
            if engine.state.book_map.is_none() {
                out.push_str("No level map loaded. No calibration has been run (or imported) yet.");
                return Some(out);
            }
            let level_count = engine.state.book_map.as_ref().unwrap().len();
            let total_sentences = engine.state.document.len();
            match engine.state.calibration_sentence_count {
                Some(n) => {
                    out.push_str(&format!("Calibration info:\n"));
                    out.push_str(&format!("  Sentences used for calibration: {}\n", n));
                    out.push_str(&format!("  Total sentences in document:    {}\n", total_sentences));
                    out.push_str(&format!("  Start levels in map:            {}\n", level_count));
                    if n >= 800 {
                        out.push_str("  Status: Calibration is STABLE (≥800 sentences). Recalibration not recommended unless the book structure changed significantly.");
                    } else {
                        let remaining = 800_usize.saturating_sub(n);
                        out.push_str(&format!("  Status: Calibration is PROVISIONAL (<800 sentences). Finish ~{} more sentences before recalibrating for stable levels.", remaining));
                    }
                },
                None => {
                    out.push_str(&format!("Calibration info:\n"));
                    out.push_str(&format!("  Sentences used for calibration: unknown (level map pre-dates tracking)\n"));
                    out.push_str(&format!("  Total sentences in document:    {}\n", total_sentences));
                    out.push_str(&format!("  Start levels in map:            {}\n", level_count));
                    out.push_str("  Status: Cannot determine — re-export the level map after next calibration to record the count.");
                },
            }
        },
        TerminalCommand::ListHeadings => {
            if engine.state.document.is_empty() {
                out.push_str("No document loaded.");
                return Some(out);
            }
            // Scan for sentences that look like chapter/section headings:
            // - Short (< 80 chars) AND matches common heading patterns
            // - e.g. "Chapter X", "CHAPTER X", roman numerals, "Part X", all-caps short lines
            let mut headings = Vec::new();
            for (i, s) in engine.state.document.iter().enumerate() {
                let base_text = s.get_tier("base")
                    .map(|t| t.full_text())
                    .unwrap_or_default();
                let trimmed = base_text.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let is_heading = is_likely_heading(trimmed);
                if is_heading {
                    headings.push(format!("[{}] {}", i + 1, trimmed));
                }
            }
            if headings.is_empty() {
                out.push_str("No headings detected. Try `search \"Chapter\"` for manual lookup.");
            } else {
                out.push_str(&format!("Found {} likely heading(s):\n", headings.len()));
                for h in &headings {
                    out.push_str(h);
                    out.push('\n');
                }
            }
        },
        TerminalCommand::ShowDetail => {
            if engine.state.document.is_empty() {
                out.push_str("No document loaded.");
                return Some(out);
            }
            let idx = engine.state.selected_sentence_idx;
            let sel_tier = &engine.state.selected_tier_id;
            if let Some(s) = engine.state.document.get(idx) {
                out.push_str(&format!("--- Sentence {} ({}) [tier: {}] ---",
                    s.id, idx + 1, tier_display_alias(sel_tier)));
                for &tid in crate::domain::sentence::Sentence::WEAVE_TIERS {
                    if let Some(tier) = s.tiers.get(tid) {
                        let alias = tier_display_alias(tid);
                        let marker = if tid == sel_tier { ">" } else { " " };
                        let text = tier.full_text();
                        let preview: String = text.chars().take(70).collect();
                        let ellipsis = if text.len() > 70 { "..." } else { "" };
                        out.push_str(&format!("\n {} {:<5} ({:?}): {}{}",
                            marker, alias, tier.state, preview, ellipsis));
                    } else {
                        let alias = tier_display_alias(tid);
                        out.push_str(&format!("\n   {:<5} (missing)", alias));
                    }
                }
                out.push_str(&format!("\n  Mappings: {}", s.mappings.len()));
            }
        },
        TerminalCommand::ShowMapping => {
            if engine.state.document.is_empty() {
                out.push_str("No document loaded.");
                return Some(out);
            }
            let idx = engine.state.selected_sentence_idx;
            let tier_id = engine.state.selected_tier_id.clone();
            if let Some(s) = engine.state.document.get(idx) {
                // Find the mapping where the selected tier is the source
                let mapping = s.mappings.iter()
                    .find(|m| m.from_tier_id == tier_id);
                match mapping {
                    Some(m) => {
                        out.push_str(&format!("--- Mapping: {} → {} (sentence {}) ---", m.from_tier_id, m.to_tier_id, idx + 1));
                        // Build a lookup from WordId → mapping entry
                        let entry_map: std::collections::HashMap<crate::domain::primitives::WordId, &crate::domain::mapping::MappingEntry> =
                            m.entries.iter().map(|e| (e.source_word_id, e)).collect();
                        // Get the token list for the source tier
                        if let Some(tier) = s.tiers.get(&tier_id) {
                            if let Some(seg) = tier.segments.first() {
                                let words = seg.stream.words_enumerated();
                                out.push_str(&format!("\n  {:>4}  {:<20} → {:<20} lemmas", "#", "source", "target"));
                                out.push_str(&format!("\n  {:>4}  {:<20}   {:<20} ------", "--", "------", "------"));
                                for (one_idx, _tok_idx, wd) in &words {
                                    let target_text = entry_map.get(&wd.id)
                                        .map(|e| e.target_text.as_str())
                                        .unwrap_or("—");
                                    let target_lemmas = entry_map.get(&wd.id)
                                        .map(|e| format_lemmas_with_ranks(&e.target_lemmas))
                                        .unwrap_or_default();
                                    out.push_str(&format!("\n  {:>4}  {:<20} → {:<20} {}", one_idx, wd.text, target_text, target_lemmas));
                                }
                            }
                        }
                    }
                    None => {
                        // Fall back: show all mappings for the sentence
                        out.push_str(&format!("--- Mappings for {} (sentence {}) ---", s.id, idx + 1));
                        if s.mappings.is_empty() {
                            out.push_str("\n  (no mappings)");
                        } else {
                            for m in &s.mappings {
                                out.push_str(&format!("\n  {} → {} ({} entries)", m.from_tier_id, m.to_tier_id, m.entries.len()));
                            }
                        }
                    }
                }
            }
        },
        TerminalCommand::ShowTokens => {
            if engine.state.document.is_empty() {
                out.push_str("No document loaded.");
                return Some(out);
            }
            let idx = engine.state.selected_sentence_idx;
            let tier_id = engine.state.selected_tier_id.clone();
            if let Some(s) = engine.state.document.get(idx) {
                if let Some(tier) = s.tiers.get(&tier_id) {
                    out.push_str(&format!("--- Tokens for sentence {} tier '{}' ---", idx + 1, tier_id));
                    for seg in &tier.segments {
                        if tier.segments.len() > 1 {
                            out.push_str(&format!("\n  [{}]:", seg.id));
                        }
                        let words = seg.stream.words_enumerated();
                        out.push_str(&format!("\n  Word count: {}", words.len()));
                        for (one_idx, _tok_idx, wd) in &words {
                            let lemma_str = if wd.lemmas.is_empty() {
                                String::new()
                            } else {
                                format!("  ({})", wd.lemmas.join(", "))
                            };
                            out.push_str(&format!("\n  {:>3}: \"{}\"{}", one_idx, wd.text, lemma_str));
                        }
                    }
                } else {
                    out.push_str(&format!("Tier '{}' not found. Use 'select tier <tier>' first.", tier_id));
                }
            }
        },
        TerminalCommand::Print { tier } => {
            if engine.state.document.is_empty() {
                out.push_str("No document loaded.");
                return Some(out);
            }
            let idx = engine.state.selected_sentence_idx;
            let tier_id = tier.unwrap_or_else(|| engine.state.selected_tier_id.clone());
            if let Some(s) = engine.state.document.get(idx) {
                if let Some(t) = s.tiers.get(&tier_id) {
                    let alias = tier_display_alias(&tier_id);
                    let full_text = t.full_text();
                    out.push_str(&format!("S{} [{}] ({:?}): {}",
                        idx + 1, alias, t.state, full_text));

                    // For mapped tiers (bas_b, bas_t): show token/mapping table
                    let is_mapped = tier_id == "basic_base" || tier_id == "basic_target";
                    if is_mapped {
                        // Find the mapping where this tier is the SOURCE
                        let mapping = s.mappings.iter()
                            .find(|m| m.from_tier_id == tier_id);
                        let entry_map: std::collections::HashMap<crate::domain::primitives::WordId, &crate::domain::mapping::MappingEntry> =
                            mapping.map(|m| m.entries.iter().map(|e| (e.source_word_id, e)).collect())
                            .unwrap_or_default();

                        if let Some(seg) = t.segments.first() {
                            // Compute column widths from actual data
                            let words = seg.stream.words_enumerated();
                            let mut src_w: usize = 6;  // "source"
                            let mut tgt_w: usize = 6;  // "target"
                            let mut lem_w: usize = 6;  // "lemmas"
                            for (_one_idx, _tok_idx, wd) in &words {
                                src_w = src_w.max(wd.text.len());
                                if let Some(e) = entry_map.get(&wd.id) {
                                    tgt_w = tgt_w.max(e.target_text.len());
                                    let lstr = format_lemmas_with_ranks(&e.target_lemmas);
                                    lem_w = lem_w.max(lstr.len());
                                }
                            }
                            // Cap widths to reasonable max
                            src_w = src_w.min(30);
                            tgt_w = tgt_w.min(30);
                            lem_w = lem_w.min(40);

                            // Column headers
                            out.push_str(&format!("\n  {:>3}  {:<sw$} | {:<tw$} | {}",
                                "#", "Source", "Target", "Lemmas",
                                sw = src_w, tw = tgt_w));
                            out.push_str(&format!("\n  {:->3}  {:->sw$} | {:->tw$} | {:->lw$}",
                                "", "", "", "",
                                sw = src_w, tw = tgt_w, lw = lem_w));

                            // Walk all tokens (B-W-B-W-B) to show fills and words
                            let tokens = seg.stream.tokens();
                            let mut word_num: usize = 0;
                            for tok in tokens {
                                match tok {
                                    crate::domain::token_stream::Token::Background(bg) => {
                                        let trimmed = bg.trim();
                                        if !trimmed.is_empty() {
                                            out.push_str(&format!("\n  {:>3}  {:<sw$} | {:<tw$} |",
                                                "", trimmed, "[-]",
                                                sw = src_w, tw = tgt_w));
                                        }
                                    }
                                    crate::domain::token_stream::Token::Word(wd) => {
                                        word_num += 1;
                                        let target_text = entry_map.get(&wd.id)
                                            .map(|e| e.target_text.as_str())
                                            .unwrap_or("—");
                                        let target_lemmas = entry_map.get(&wd.id)
                                            .map(|e| format_lemmas_with_ranks(&e.target_lemmas))
                                            .unwrap_or_default();
                                        out.push_str(&format!("\n  {:>3}  {:<sw$} | {:<tw$} | {}",
                                            word_num, wd.text, target_text, target_lemmas,
                                            sw = src_w, tw = tgt_w));
                                    }
                                }
                            }

                            if mapping.is_none() {
                                out.push_str("\n  (no mapping — use edit_target or edit_targets to create)");
                            }
                        }
                    } else {
                        // Segment tiers (adv, mod, base/source): show segments
                        for seg in &t.segments {
                            let lemma_str = if seg.lemmas.is_empty() {
                                String::new()
                            } else {
                                format!("  [{}]", format_lemmas_with_ranks(&seg.lemmas))
                            };
                            out.push_str(&format!("\n  [{}]: {}{}", seg.id, seg.full_text(), lemma_str));
                        }
                    }
                } else {
                    out.push_str(&format!("Tier '{}' not found on sentence {}.", tier_id, idx + 1));
                }
            }
        },
        TerminalCommand::WatchJob => {
            // Block until the current LLM job completes, draining and applying results.
            if engine.state.llm_results_receiver.is_none() {
                out.push_str("No LLM job in progress.");
            } else {
                let total = engine.state.llm_job_total;
                let mut applied = 0usize;
                let (base_lang, target_lang) = engine.state.project_languages.clone();

                // Take the receiver out so we can loop without borrow issues
                let rx = engine.state.llm_results_receiver.take().unwrap();

                loop {
                    match rx.recv() {
                        Ok(Ok(results)) => {
                            let num = results.len();
                            for (idx, _s_id, tier_id, text) in results {
                                if idx < engine.state.document.len() {
                                    let lang = crate::services::tier_processor::lang_for_tier(
                                        &tier_id, &base_lang, &target_lang,
                                    );
                                    let bridge_ref = engine.state.bridge.as_ref();
                                    if let Some(sent) = engine.state.document.get_mut(idx) {
                                        apply_llm_result(sent, &tier_id, &text, bridge_ref, &lang, &target_lang);
                                    }
                                }
                            }
                            applied += num;
                            engine.state.llm_job_done = applied;

                            // Check if all done
                            if total > 0 && applied >= total {
                                break;
                            }
                        }
                        Ok(Err(e)) => {
                            let log_hint = engine.state.logger.as_ref()
                                .map(|l| format!("\nLLM log: {}", l.log_file_path().display()))
                                .unwrap_or_default();
                            if applied > 0 {
                                out.push_str(&format!(
                                    "Error after {}/{} items applied: {}{}",
                                    applied, total, e, log_hint
                                ));
                            } else {
                                out.push_str(&format!("Error: {}{}", e, log_hint));
                            }
                            break;
                        }
                        Err(_) => {
                            // Channel disconnected — sender is done
                            break;
                        }
                    }
                }

                // Clean up
                engine.state.llm_results_receiver = None;
                engine.state.llm_cancel_flag = None;

                if out.is_empty() {
                    out.push_str(&format!("LLM job completed. {} items applied.", applied));
                }

                // Auto-advance follow-up queue (e.g. mapping generation after tier generation)
                let had_error = out.contains("Error");
                if !had_error {
                    while let Some(next_cmd) = engine.state.llm_followup_queue.pop_front() {
                        out.push_str(&format!("\n\n--- Follow-up: {} ---\n", next_cmd));
                        match crate::app::terminal::parse_command(&next_cmd) {
                            Ok(cmd) => {
                                if let Some(result) = execute_command(engine, cmd) {
                                    out.push_str(&result);
                                }
                            }
                            Err(e) => {
                                out.push_str(&format!("Parse error: {}", e));
                                break;
                            }
                        }
                        // If the follow-up spawned a new LLM job, block on it
                        if engine.state.llm_results_receiver.is_some() {
                            out.push_str("\n");
                            // Recursively watch this sub-job (inline drain)
                            let sub_total = engine.state.llm_job_total;
                            let mut sub_applied = 0usize;
                            let sub_rx = engine.state.llm_results_receiver.take().unwrap();
                            loop {
                                match sub_rx.recv() {
                                    Ok(Ok(results)) => {
                                        let num = results.len();
                                        for (idx, _s_id, tier_id, text) in results {
                                            if idx < engine.state.document.len() {
                                                let lang = crate::services::tier_processor::lang_for_tier(
                                                    &tier_id, &base_lang, &target_lang,
                                                );
                                                let bridge_ref = engine.state.bridge.as_ref();
                                                if let Some(sent) = engine.state.document.get_mut(idx) {
                                                    apply_llm_result(sent, &tier_id, &text, bridge_ref, &lang, &target_lang);
                                                }
                                            }
                                        }
                                        sub_applied += num;
                                        engine.state.llm_job_done = sub_applied;
                                        if sub_total > 0 && sub_applied >= sub_total {
                                            break;
                                        }
                                    }
                                    Ok(Err(e)) => {
                                        out.push_str(&format!("Follow-up error: {}", e));
                                        // Clear remaining queue on error
                                        engine.state.llm_followup_queue.clear();
                                        break;
                                    }
                                    Err(_) => break,
                                }
                            }
                            engine.state.llm_results_receiver = None;
                            engine.state.llm_cancel_flag = None;
                            out.push_str(&format!("Follow-up completed. {} items applied.", sub_applied));
                        }
                    }
                } else {
                    let cleared = engine.state.llm_followup_queue.len();
                    engine.state.llm_followup_queue.clear();
                    if cleared > 0 {
                        out.push_str(&format!("\n({} queued follow-up steps cancelled due to error.)", cleared));
                    }
                }
            }
        },
        TerminalCommand::JobStatus => {
            // Non-blocking job status check — returns immediately with progress info.
            // Designed for copilot/agent polling instead of blocking watch_job.
            if engine.state.llm_results_receiver.is_some() {
                let done = engine.state.llm_job_done;
                let total = engine.state.llm_job_total;
                let stage = &engine.state.llm_job_stage;
                let tier = &engine.state.llm_job_target_tier;
                out.push_str(&format!("RUNNING {}/{} stage={} tier={}", done, total, stage, tier));
            } else if engine.state.llm_job_total > 0 && engine.state.llm_job_done >= engine.state.llm_job_total {
                out.push_str(&format!("DONE {}/{} stage={} tier={}",
                    engine.state.llm_job_done, engine.state.llm_job_total,
                    engine.state.llm_job_stage, engine.state.llm_job_target_tier));
            } else if let Some(ref av) = engine.state.av_job {
                let j = av.lock().unwrap();
                if j.finished {
                    let msg = j.result_message.as_deref().unwrap_or("AV job finished.");
                    out.push_str(&format!("AV_DONE {}", msg));
                } else {
                    out.push_str(&format!("AV_RUNNING {}", j.label));
                }
            } else {
                out.push_str("IDLE");
            }
        },
        TerminalCommand::ServerInfo => {
            // The actual server name/port are only known by the GUI layer
            // which manages the relay. This command is handled specially by the
            // GUI's execute_terminal_command override. In headless mode (CLI REPL)
            // there is no copilot server.
            out.push_str("Copilot server is not running (headless/REPL mode).\nUse the GUI for co-pilot mode, or start a daemon with 'weavelang_cli daemon start'.");
        },
        TerminalCommand::App(app_cmd) => {
            match engine.execute(app_cmd) {
                Ok(msg) => out.push_str(&msg),
                Err(e) => out.push_str(&format!("Error: {}", e)),
            }
        },
        _ => out.push_str("Command not implemented yet."),
    }

    Some(out)
}

/// Convenience: parse and execute in one call. Returns the output string.
/// Returns None for Exit.
pub fn run_terminal_command(engine: &mut Engine, input: &str) -> Result<Option<String>, String> {
    let cmd = parse_command(input)?;
    Ok(execute_command(engine, cmd))
}

/// Apply an LLM result to a sentence.
/// Shared by both the terminal and GUI front ends — the single source of truth.
///
/// If the text starts with `SEG_FAIL_PREFIX`, segmentation failed gracefully.
/// The prefix is stripped, the translation is applied, and the tier is marked
/// Stale instead of Valid so the UI signals the partial failure.
pub fn apply_llm_result(
    sent: &mut Sentence,
    tier_id: &str,
    text: &str,
    bridge: Option<&crate::services::python_bridge::BridgeService>,
    lang_code: &str,
    target_lang_code: &str,
) {
    use crate::services::llm_worker::SEG_FAIL_PREFIX;

    // Detect and strip the segmentation-failure marker.
    let (actual_text, seg_failed) = if text.starts_with(SEG_FAIL_PREFIX) {
        (&text[SEG_FAIL_PREFIX.len()..], true)
    } else {
        (text, false)
    };

    if tier_id.starts_with("MAPPING:") {
        // Format: MAPPING:source:target
        let parts: Vec<&str> = tier_id.split(':').collect();
        if parts.len() == 3 {
            let source_id = parts[1];
            let target_id = parts[2];

            let mut mapping_to_add: Option<TierMapping> = None;
            let sent_id_for_err = sent.id.clone();

            if let Some(source_tier) = sent.get_tier_mut(source_id) {
                if let Some(segment) = source_tier.segments.first_mut() {
                    // Re-tokenize the stream from its full text so that any
                    // previous fusion (from an earlier mapping attempt) is
                    // undone. This ensures the individual word tokens are
                    // available for the new LLM groupings to match against.
                    let original_text = segment.stream.full_text();
                    let retok_lang = if source_id.contains("target") {
                        target_lang_code
                    } else {
                        lang_code
                    };
                    if let Some(br) = bridge {
                        if let Ok(raw_tokens) = br.tokenize(&original_text, retok_lang) {
                            segment.stream = crate::domain::token_stream::TokenStream::from_raw_spacy(raw_tokens, &original_text);
                        }
                    }

                    // Clone the stream before fusion so we can restore on failure.
                    let stream_backup = segment.stream.clone();
                    match apply_llm_mapping(&mut segment.stream, actual_text, source_id, target_id) {
                        Ok(m) => mapping_to_add = Some(m),
                        Err(e) => {
                            eprintln!("Mapping Error on {}: {}", sent_id_for_err, e);
                            // Restore the stream to its pre-fusion state
                            segment.stream = stream_backup;
                            if let Some(tier) = sent.tiers.get_mut(source_id) {
                                tier.state = crate::domain::tier::TierState::Broken;
                            }
                        }
                    }
                }
            }

            if let Some(m) = mapping_to_add {
                let source_id_owned = source_id.to_string();
                sent.add_mapping(m);

                // Mirror the Python pipeline's FinalizeMappings stage:
                // populate target_lemmas on every mapping entry using
                // SpaCy (forward diglot) or the basic_target token
                // stream (inverse diglot).
                sent.finalize_mapping_lemmas(bridge, target_lang_code);

                // Round-trip validation: upgrade the source tier from Stale
                // to Valid if every word now has a mapping entry, or Broken
                // if the mapping is incomplete.
                if sent.check_mapping_coverage(&source_id_owned) {
                    if let Some(tier) = sent.get_tier_mut(&source_id_owned) {
                        if tier.state == crate::domain::tier::TierState::Stale
                            || tier.state == crate::domain::tier::TierState::Pending
                            || tier.state == crate::domain::tier::TierState::Broken
                            || tier.state == crate::domain::tier::TierState::Dirty
                        {
                            tier.state = crate::domain::tier::TierState::Valid;
                        }
                    }
                } else {
                    if let Some(tier) = sent.get_tier_mut(&source_id_owned) {
                        tier.state = crate::domain::tier::TierState::Broken;
                    }
                }
            }
        }
    } else {
        if actual_text.contains('\0') {
            // Null byte denotes pre-segmented text from the LLM worker.
            // Mirror Python's `reconstruct_and_separate_segments`: add a
            // trailing space to every non-final segment that lacks one.
            let raw_segs: Vec<&str> = actual_text.split('\0').collect();
            let num_segs = raw_segs.len();
            let mut all_segments = Vec::new();
            for (i, raw) in raw_segs.into_iter().enumerate() {
                let seg_id = format!("S{}", i + 1);
                // Add separator space for non-final segments (Python parity).
                let owned: String;
                let seg_text: &str = if i < num_segs - 1 && !raw.ends_with(' ') {
                    owned = format!("{} ", raw);
                    &owned
                } else {
                    raw
                };
                let stream = crate::services::tier_processor::tokenize_only(seg_text, lang_code, bridge)
                    .into_iter()
                    .next()
                    .unwrap()
                    .stream; // tokenize_only returns a single Segment; extract its stream
                all_segments.push(crate::domain::segment::Segment::from_stream(seg_id, stream, vec![]));
            }
            sent.update_tier_with_segments(tier_id, all_segments);
        } else {
            let segments = crate::services::tier_processor::tokenize_only(actual_text, lang_code, bridge);
            sent.update_tier_with_segments(tier_id, segments);
        }

        // Store the input text so we can validate the round-trip later.
        if let Some(tier) = sent.get_tier_mut(tier_id) {
            tier.input_text = Some(actual_text.to_string());
        }

        // NOTE: basic_base / basic_target are left Valid after LLM generation.
        // The follow-up mapping stage (GeneratePhraseMap) requires the source
        // tier to be Valid, so downgrading to Stale here would deadlock the
        // auto-queued follow-up pipeline.  The follow-up queue itself signals
        // that mapping is still pending.
        if matches!(tier_id, "basic_base" | "basic_target") {
            if let Some(tier) = sent.get_tier_mut(tier_id) {
                tier.state = crate::domain::tier::TierState::Pending;
            }
        }

        // If segmentation failed, downgrade from Valid to Stale so the UI shows a warning.
        if seg_failed {
            if let Some(tier) = sent.get_tier_mut(tier_id) {
                tier.state = crate::domain::tier::TierState::Stale;
            }
        }

        // Integrity check: moderate_target must have the same segment count
        // as advanced_target. A mismatch means the weave would produce garbled
        // output. Mark the tier Stale so the user sees the problem.
        if tier_id == "moderate_target" {
            let mod_count = sent.get_tier("moderate_target").map(|t| t.segments.len()).unwrap_or(0);
            let adv_count = sent.get_tier("advanced_target").map(|t| t.segments.len()).unwrap_or(0);
            if adv_count > 0 && mod_count != adv_count {
                eprintln!(
                    "[Integrity] {} moderate_target has {} segments but advanced_target has {} — marking Stale",
                    sent.id, mod_count, adv_count
                );
                if let Some(tier) = sent.get_tier_mut("moderate_target") {
                    tier.state = crate::domain::tier::TierState::Stale;
                }
            }
        }

        // Auto-lemmatize: populate word lemmas on freshly-generated tiers so
        // the user doesn't have to manually approve every LLM result.
        // Tier language: base/basic_base are source-language, all others target.
        if let Some(br) = bridge {
            let tier_lang = if tier_id == "base" || tier_id == "basic_base" {
                lang_code
            } else {
                target_lang_code
            };
            if let Err(e) = crate::app::engine::lemmatize_tier_segments(sent, tier_id, br, tier_lang) {
                eprintln!("[Auto-lemmatize] segment lemmas failed for {}: {}", tier_id, e);
            }
            if let Err(e) = crate::app::engine::lemmatize_mapping_targets(sent, tier_id, br, lang_code, target_lang_code) {
                eprintln!("[Auto-lemmatize] mapping lemmas failed for {}: {}", tier_id, e);
            }
        }
    }
}
