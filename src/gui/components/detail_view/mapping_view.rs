// src/gui/components/detail_view/mapping_view.rs

use crate::gui::state::{DetailView, AppState};
use eframe::egui;
use std::collections::HashMap;

// Signature changed: No &Sentence argument. state is passed directly.
pub fn render(ui: &mut egui::Ui, mode: DetailView, state: &mut AppState) {
    // 1. Determine Context
    let (source_tier_id, target_tier_id, prompt_name) = match mode {
        DetailView::MappingDiglot => ("basic_base", "basic_target", "generate_diglot_map"),
        DetailView::MappingInverse => ("basic_target", "basic_base", "generate_inverse_phrase_map"),
        _ => return,
    };

    ui.horizontal(|ui| {
        ui.heading(format!("Mapping: {} -> {}", source_tier_id, target_tier_id));
    });
    ui.separator();

    // 2. Clone handles (Safe to do before borrowing state)
    let llm_handle = state.llm.clone();
    let prompts_handle = state.prompts.clone();
    let logger_handle = state.logger.clone();
    let (proj_base, proj_target) = state.project_languages.clone();

    // 3. Mutable Interaction (Generate Button)
    // We access state mutably here to read text and potentially write logs/status
    if let Some(sentence_mut) = state.get_current_sentence_mut() {
        
        let source_text = sentence_mut.get_tier(source_tier_id).map(|t| t.full_text()).unwrap_or_default();
        let target_text = sentence_mut.get_tier(target_tier_id).map(|t| t.full_text()).unwrap_or_default();

        let tiers_exist = !source_text.is_empty() && !target_text.is_empty();

        if tiers_exist {
            if ui.button("🧠 Generate Mapping").on_hover_text("Sends source tier to LLM").clicked() {
                
                if let (Some(llm), Some(pm), Some(logger)) = (&llm_handle, &prompts_handle, &logger_handle) {
                    
                    // A. Load Prompt
                    let sys_prompt_res = pm.get_prompt(prompt_name, &proj_base, &proj_target);
                    
                    match sys_prompt_res {
                        Ok(sys_prompt) => {
                            // B. Call LLM
                            match llm.complete("claude-3-haiku-20240307", &sys_prompt, &source_text) {
                                Ok(raw_response) => {
                                    // C. Log it
                                    logger.log_interaction(
                                        &format!("Generate Mapping ({})", prompt_name),
                                        &sys_prompt,
                                        &source_text,
                                        &raw_response
                                    );
                                    println!("[Studio] Mapping generated. Check 'studio_llm.log'.");
                                },
                                Err(e) => eprintln!("[Studio] LLM Error: {}", e),
                            }
                        },
                        Err(e) => eprintln!("[Studio] Prompt Error: {}", e),
                    }
                } else {
                    eprintln!("[Studio] Services missing.");
                }
            }
        } else {
            ui.colored_label(egui::Color32::RED, "Source or Target Tier missing.");
        }
    }

    ui.separator();

    // 4. Render Existing Mapping Table (Read-Only)
    // We re-acquire the sentence (immutably) for rendering the grid
    if let Some(sentence) = state.get_current_sentence() {
        let source_tier = match sentence.get_tier(source_tier_id) {
            Some(t) => t,
            None => { ui.label("Source Tier not found."); return; }
        };

        let mapping_lookup: HashMap<_, _> = sentence
            .mappings()
            .iter()
            .find(|m| m.from_tier_id == source_tier_id && m.to_tier_id == target_tier_id)
            .map(|tier_mapping| {
                tier_mapping.entries.iter().map(|entry| (entry.source_word_id, entry)).collect()
            })
            .unwrap_or_default();

        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("mapping_grid").striped(true).min_col_width(100.0).show(ui, |ui| {
                ui.strong("Source Token");
                ui.strong("Mapped Target");
                ui.end_row();

                for segment in &source_tier.segments {
                    for token in segment.stream.tokens() {
                        if let crate::domain::token_stream::Token::Word(word_data) = token {
                            ui.label(&word_data.text);
                            if let Some(entry) = mapping_lookup.get(&word_data.id) {
                                let color = if entry.is_viable { egui::Color32::DARK_GREEN } else { egui::Color32::GRAY };
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