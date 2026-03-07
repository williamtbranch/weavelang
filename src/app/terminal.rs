// src/app/terminal.rs
//
// Terminal command parsing and execution — shared by the interactive REPL and the API server.
// All output is returned as String rather than printed, so both frontends can use it.

use crate::app::commands::{AppCommand, TerminalCommand};
use crate::app::engine::Engine;
use crate::domain::mapping_logic::apply_llm_mapping;
use crate::domain::mapping::TierMapping;
use crate::domain::sentence::Sentence;

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
            if parts.len() > 1 && parts[1] == "sentence" {
                Ok(TerminalCommand::App(AppCommand::AddSentence))
            } else if parts.len() > 3 && parts[1] == "pn_lemma" {
                let n = parts[2].parse::<usize>().map_err(|_| "Invalid sentence number")?;
                if n == 0 { return Err("Sentence numbers start at 1".to_string()); }
                let lemma = parts[3..].join(" ");
                Ok(TerminalCommand::App(AppCommand::AddPnLemma { index: n - 1, lemma }))
            } else {
                Err("Usage: add sentence | add pn_lemma <N> <lemma>".to_string())
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
            } else {
                Err("Usage: select sentence <id|number>  (numbers are 1-based)".to_string())
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
            if parts.len() > 1 && parts[1] == "collateral" {
                Ok(TerminalCommand::App(AppCommand::ApplyCollateral { accept: true }))
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
                Err("Usage: approve collateral | approve edits <id|number> <tier_id>".to_string())
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
            } else {
                Err("Usage: set right_view <v> | set left_view <v> | set output_dir <p> | set key <anthropic|google> <value>".to_string())
            }
        },
        "show" => {
            if parts.len() > 1 && parts[1] == "detail" {
                Ok(TerminalCommand::ShowDetail)
            } else if parts.len() > 1 && parts[1] == "mapping" {
                Ok(TerminalCommand::ShowMapping)
            } else {
                Err("Usage: show detail | show mapping".to_string())
            }
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
                let level = parts[1].to_string();
                Ok(TerminalCommand::App(AppCommand::GenerateWeave { level }))
            } else {
                Err("Usage: generate_weave <level|all>".to_string())
            }
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
            } else {
                Err("Usage: rm pn_lemma <N> <lemma>".to_string())
            }
        },
        "delete" => {
            if parts.len() > 2 && parts[1] == "key" {
                Ok(TerminalCommand::App(AppCommand::DeleteKey { provider: parts[2].to_string() }))
            } else {
                Err("Usage: delete key <anthropic|google>".to_string())
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
            out.push_str("  list nav [N]           - List navigator sentences (N is 1-based)\n");
            out.push_str("  select sentence <N>    - Select sentence by number (1-based) or ID\n");
            out.push_str("  import source <path>   - Import source text file\n");
            out.push_str("  open workspace <path>  - Open (or create) a workspace directory\n");
            out.push_str("  load project <path>    - Load a .wvl file\n");
            out.push_str("  save project [path]    - Save project\n");
            out.push_str("  config set <k> <v>     - Set config value\n");
            out.push_str("  config list            - List config\n");
            out.push_str("  export level_map [p]   - Export level map (.lm file)\n");
            out.push_str("  run generate <S> <s> <e> - Run generation stage (s,e are 1-based)\n");
            out.push_str("  approve collateral     - Approve collateral updates\n");
            out.push_str("  show detail            - Show selected sentence details\n");
            out.push_str("  show mapping           - Show mappings for selected sentence\n");
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
            out.push_str("  calibrate [max_level]  - Run calibration on loaded document (default max: 45)\n");
            out.push_str("  list pn_lemmas <N>     - List proper noun lemmas for sentence N (1-based)\n");
            out.push_str("  add pn_lemma <N> <L>   - Add lemma L to sentence N's PN list\n");
            out.push_str("  rm pn_lemma <N> <L>    - Remove lemma L from sentence N's PN list\n");
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
            if let Some(s) = engine.state.document.get(idx) {
                out.push_str(&format!("--- Sentence {} ({}) ---", s.id, idx + 1));
                for (tier_id, tier) in &s.tiers {
                    out.push_str(&format!("\n  Tier '{}': {}", tier_id, tier.full_text()));
                }
            }
        },
        TerminalCommand::ShowMapping => {
            if engine.state.document.is_empty() {
                out.push_str("No document loaded.");
                return Some(out);
            }
            let idx = engine.state.selected_sentence_idx;
            if let Some(s) = engine.state.document.get(idx) {
                out.push_str(&format!("--- Mappings for {} (index {}) ---", s.id, idx));
                if s.mappings.is_empty() {
                    out.push_str("\n  (no mappings)");
                } else {
                    for mapping in &s.mappings {
                        out.push_str(&format!("\n  {:?}", mapping));
                    }
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
            }
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
    }
}
