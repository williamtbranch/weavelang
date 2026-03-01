// src/gui/components/detail_view/mapping_view.rs

use crate::app::state::{AppState, DetailView};
use crate::domain::mapping_logic::apply_llm_mapping;
use crate::services::llm_worker::spawn_llm_job;
use eframe::egui;
use std::collections::HashMap;

const DEFAULT_MODEL: &str = "claude-3-haiku-20240307";

// Signature changed: No &Sentence argument. state is passed directly.
pub fn render(ui: &mut egui::Ui, mode: DetailView, state: &mut AppState) {
    // 1. Determine Context
    let (source_tier_id, target_tier_id, prompt_name) = match mode {
        DetailView::MappingDiglot => ("basic_base", "basic_target", "generate_diglot_map"),
        DetailView::MappingInverse => ("basic_target", "basic_base", "generate_inverse_phrase_map"),
        _ => return,
    };

    ui.horizontal(|ui| {
        ui.heading(format!("Mapping: {source_tier_id} -> {target_tier_id}"));
    });
    ui.separator();

    // 2. Clone handles (Safe to do before borrowing state)
    let llm_handle = state.llm.clone();
    let prompts_handle = state.prompts.clone();
    let logger_handle = state.logger.clone();
    let (proj_base, proj_target) = state.project_languages.clone();
    let cur_selected_idx = state.selected_sentence_idx;
    let cur_batch_size = state.llm_batch_settings.mapping; // Use mapping batch size

    let mut pending_action: Option<&str> = None;
    let mut after_job: Option<(
        std::sync::mpsc::Receiver<Result<Vec<(usize, String, String, String)>, String>>,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
        Vec<(usize, String, String)>,
        usize,
        String,
    )> = None;
    let mut status_update: Option<String> = None;

    // 3. Mutable Interaction (Generate Button)
    if let Some(sentence_mut) = state.get_current_sentence_mut() {
        let s_id = sentence_mut.id.clone();
        let source_text = sentence_mut
            .get_tier(source_tier_id)
            .map(|t| t.full_text())
            .unwrap_or_default();
        
        // Relaxed check: Only source text is strictly required to generate a mapping from source.
        // If target is missing, the mapping will generate the target phrases implicitly.
        let tiers_exist = !source_text.is_empty();

        if tiers_exist {
            if ui
                .button("🧠 Generate Mapping")
                .on_hover_text("Sends source tier to LLM")
                .clicked()
            {
                pending_action = Some("Generate");
            }
        } else {
            ui.colored_label(egui::Color32::RED, "Source Tier missing.");
        }
    } // End mutable borrow

    // 4. Handle Deferred Generation (outside borrow)
    if let Some("Generate") = pending_action {
         if let (Some(pm), Some(llm), Some(logger)) = (&prompts_handle, &llm_handle, &logger_handle) {
            let context_window_size = cur_batch_size.max(1);
            let start_idx = cur_selected_idx;
            let end_idx = std::cmp::min(start_idx + context_window_size, state.document.len());
            
            let mut items = Vec::new();
            for i in start_idx..end_idx {
                if let Some(s) = state.document.get(i) {
                     if let Some(t) = s.get_tier(source_tier_id) {
                         items.push((i, s.id.clone(), t.full_text()));
                     }
                }
            }
            
            if !items.is_empty() {
                // Determine target tier ID for the RESULT of the job.
                // For mapping jobs, the "result" text is the mapping string (S1: phrase -> phrase).
                // But `spawn_llm_job` puts the result into `tier_id` parameter of the result tuple.
                // We want the app logic to know this is a MAPPING result, not a tier text update.
                // Currently `app.rs` interprets results as "Update Tier Text".
                // We need a way to signal "This is a Mapping Result".
                // Hack: Use a special prefix or just use the prompt name as the ID and handle it in `app.rs`?
                
                // Wait, `app.rs` calls `sent.update_tier_text`. This expects plain text for a tier.
                // Mappings are different. `apply_llm_mapping` parses the text and adds mapping objects.
                
                // We need to modify `app.rs` to handle mapping results!
                // Or we can use a special "tier_id" like "MAPPING:basic_base->basic_target".
                
                let job_tier_id = format!("MAPPING:{}:{}", source_tier_id, target_tier_id);
                
                let (rx, cancel_flag) = spawn_llm_job(
                    pm.clone(),
                    llm.clone(),
                    logger.clone(),
                    proj_base.clone(),
                    proj_target.clone(),
                    prompt_name.to_string(),
                    job_tier_id, // Pass this special ID
                    items,
                    cur_batch_size,
                    DEFAULT_MODEL.to_string(),
                    None,
                    false, // not segment-level
                );
                
                // We don't have a simple text backup for mappings, so empty backup
                let backup = vec![]; 
                
                after_job = Some((rx, cancel_flag, backup, end_idx - start_idx, prompt_name.to_string()));
                status_update = Some("Mapping generation started.".to_string());
            }
         }
    }

    // 5. Apply Job to State
    if let Some((rx, cancel_flag, backup, total, prompt_name_str)) = after_job {
        state.llm_results_receiver = Some(rx);
        state.llm_cancel_flag = Some(cancel_flag);
        state.llm_job_backup = backup;
        state.llm_job_total = total;
        state.llm_job_done = 0;
        state.last_log = format!("LLM job '{}' started.", prompt_name_str);
    }
    
    if let Some(msg) = status_update {
         ui.label(msg);
    }

    ui.separator();

    // 4. Render Existing Mapping Table (Read-Only)
    // We re-acquire the sentence (immutably) for rendering the grid
    if let Some(sentence) = state.get_current_sentence() {
        let source_tier = match sentence.get_tier(source_tier_id) {
            Some(t) => t,
            None => {
                ui.label("Source Tier not found.");
                return;
            }
        };

        let mapping_lookup: HashMap<_, _> = sentence
            .mappings()
            .iter()
            .find(|m| m.from_tier_id == source_tier_id && m.to_tier_id == target_tier_id)
            .map(|tier_mapping| {
                tier_mapping

                    .entries
                    .iter()
                    .map(|entry| (entry.source_word_id, entry))
                    .collect()
            })
            .unwrap_or_default();

        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("mapping_grid")
                .striped(true)
                .min_col_width(100.0)
                .show(ui, |ui| {
                    ui.strong("Source Token");
                    ui.strong("Mapped Target");
                    ui.end_row();

                    for segment in &source_tier.segments {
                        for token in segment.stream.tokens() {
                            if let crate::domain::token_stream::Token::Word(word_data) = token {
                                ui.label(&word_data.text);
                                if let Some(entry) = mapping_lookup.get(&word_data.id) {
                                    let color = if entry.is_viable {
                                        egui::Color32::DARK_GREEN
                                    } else {
                                        egui::Color32::GRAY
                                    };
                                    ui.colored_label(color, &entry.target_text);
                                } else {
                                    ui.weak("---");
                                }
                                ui.end_row();
                            }
                        }
                    }
                });
        });
    }
}
