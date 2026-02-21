// src/gui/components/top_bar.rs

use eframe::egui;
use crate::gui::state::{AppState, TierView};

pub fn render(ui: &mut egui::Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        ui.strong("Navigator View:");
        selectable_view_tab(ui, state, "Base", TierView::Base);
        selectable_view_tab(ui, state, "Advanced", TierView::AdvancedTarget);
        selectable_view_tab(ui, state, "Moderate", TierView::ModerateTarget);
        selectable_view_tab(ui, state, "Basic T", TierView::BasicTarget);
        selectable_view_tab(ui, state, "Basic B", TierView::BasicBase);
        
        ui.separator();
        
        if ui.selectable_label(state.left_view == TierView::Simulation, "Simulation").clicked() {
            state.left_view = TierView::Simulation;
        }
        
        ui.separator();

        // --- Bridge Test ---
        if ui.button("🔌 Test Bridge").on_hover_text("Check Python Connection").clicked() {
            if let Some(bridge) = &state.bridge {
                state.last_log = "Sending to Python...".to_string();
                match bridge.tokenize("Hola mundo.", "es") {
                    Ok(tokens) => {
                        state.last_log = format!("✅ Bridge: Success! {} tokens.", tokens.len());
                    },
                    Err(e) => state.last_log = format!("❌ Bridge Error: {}", e),
                }
            } else {
                state.last_log = "⚠️ Bridge Not Loaded".to_string();
            }
        }

        // --- NEW: LLM Test ---
        if ui.button("🧠 Test LLM").on_hover_text("Ping Anthropic API (Costs tokens!)").clicked() {
            if let Some(llm) = &state.llm {
                state.last_log = "Querying Claude... (UI may freeze briefly)".to_string();
                
                // Force a repaint so the "Querying..." message shows up before the blocking call
                ui.ctx().request_repaint(); 

                // We use Haiku for a cheap, fast test
                match llm.complete(
                    "claude-3-haiku-20240307", 
                    "You are a test interface.", 
                    "Say 'Hello from Rust' and nothing else."
                ) {
                    Ok(response) => state.last_log = format!("✅ LLM: {}", response),
                    Err(e) => state.last_log = format!("❌ LLM Error: {}", e),
                }
            } else {
                state.last_log = "⚠️ LLM Service Not Available (Check .env)".to_string();
            }
        }
        
        ui.separator();
        ui.label(&state.last_log);
    });
}

fn selectable_view_tab(ui: &mut egui::Ui, state: &mut AppState, label: &str, view: TierView) {
    if ui.selectable_label(state.left_view == view, label).clicked() {
        state.left_view = view;
    }
}