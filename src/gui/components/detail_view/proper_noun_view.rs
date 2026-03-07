// src/gui/components/detail_view/proper_noun_view.rs

use crate::app::state::AppState;
use eframe::egui;

/// Render the Proper Noun Lemmas editor tab.
///
/// Shows the current sentence's `proper_noun_lemmas` list with the ability
/// to add new lemmas or remove existing ones.  All mutations go through
/// terminal commands (`add pn_lemma`, `rm pn_lemma`) rather than touching
/// the document directly.
pub fn render(ui: &mut egui::Ui, state: &mut AppState) {
    let Some(sentence) = state.get_current_sentence() else {
        ui.label("No sentence selected.");
        return;
    };

    // 1-based sentence number for terminal commands
    let sentence_num = state.selected_sentence_idx + 1;
    let sentence_id = sentence.id.clone();
    let lemmas: Vec<String> = sentence.proper_noun_lemmas.clone();

    ui.heading(format!("Proper Noun Lemmas — {}", sentence_id));
    ui.add_space(4.0);

    // --- Add lemma row ---
    ui.horizontal(|ui| {
        ui.label("Add:");
        let response = ui.text_edit_singleline(&mut state.pn_lemma_input);
        let enter_pressed = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (ui.button("+").clicked() || enter_pressed) && !state.pn_lemma_input.trim().is_empty() {
            let lemma = state.pn_lemma_input.trim().to_string();
            state.pending_terminal_command = Some(format!(
                "add pn_lemma {} {}", sentence_num, lemma
            ));
            state.pn_lemma_input.clear();
        }
    });

    ui.add_space(6.0);
    ui.separator();

    // --- Lemma list with remove buttons ---
    if lemmas.is_empty() {
        ui.label("(none)");
    } else {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for lemma in &lemmas {
                    ui.horizontal(|ui| {
                        if ui.small_button("✕").clicked() {
                            state.pending_terminal_command = Some(format!(
                                "rm pn_lemma {} {}", sentence_num, lemma
                            ));
                        }
                        ui.label(lemma);
                    });
                }
            });
    }
}
