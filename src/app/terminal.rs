// src/app/terminal.rs
//
// Terminal command parsing and execution — shared by the interactive REPL and the API server.
// All output is returned as String rather than printed, so both frontends can use it.

use crate::app::commands::{AppCommand, TerminalCommand};
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
fn tier_display_alias(tier_id: &str) -> &str {
    match tier_id {
        "advanced_target" => "adv",
        "moderate_target" => "mod",
        "basic_target" => "bas_t",
        "basic_base" => "bas_b",
        "base" => "base",
        other => other,
    }
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
                let start_index = if parts.len() > 2 {
                    parts[2].parse::<usize>().ok().map(|n| n.saturating_sub(1))
                } else {
                    None
                };
                Ok(TerminalCommand::ListNav { start_index })
            } else if parts.len() > 2 && parts[1] == "pn_lemmas" {
                let n = parts[2].parse::<usize>().map_err(|_| "Invalid sentence number")?;
                if n == 0 { return Err("Sentence numbers start at 1".to_string()); }
                Ok(TerminalCommand::App(AppCommand::ListPnLemmas { index: n - 1 }))
            } else {
                Err("Unknown list command. Try 'list nav' or 'list pn_lemmas <N>'".to_string())
            }
        },
        "load" => {
            if parts.len() > 2 && parts[1] == "project" {
                Ok(TerminalCommand::App(AppCommand::LoadProject { path: parts[2..].join(" ") }))
            } else {
                Err("Usage: load project <path>".to_string())
            }
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
            if parts.len() > 2 && parts[1] == "sentence" {
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
            } else if parts[1] == "collateral" {
                Ok(TerminalCommand::App(AppCommand::ApplyCollateral { accept: true }))
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
                Err("Usage: approve | approve tier <N> <tier> | approve collateral".to_string())
            }
        },
        "discard" => {
            if parts.len() > 1 && parts[1] == "collateral" {
                Ok(TerminalCommand::App(AppCommand::ApplyCollateral { accept: false }))
            } else {
                 Err("Usage: discard collateral".to_string())
            }
        },
        "run" => {
             if parts.len() > 4 && parts[1] == "generate" {
                 // run generate <StageName> <start> <end>  (1-based inclusive)
                 let stage_name = parts[2].to_string();
                 let start_num = parts[3].parse::<usize>().map_err(|_| "Invalid start number")?;
                 let end_num = parts[4].parse::<usize>().map_err(|_| "Invalid end number")?;
                 if start_num == 0 || end_num == 0 {
                     return Err("Sentence numbers start at 1".to_string());
                 }
                 Ok(TerminalCommand::App(AppCommand::GenerateStage { stage_name, start_index: start_num - 1, end_index: end_num - 1 }))
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
            } else {
                Err("Usage: set right_view <v> | set left_view <v> | set output_dir <p> | set key <anthropic|google> <value> | set languages <source> <target> | set book_name <name>".to_string())
            }
        },
        "show" => {
            if parts.len() > 1 && parts[1] == "detail" {
                Ok(TerminalCommand::ShowDetail)
            } else if parts.len() > 1 && parts[1] == "mapping" {
                Ok(TerminalCommand::ShowMapping)
            } else if parts.len() > 1 && parts[1] == "tokens" {
                Ok(TerminalCommand::ShowTokens)
            } else {
                Err("Usage: show detail | show mapping | show tokens".to_string())
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
        "calibrate" => {
            let max_level = if parts.len() > 1 {
                Some(parts[1].parse::<u32>().map_err(|_| "Invalid max level number")?)
            } else {
                None
            };
            Ok(TerminalCommand::App(AppCommand::Calibrate { max_level }))
        },
        "generate_weave" => {
            if parts.len() > 1 {
                let mut level = parts[1].to_string();
                let force = parts.iter().any(|p| *p == "--force");
                // If --force was the level arg, look for the real level
                if level == "--force" && parts.len() > 2 {
                    level = parts[2].to_string();
                }
                Ok(TerminalCommand::App(AppCommand::GenerateWeave { level, force }))
            } else {
                Err("Usage: generate_weave <level|all> [--force]".to_string())
            }
        },
        "drc" => {
            Ok(TerminalCommand::App(AppCommand::Drc))
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
            if parts.len() > 2 && parts[1] == "key" {
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
                    Ok(TerminalCommand::App(AppCommand::ReportSentencesIncomplete))
                } else if parts.len() > 2 && parts[2] == "complete" {
                    Ok(TerminalCommand::App(AppCommand::ReportSentencesComplete))
                } else {
                    Err("Usage: report sentences incomplete | report sentences complete".to_string())
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
            if parts.len() > 1 && parts[1] == "map" {
                Ok(TerminalCommand::App(AppCommand::AcceptMap))
            } else {
                Err("Usage: accept map".to_string())
            }
        },
        "init" => {
            if parts.len() > 1 && parts[1] == "mapping" {
                Ok(TerminalCommand::App(AppCommand::InitMapping))
            } else {
                Err("Usage: init mapping".to_string())
            }
        },
        "server" => {
            if parts.len() > 1 && parts[1] == "info" {
                Ok(TerminalCommand::ServerInfo)
            } else {
                Err("Usage: server info".to_string())
            }
        },
        "new" => {
            if parts.len() > 2 && parts[1] == "project" {
                let name = parts[2..].join(" ");
                Ok(TerminalCommand::App(AppCommand::NewProject { name }))
            } else {
                Err("Usage: new project <name>".to_string())
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
        _ => Err(format!("Unknown command: {}", parts[0])),
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
            out.push_str("  run generate <S> <s> <e> - Run generation stage (s,e are 1-based)\n");
            out.push_str("  approve collateral     - Approve collateral updates\n");
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
            out.push_str("  import level_map <p>   - Import a .lm level map file\n");
            out.push_str("  set output_dir <p>     - Set output directory for weave files\n");
            out.push_str("  generate_weave <N|all> - Generate weave text file(s) for level N or all\n");
            out.push_str("  generate_weave <N|all> --force - Generate weave, skip DRC\n");
            out.push_str("  drc                    - Run Design Rule Check on all sentences\n");
            out.push_str("  calibrate [max_level]  - Run calibration on loaded document (default max: 45)\n");
            out.push_str("  list pn_lemmas <N>     - List proper noun lemmas for sentence N (1-based)\n");
            out.push_str("  add pn_lemma <N> <L>   - Add lemma L to sentence N's PN list\n");
            out.push_str("  rm pn_lemma <N> <L>    - Remove lemma L from sentence N's PN list\n");
            out.push_str("\n--- Segment Editing (Adv/Mod) ---\n");
            out.push_str("  edit seg <N> <tier> <seg_id> <text>  - Edit segment text\n");
            out.push_str("  add seg <N> <tier> <after> <text>    - Add segment after <after>\n");
            out.push_str("  rm seg <N> <tier> <seg_id>           - Remove segment\n");
            out.push_str("  lemmatize <N> <tier>   - Re-lemmatize tier segments via SpaCy\n");
            out.push_str("  validate <N> <tier>    - Lemmatize + mark tier Valid\n");
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
            out.push_str("\n--- Co-Pilot ---\n");
            out.push_str("  server info            - Show copilot server name and port\n");
            out.push_str("  exit                   - Exit");
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

            if let Some(source_tier) = sent.get_tier_mut(source_id) {
                if let Some(segment) = source_tier.segments.first_mut() {
                    match apply_llm_mapping(&mut segment.stream, actual_text, source_id, target_id) {
                        Ok(m) => mapping_to_add = Some(m),
                        Err(e) => eprintln!("Mapping Error: {}", e),
                    }
                }
            }

            if let Some(m) = mapping_to_add {
                sent.add_mapping(m);

                // Mirror the Python pipeline's FinalizeMappings stage:
                // populate target_lemmas on every mapping entry using
                // SpaCy (forward diglot) or the basic_target token
                // stream (inverse diglot).
                sent.finalize_mapping_lemmas(bridge, target_lang_code);
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
