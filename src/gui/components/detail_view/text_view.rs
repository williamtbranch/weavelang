// src/gui/components/detail_view/text_view.rs

use crate::domain::tier::TierState;
use crate::domain::token_stream::{Token, TokenStream};
use crate::gui::preview;
use crate::app::state::{AppState, TierView};
use crate::services::llm_worker::spawn_llm_job;
use eframe::egui;
use regex::Regex;

const DEFAULT_MODEL: &str = "claude-3-haiku-20240307";

fn extract_translation_from_response(raw_response: &str, target_id: &str) -> String {
    let num_str: String = target_id.chars().filter(|c| c.is_ascii_digit()).collect();

    let pattern_str = format!(r"(?mi)^(?:id\s*)?S0*{num_str}\s*[:.]\s*(.*)$");
    if let Ok(specific_re) = Regex::new(&pattern_str) {
        for line in raw_response.lines() {
            let trimmed = line.trim();
            if let Some(caps) = specific_re.captures(trimmed) {
                return caps.get(1).unwrap().as_str().trim().to_string();
            }
        }
    }

    let generic_re = Regex::new(r"(?mi)^(?:id\s*)?S\d+\s*[:.]\s*(.*)$").unwrap();
    if let Some(caps) = generic_re.captures(raw_response) {
        return caps.get(1).unwrap().as_str().trim().to_string();
    }

    let non_empty_lines: Vec<&str> = raw_response
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    if let Some(last) = non_empty_lines.last() {
        return last.trim().to_string();
    }

    raw_response.trim().to_string()
}

pub fn render(ui: &mut egui::Ui, view: TierView, state: &mut AppState) {
    if view == TierView::Simulation {
        ui.heading("Tier: Live Simulation");
        ui.separator();
        if let Some(sentence) = state.get_current_sentence() {
            let recipe = state.get_effective_recipe();
            let mut text = preview::generate_preview_text(sentence, &recipe);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut text)
                            .font(egui::TextStyle::Body)
                            .desired_width(f32::INFINITY)
                            .frame(false),
                    );
                });
        } else {
            ui.label("No sentence selected.");
        }
        return;
    }

    // --- Configuration for this View ---
    let (tier_id, lang_code, parent_tier_id, prompt_name) = match view {
        // Base is the Root.
        TierView::Base => ("base", "en", None, None),

        // Advanced depends on Base (Translation)
        TierView::AdvancedTarget => (
            "advanced_target",
            "es",
            Some("base"),
            Some("translate_text"),
        ),

        // Moderate depends on Advanced (Simplification)
        TierView::ModerateTarget => (
            "moderate_target",
            "es",
            Some("advanced_target"),
            Some("simplify_segments_moderate"),
        ),

        // Basic Target depends on Moderate (Simplification)
        TierView::BasicTarget => (
            "basic_target",
            "es",
            Some("moderate_target"),
            Some("simplify_segments_basic"),
        ),

        // --- FIXED: Basic Base depends on Base (Simplification of the original English) ---
        TierView::BasicBase => (
            "basic_base",
            "en",
            Some("base"),
            Some("simplify_to_basic_english"),
        ),

        TierView::Simulation => unreachable!(),
    };
    ui.horizontal(|ui| {
        ui.heading(format!("Tier: {tier_id}"));
        if let Some(sentence) = state.get_current_sentence() {
            if let Some(tier) = sentence.get_tier(tier_id) {
                match tier.state {
                    TierState::Valid => ui.colored_label(egui::Color32::DARK_GREEN, "[Valid]"),
                    TierState::Dirty => {
                        ui.colored_label(egui::Color32::from_rgb(200, 100, 0), "[Dirty]")
                    }
                    TierState::Stale => {
                        ui.colored_label(egui::Color32::from_rgb(255, 140, 0), "[Stale]")
                    }
                    TierState::Broken => ui.colored_label(egui::Color32::RED, "[BROKEN]"),
                };
            } else {
                ui.colored_label(egui::Color32::GRAY, "[Missing]");
            }
        }
    });
    ui.separator();

    let bridge_handle = state.bridge.clone();
    let llm_handle = state.llm.clone();
    let prompts_handle = state.prompts.clone();
    let logger_handle = state.logger.clone();
    let (proj_base, proj_target) = state.project_languages.clone();
    let cur_selected_idx = state.selected_sentence_idx;
    let cur_batch_size = state.llm_batch_settings.simplify;

    let mut parent_text_opt = None;
    if let Some(p_id) = parent_tier_id {
        if let Some(sentence) = state.get_current_sentence() {
            if let Some(parent_tier) = sentence.get_tier(p_id) {
                parent_text_opt = Some(parent_tier.full_text());
            }
        }
    }

    let mut status_update: Option<String> = None;

    // We may need to start an LLM job but avoid mutably borrowing `state` while
    // holding `sentence_mut`. Collect a pending job to apply after the borrow.
    let mut after_job: Option<(
        std::sync::mpsc::Receiver<Result<Vec<(usize, String, String, String)>, String>>,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
        Vec<(usize, String, String)>,
        usize,
        String,
    )> = None;

    let mut current_tier_text = String::new();
    let mut current_tier_state = TierState::Valid;
    let mut current_tier_exists = false;
    let mut s_id = String::new();
    let mut pending_action: Option<&str> = None;
    let mut pending_text_update: Option<String> = None;

    if let Some(sentence_mut) = state.get_current_sentence_mut() {
        s_id = sentence_mut.id.clone();
        if let Some(tier) = sentence_mut.get_tier(tier_id) {
            current_tier_text = tier.full_text();
            current_tier_state = tier.state;
            current_tier_exists = true;
        } else {
            current_tier_exists = false;
        }

        if current_tier_exists {
            ui.horizontal(|ui| {
                if current_tier_state == TierState::Dirty {
                    ui.label(egui::RichText::new("Changes detected.").italics());
                    if ui.button("⟳ Apply, Re-tokenize & Validate").clicked() {
                        pending_action = Some("Retokenize");
                    }
                } else {
                    if parent_tier_id.is_some()
                        && prompt_name.is_some()
                        && parent_text_opt.is_some()
                    {
                        if ui.button("🧠 Regenerate via LLM").clicked() {
                            pending_action = Some("Regenerate");
                        }
                    }
                    if ui.button("⟳ Force Re-tokenize").clicked() {
                        pending_action = Some("Retokenize");
                    }
                }
                
                // Allow regenerating even if dirty
                if current_tier_state == TierState::Dirty 
                    && parent_tier_id.is_some()
                    && prompt_name.is_some() 
                    && parent_text_opt.is_some() 
                {
                     if ui.button("🧠 Regenerate (Discard Edits)").clicked() {
                        pending_action = Some("Regenerate");
                     }
                }
            });

            if current_tier_state != TierState::Valid {
                ui.separator();
            }

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let response = ui.add(
                        egui::TextEdit::multiline(&mut current_tier_text)
                            .font(egui::TextStyle::Body)
                            .desired_width(f32::INFINITY)
                            .id_source(format!("editor_{tier_id}")),
                    );
                    if response.changed() {
                        pending_text_update = Some(current_tier_text.clone());
                    }
                });
        } else {
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("Tier does not exist.").weak());
                    if let (Some(p_text), Some(_)) = (&parent_text_opt, prompt_name) {
                        if !p_text.trim().is_empty()
                            && ui.button("🧠 Generate from Parent").clicked()
                        {
                            pending_action = Some("Regenerate");
                        }
                    } else if parent_tier_id.is_none() && ui.button("Create Empty Tier").clicked() {
                        pending_text_update = Some(String::new());
                    }
                });
            });
        }

        // Apply local text updates immediately (re-tokenizing is deferred if needed)
        if let Some(new_text) = pending_text_update {
            sentence_mut.update_tier_text(tier_id, new_text);
        }
        
        // Handle "Retokenize" immediately
        if let Some("Retokenize") = pending_action {
             state.pending_terminal_command = Some(format!("approve edits {} {}", cur_selected_idx, tier_id));
             // Clear pending action so we don't double process
             pending_action = None; 
        }
    } // End of mutable borrow

    // 2. Process Actions that require broader state access (LLM Job)
    if let Some("Regenerate") = pending_action {
        if prompts_handle.is_none() || llm_handle.is_none() || logger_handle.is_none() {
            status_update = Some("LLM services not configured.".to_string());
        } else if parent_tier_id.is_none() || prompt_name.is_none() {
            status_update = Some("No parent tier or prompt defined.".to_string());
        } else {
            let prompts = prompts_handle.unwrap();
            let llm = llm_handle.unwrap();
            let logger = logger_handle.unwrap();
            let (base_code, target_code) = (proj_base.clone(), proj_target.clone());
            let prompt_name_str = prompt_name.unwrap().to_string();
            let p_id = parent_tier_id.unwrap();

            // CONTEXT WINDOW LOGIC:
            // Use the User's Batch Setting for context size (typically 10).
            // This ensures we have enough context to prevent "completion mode".
            // If user sets batch=1, we warn them but obey.
            let context_window_size = cur_batch_size.max(1);
            
            // Check if user is using a very small batch size which risks hallucinations
            if context_window_size < 3 {
                 // We can't easily pop a warning here without interrupting the flow, 
                 // but we can log it or show it in the spinner.
                 status_update = Some("Warning: Batch size < 3 increases hallucination risk.".to_string());
            }

            let start_idx = cur_selected_idx;
            let end_idx = std::cmp::min(cur_selected_idx + context_window_size, state.document.len());
            
            let mut items = Vec::new();
            
            for i in start_idx..end_idx {
                if let Some(s) = state.document.get(i) {
                    if let Some(p_tier) = s.get_tier(p_id) {
                        items.push((i, s.id.clone(), p_tier.full_text()));
                    }
                }
            }

            if items.is_empty() {
                 status_update = Some("No source text found in window.".to_string());
            } else {
                // Snapshot current target tier text for possible revert (we only care about the *current* sentence for revert)
                let prior = if current_tier_exists { current_tier_text } else { String::new() };
                let backup = vec![(cur_selected_idx, tier_id.to_string(), prior)];

                let batch = cur_batch_size; // Pass through to worker
                let model = DEFAULT_MODEL.to_string();

                // If we grabbed collateral sentences, the worker will return them all.
                // We'll filter them in the main app loop or handle them via the new collateral confirmation logic.

                let (rx, cancel_flag) = spawn_llm_job(
                    prompts,
                    llm,
                    logger,
                    base_code,
                    target_code,
                    prompt_name_str.clone(),
                    tier_id.to_string(),
                    items,
                    batch,
                    model,
                    None,
                    false, // not segment-level
                );

                after_job = Some((rx, cancel_flag, backup, end_idx - start_idx, prompt_name_str.clone()));
                if status_update.is_none() {
                    status_update = Some(format!("LLM regeneration started (Window: {}).", end_idx - start_idx));
                }
            }
        }
    }

    // 3. Apply any deferred LLM job into the AppState
    if let Some((rx, cancel_flag, backup, total, prompt_name_str)) = after_job {
        state.llm_results_receiver = Some(rx);
        state.llm_cancel_flag = Some(cancel_flag);
        state.llm_job_backup = backup;
        state.llm_job_total = total;
        state.llm_job_done = 0;
        state.last_log = format!("LLM job '{}' started.", prompt_name_str);
    }
    
    // 4. Update Status if needed
    if let Some(msg) = status_update {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(msg);
        });
    }
}
