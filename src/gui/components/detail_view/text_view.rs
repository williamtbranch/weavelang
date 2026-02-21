// src/gui/components/detail_view/text_view.rs

use eframe::egui;
use crate::domain::tier::TierState;
use crate::gui::state::{TierView, AppState};
use crate::gui::preview;
use crate::domain::token_stream::{Token, TokenStream};
use regex::Regex;

const DEFAULT_MODEL: &str = "claude-3-haiku-20240307";

fn extract_translation_from_response(raw_response: &str, target_id: &str) -> String {
    let num_str: String = target_id.chars().filter(|c| c.is_ascii_digit()).collect();
    
    let pattern_str = format!(r"(?mi)^(?:id\s*)?S0*{}\s*[:.]\s*(.*)$", num_str);
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

    let non_empty_lines: Vec<&str> = raw_response.lines().filter(|l| !l.trim().is_empty()).collect();
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
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.add(egui::TextEdit::multiline(&mut text).font(egui::TextStyle::Body).desired_width(f32::INFINITY).frame(false));
            });
        } else {
            ui.label("No sentence selected.");
        }
        return; 
    }

    let (tier_id, lang_code, parent_tier_id, prompt_name) = match view {
        TierView::Base => ("base", "en", None, None), 
        TierView::AdvancedTarget => ("advanced_target", "es", Some("base"), Some("translate_text")),
        TierView::ModerateTarget => ("moderate_target", "es", Some("advanced_target"), Some("simplify_segments_moderate")),
        TierView::BasicTarget => ("basic_target", "es", Some("moderate_target"), Some("simplify_segments_basic")),
        TierView::BasicBase => ("basic_base", "en", Some("basic_target"), Some("translate_text_basic")),
        TierView::Simulation => unreachable!(),
    };

    ui.horizontal(|ui| {
        ui.heading(format!("Tier: {}", tier_id));
        if let Some(sentence) = state.get_current_sentence() {
            if let Some(tier) = sentence.get_tier(tier_id) {
                match tier.state {
                    TierState::Valid => ui.colored_label(egui::Color32::DARK_GREEN, "[Valid]"),
                    TierState::Dirty => ui.colored_label(egui::Color32::from_rgb(200, 100, 0), "[Dirty]"),
                    TierState::Stale => ui.colored_label(egui::Color32::from_rgb(255, 140, 0), "[Stale]"),
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

    let mut parent_text_opt = None;
    if let Some(p_id) = parent_tier_id {
        if let Some(sentence) = state.get_current_sentence() {
            if let Some(parent_tier) = sentence.get_tier(p_id) {
                parent_text_opt = Some(parent_tier.full_text());
            }
        }
    }

    let mut status_update = None;

    if let Some(sentence_mut) = state.get_current_sentence_mut() {
        let s_id = sentence_mut.id.clone();
        let (mut current_text, tier_state, tier_exists) = if let Some(tier) = sentence_mut.get_tier(tier_id) {
            (tier.full_text(), tier.state, true)
        } else {
            (String::new(), TierState::Valid, false) 
        };

        let mut pending_action = None; 
        let mut pending_text_update = None;

        if tier_exists {
            ui.horizontal(|ui| {
                if tier_state == TierState::Dirty {
                    ui.label(egui::RichText::new("Changes detected.").italics());
                    if ui.button("⟳ Apply & Re-tokenize").clicked() { pending_action = Some("Retokenize"); }
                } else {
                    if parent_tier_id.is_some() && prompt_name.is_some() && parent_text_opt.is_some() {
                        if ui.button("🧠 Regenerate via LLM").clicked() { pending_action = Some("Regenerate"); }
                    }
                    if ui.button("⟳ Force Re-tokenize").clicked() { pending_action = Some("Retokenize"); }
                }
            });
            if tier_state != TierState::Valid { ui.separator(); }

            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let response = ui.add(egui::TextEdit::multiline(&mut current_text).font(egui::TextStyle::Body).desired_width(f32::INFINITY).id_source(format!("editor_{}", tier_id)));
                if response.changed() { pending_text_update = Some(current_text.clone()); }
            });
        } else {
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("Tier does not exist.").weak());
                    if let (Some(p_text), Some(_)) = (&parent_text_opt, prompt_name) {
                        if !p_text.trim().is_empty() {
                            if ui.button("🧠 Generate from Parent").clicked() { pending_action = Some("Regenerate"); }
                        }
                    } else if parent_tier_id.is_none() {
                        if ui.button("Create Empty Tier").clicked() { pending_text_update = Some(String::new()); }
                    }
                });
            });
        }

        if let Some(new_text) = pending_text_update {
            sentence_mut.update_tier_text(tier_id, new_text);
        }

        if pending_action == Some("Regenerate") {
            if let (Some(llm), Some(pm), Some(p_text), Some(p_name), Some(logger)) = (&llm_handle, &prompts_handle, &parent_text_opt, prompt_name, &logger_handle) {
                status_update = Some(format!("Loading Prompt '{}'...", p_name));
                match pm.get_prompt(p_name, &proj_base, &proj_target) {
                    Ok(sys_prompt) => {
                        let formatted_user_prompt = format!("{}: {}", s_id, p_text);
                        status_update = Some("Querying Claude...".to_string());
                        match llm.complete(DEFAULT_MODEL, &sys_prompt, &formatted_user_prompt) {
                            Ok(raw_response) => {
                                let clean_text = extract_translation_from_response(&raw_response, &s_id);
                                logger.log_interaction(&format!("Regen: {}", tier_id), &sys_prompt, &formatted_user_prompt, &raw_response);
                                sentence_mut.update_tier_text(tier_id, clean_text.clone());
                                pending_action = Some("Retokenize");
                                current_text = clean_text; 
                                status_update = Some("Regeneration Successful.".to_string());
                            },
                            Err(e) => status_update = Some(format!("LLM Error: {}", e)),
                        }
                    },
                    Err(e) => status_update = Some(format!("Prompt Error: {}", e)),
                }
            }
        }

        if pending_action == Some("Retokenize") {
            if let Some(bridge) = &bridge_handle {
                let text_to_process = if let Some(t) = sentence_mut.get_tier(tier_id) { t.full_text() } else { current_text };
                match bridge.tokenize(&text_to_process, lang_code) {
                    Ok(raw_spacy_tokens) => {
                        if let Some(tier) = sentence_mut.get_tier_mut(tier_id) {
                            let new_stream = TokenStream::from_raw_spacy(raw_spacy_tokens, &text_to_process);
                            let mut all_lemmas = Vec::new();
                            for token in new_stream.tokens() {
                                if let Token::Word(w) = token { all_lemmas.extend(w.lemmas.clone()); }
                            }
                            tier.segments.clear();
                            tier.segments.push(crate::domain::segment::Segment::from_stream("S1".to_string(), new_stream, vec![]));
                            tier.lemmas = all_lemmas;
                            tier.state = TierState::Valid;
                            if status_update.is_none() { status_update = Some("Tokens Updated.".to_string()); }
                        }
                    },
                    Err(e) => {
                        status_update = Some(format!("Bridge Error: {}", e));
                        if let Some(tier) = sentence_mut.get_tier_mut(tier_id) { tier.state = TierState::Broken; }
                    }
                }
            }
        }
    }

    if let Some(msg) = status_update {
        state.last_log = msg;
    }
}