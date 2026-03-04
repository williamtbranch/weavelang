// src/gui/app.rs

use eframe::{egui, App, Frame};
use std::path::PathBuf;

use crate::gui::components;
use crate::app::state::AppState;
use crate::app::terminal::apply_llm_result;
use crate::services::llm_client::LlmService;
use crate::services::llm_logger::LlmLogger;
use crate::services::prompt_manager::PromptManager;
use crate::services::python_bridge::BridgeService;
use crate::services::tier_processor::lang_for_tier;
use std::sync::mpsc::TryRecvError;
use std::cmp::min;

pub struct WeaveLangApp {
    state: AppState,
    current_file_path: Option<PathBuf>,
    status_message: String,
    terminal_history: Vec<String>,
    terminal_input: String,
}

impl WeaveLangApp {
    pub fn new(
        _cc: &eframe::CreationContext<'_>,
        bridge: Option<BridgeService>,
        llm: Option<LlmService>,
        prompts: Option<PromptManager>,
        logger: Option<LlmLogger>,
    ) -> Self {
        let mut state = AppState::default();
        state.bridge = bridge;
        state.llm = llm;
        state.prompts = prompts;
        state.logger = logger;

        let mut status = Vec::new();
        if state.bridge.is_some() {
            status.push("Bridge: OK");
        } else {
            status.push("Bridge: OFF");
        }
        if state.llm.is_some() {
            status.push("LLM: OK");
        } else {
            status.push("LLM: OFF");
        }

        Self {
            state,
            current_file_path: None,
            status_message: format!("Ready. [{}]", status.join(", ")),
            terminal_history: vec!["WeaveLang Terminal initialized.".to_string()],
            terminal_input: String::new(),
        }
    }

    fn execute_terminal_command(&mut self, cmd: &str) {
        self.terminal_history.push(format!("> {}", cmd));
        
        // Temporarily take state to run engine
        let mut engine = crate::app::engine::Engine::new(std::mem::take(&mut self.state));
        engine.current_file_path = self.current_file_path.clone();
        
        match crate::app::terminal::run_terminal_command(&mut engine, cmd) {
            Ok(Some(output)) => {
                for line in output.lines() {
                    self.terminal_history.push(line.to_string());
                }
            }
            Ok(None) => {
                self.terminal_history.push("Exit command ignored in GUI.".to_string());
            }
            Err(e) => {
                self.terminal_history.push(format!("Error: {}", e));
            }
        }
        
        self.state = engine.state;
        self.current_file_path = engine.current_file_path;
    }

    fn render_terminal(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.heading("Terminal");
            ui.separator();
            
            // Scrollable history area
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .max_height(ui.available_height() - 30.0)
                .show(ui, |ui| {
                    for line in &self.terminal_history {
                        ui.label(egui::RichText::new(line).family(egui::FontFamily::Monospace));
                    }
                });
                
            ui.separator();
            
            // Input area
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(">").family(egui::FontFamily::Monospace));
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.terminal_input)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(ui.available_width() - 50.0)
                );
                
                if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    let cmd = self.terminal_input.clone();
                    if !cmd.trim().is_empty() {
                        self.execute_terminal_command(&cmd);
                        self.terminal_input.clear();
                        response.request_focus();
                    }
                }
                
                if ui.button("Run").clicked() {
                    let cmd = self.terminal_input.clone();
                    if !cmd.trim().is_empty() {
                        self.execute_terminal_command(&cmd);
                        self.terminal_input.clear();
                        response.request_focus();
                    }
                }
            });
        });
    }

    fn render_menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Import JSON...").clicked() {
                    self.import_json();
                    ui.close_menu();
                }
                if ui.button("Import Source Text...").clicked() {
                    self.import_source_text();
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Open .wvl...").clicked() {
                    self.open_binary();
                    ui.close_menu();
                }
                if ui.button("Save .wvl").clicked() {
                    self.save_binary();
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Export JSON...").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("JSON", &["json"])
                        .save_file()
                    {
                        let cmd = format!("export json {}", path.to_string_lossy());
                        self.execute_terminal_command(&cmd);
                    }
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Debug Dump...").clicked() {
                    self.state.debug_dump_start = self.state.selected_sentence_idx;
                    self.state.debug_dump_end   = self.state.selected_sentence_idx;
                    self.state.debug_dump_path  = String::new();
                    self.state.show_debug_dump  = true;
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Exit").clicked() {
                    std::process::exit(0);
                }
            });

            ui.menu_button("Pipeline", |ui| {
                const PIPELINE_STAGES: &[(&str, &str)] = &[
                    ("GenerateBasicBase",        "Generate Basic Base..."),
                    ("GenerateAdvancedTarget",   "Generate Advanced Translation..."),
                    ("GenerateModerateTarget",   "Generate Moderate Target..."),
                    ("GenerateBasicTarget",      "Generate Basic Target..."),
                ];
                const MAPPING_STAGES: &[(&str, &str)] = &[
                    ("GeneratePhraseMap",        "Generate Phrase Map..."),
                    ("GenerateInversePhraseMap", "Generate Inverse Phrase Map..."),
                ];

                for (stage_key, label) in PIPELINE_STAGES {
                    if ui.button(*label).clicked() {
                        let start = self.state.selected_sentence_idx;
                        let end   = min(start + self.state.llm_batch_settings.simplify.saturating_sub(1),
                                        self.state.document.len().saturating_sub(1));
                        self.state.llm_run_prompt_name = stage_key.to_string();
                        self.state.llm_run_start = start;
                        self.state.llm_run_end   = end;
                        self.state.show_llm_run  = true;
                        ui.close_menu();
                    }
                }
                ui.separator();
                for (stage_key, label) in MAPPING_STAGES {
                    if ui.button(*label).clicked() {
                        let cur = self.state.selected_sentence_idx;
                        self.state.llm_run_prompt_name = stage_key.to_string();
                        self.state.llm_run_start = cur;
                        self.state.llm_run_end   = cur;
                        self.state.show_llm_run  = true;
                        ui.close_menu();
                    }
                }
                ui.separator();
                if ui.button("LLM Settings...").clicked() {
                    self.state.show_llm_settings = true;
                    ui.close_menu();
                }
            });

            ui.menu_button("Tools", |ui| {
                if ui.button("Measure AVD...").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Text File", &["txt"])
                        .pick_file()
                    {
                        let cmd = format!("measure_avd {}", path.to_string_lossy());
                        self.execute_terminal_command(&cmd);
                    }
                    ui.close_menu();
                }
                if ui.button("Measure User Score...").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Text File", &["txt"])
                        .pick_file()
                    {
                        let cmd = format!("measure_user_score {}", path.to_string_lossy());
                        self.execute_terminal_command(&cmd);
                    }
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Generate Weave (All)").clicked() {
                    self.execute_terminal_command("generate_weave all");
                    ui.close_menu();
                }
                if ui.button("Generate Weave (Level)...").clicked() {
                    // Emit as terminal command; user can type the level in the terminal
                    self.execute_terminal_command("generate_weave all");
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Import Level Map...").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Level Map", &["lm", "json"])
                        .pick_file()
                    {
                        let cmd = format!("import level_map {}", path.to_string_lossy());
                        self.execute_terminal_command(&cmd);
                    }
                    ui.close_menu();
                }
                if ui.button("Export Level Map...").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Level Map", &["lm"])
                        .save_file()
                    {
                        let cmd = format!("export level_map {}", path.to_string_lossy());
                        self.execute_terminal_command(&cmd);
                    }
                    ui.close_menu();
                }
            });

            ui.menu_button("Preferences", |ui| {
                if ui.button("LLM Settings...").clicked() {
                    self.state.show_llm_settings = true;
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Config List").clicked() {
                    self.execute_terminal_command("config list");
                    ui.close_menu();
                }
                if ui.button("Config Set...").clicked() {
                    self.state.config_set_key   = String::new();
                    self.state.config_set_value = String::new();
                    self.state.show_config_set  = true;
                    ui.close_menu();
                }
            });

            ui.separator();
            ui.label(format!("Status: {}", self.status_message));
        });
    }

    fn import_json(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("WeaveLang JSON", &["json"])
            .pick_file()
        {
            let cmd = format!("import json {}", path.to_string_lossy());
            self.execute_terminal_command(&cmd);
        }
    }

    fn import_source_text(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Text File", &["txt"])
            .pick_file()
        {
            let cmd = format!("import source {}", path.to_string_lossy());
            self.execute_terminal_command(&cmd);
        }
    }

    fn save_binary(&mut self) {
        let path_opt = if let Some(p) = &self.current_file_path {
            Some(p.clone())
        } else {
            rfd::FileDialog::new()
                .add_filter("WeaveLang Binary", &["wvl"])
                .save_file()
        };

        if let Some(path) = path_opt {
            let cmd = format!("save project {}", path.to_string_lossy());
            self.execute_terminal_command(&cmd);
        }
    }

    // --- FIX IS HERE ---
    fn open_binary(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("WeaveLang Binary", &["wvl"])
            .pick_file()
        {
            let cmd = format!("load project {}", path.to_string_lossy());
            self.execute_terminal_command(&cmd);
        }
    }
}

impl App for WeaveLangApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        // Check for pending terminal commands from UI components
        if let Some(cmd) = self.state.pending_terminal_command.take() {
            self.execute_terminal_command(&cmd);
        }

        // Check for LLM results and apply them before rendering UI
        // Drain receiver messages without holding a borrow across potential assignment
        if self.state.llm_results_receiver.is_some() {
            let mut clear_receiver = false;
            loop {
                // Try to get a message if receiver still exists
                let msg = match self.state.llm_results_receiver.as_ref() {
                    None => break,
                    Some(rx) => match rx.try_recv() {
                        Ok(v) => Ok(v),
                        Err(e) => Err(e),
                    },
                };

                match msg {
                    Ok(Ok(results)) => {
                        let mut applied = 0usize;
                        let mut last_applied_text: Option<String> = None;
                        
                        // NEW: Check if this is a "Regeneration" job (single-click) vs a "Bulk" job.
                        // Currently, we don't distinguish explicitly in the receiver, but "Regeneration" 
                        // jobs have total=batch_size (or small N) and start at a specific index.
                        // However, simpler heuristic:
                        // If we are getting results for indices OTHER than the currently selected one, 
                        // and we are NOT in a bulk run (llm_run_end > llm_run_start + batch), treat as collateral?
                        
                        // Actually, let's just use the `pending_collateral_updates` mechanism for ANY update 
                        // that isn't the primary selected sentence IF we are not in "Run All" mode.
                        // "Run All" mode is indicated by `state.show_llm_run` active? No, that's just the dialog.
                        // We can check `state.llm_job_total`.
                        
                        // If `llm_job_total` is small (e.g. <= batch size) it's likely a single-click regen.
                        // If it's large, it's a bulk run where we WANT auto-apply.
                        
                        let is_bulk_run = self.state.llm_job_total > self.state.llm_batch_settings.simplify.max(self.state.llm_batch_settings.translate).max(10);
                        let selected_idx = self.state.selected_sentence_idx;
                        let num_results = results.len();

                        let (base_lang, target_lang) = self.state.project_languages.clone();
                        let bridge_ref = self.state.bridge.as_ref();

                        for (idx, s_id, tier_id, text) in results {
                            if idx < self.state.document.len() {
                                // Always apply if it's the selected sentence OR if it's a bulk run
                                if is_bulk_run || idx == selected_idx {
                                    let lang = lang_for_tier(&tier_id, &base_lang, &target_lang);
                                    if let Some(sent) = self.state.document.get_mut(idx) {
                                        apply_llm_result(sent, &tier_id, &text, bridge_ref, &lang);
                                        if idx == selected_idx {
                                             if tier_id.starts_with("MAPPING:") {
                                                 last_applied_text = Some("Mapping Generated".to_string());
                                             } else {
                                                 last_applied_text = Some(sent.get_tier(&tier_id).map(|t| t.full_text()).unwrap_or_default());
                                             }
                                        }
                                        applied += 1;
                                    }
                                } else {
                                    // It's a collateral update (neighbor) in a non-bulk run
                                    self.state.pending_collateral_updates.push((idx, s_id, tier_id, text));
                                }
                            }
                        }
                        // FIX: Progress should count ALL items processed by the LLM, 
                        // whether applied immediately or parked for confirmation.
                        self.state.llm_job_done = self.state.llm_job_done.saturating_add(num_results);

                        // If we have collateral updates, show the confirmation dialog
                        if !self.state.pending_collateral_updates.is_empty() {
                            self.state.show_collateral_confirm = true;
                        }

                        // Update visible logs
                        if let Some(txt) = last_applied_text {
                            self.state.last_log = format!("LLM result: {}", txt);
                        } else if applied > 0 {
                            self.state.last_log = format!("LLM applied {} items.", applied);
                        }

                        if self.state.llm_job_total > 0 && self.state.llm_job_done >= self.state.llm_job_total {
                            self.status_message = "LLM job completed.".to_string();
                            // also reflect in last_log
                            if self.state.last_log.is_empty() {
                                self.state.last_log = "LLM job completed.".to_string();
                            }
                            clear_receiver = true;
                            break;
                        }
                        // continue draining
                    }
                    Ok(Err(err_str)) => {
                        self.status_message = format!("LLM job failed: {}", err_str);
                        self.state.last_log = format!("LLM job failed: {}", err_str);
                        clear_receiver = true;
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.status_message = "LLM results channel disconnected".to_string();
                        self.state.last_log = "LLM results channel disconnected".to_string();
                        clear_receiver = true;
                        break;
                    }
                }
            }

            if clear_receiver {
                self.state.llm_results_receiver = None;
            }
        }
        
        // --- Collateral Update Confirmation Dialog ---
        if self.state.show_collateral_confirm && !self.state.pending_collateral_updates.is_empty() {
             let mut open = true;
             
             let num_updates = self.state.pending_collateral_updates.len();
             let first_idx = self.state.pending_collateral_updates.first().map(|(i,_,_,_)| *i).unwrap_or(0);
             let last_idx = self.state.pending_collateral_updates.last().map(|(i,_,_,_)| *i).unwrap_or(0);
             
             egui::Window::new("Collateral Updates Detected")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(format!("The LLM returned updates for {} additional sentences (S{} - S{}).", num_updates, first_idx + 1, last_idx + 1));
                    ui.label("This often happens when using a context window to prevent hallucinations.");
                    ui.label("Do you want to apply these extra updates?");
                    
                    ui.separator();
                    
                    ui.horizontal(|ui| {
                        if ui.button("Yes, Apply All").clicked() {
                            self.state.pending_terminal_command = Some("approve collateral".to_string());
                            self.state.show_collateral_confirm = false;
                        }
                        
                        if ui.button("No, Discard Extras").clicked() {
                            self.state.pending_terminal_command = Some("discard collateral".to_string());
                            self.state.show_collateral_confirm = false;
                        }
                    });
                });
                
             if !open {
                 self.state.pending_collateral_updates.clear();
                 self.state.show_collateral_confirm = false;
             }
        }
        
        // LLM Settings window
        if self.state.show_llm_settings {            let mut open = true;
            egui::Window::new("LLM Settings").open(&mut open).show(ctx, |ui| {
                ui.label("Batch sizes (per-stage)");
                ui.horizontal(|ui| {
                    ui.label("Simplify:");
                    ui.add(egui::DragValue::new(&mut self.state.llm_batch_settings.simplify).clamp_range(1..=100));
                    if self.state.llm_batch_settings.simplify == 1 {
                        ui.label(egui::RichText::new("⚠ Risk of Hallucination").color(egui::Color32::RED).small());
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Mapping:");
                    ui.add(egui::DragValue::new(&mut self.state.llm_batch_settings.mapping).clamp_range(1..=100));
                });
                ui.horizontal(|ui| {
                    ui.label("Translate:");
                    ui.add(egui::DragValue::new(&mut self.state.llm_batch_settings.translate).clamp_range(1..=100));
                    if self.state.llm_batch_settings.translate == 1 {
                        ui.label(egui::RichText::new("⚠ Risk of Hallucination").color(egui::Color32::RED).small());
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        if let Some(p) = &self.current_file_path {
                            let proj_root = p.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."));
                            match self.state.llm_batch_settings.save(&proj_root) {
                                Ok(_) => self.status_message = "LLM settings saved.".to_string(),
                                Err(e) => self.status_message = format!("Failed to save settings: {}", e),
                            }
                        } else {
                            self.status_message = "No project file open to save settings.".to_string();
                        }
                        self.state.show_llm_settings = false;
                    }
                    if ui.button("Close").clicked() {
                        self.state.show_llm_settings = false;
                    }
                });
            });
            // Keep the flag in sync with window
            self.state.show_llm_settings = open;
        }

        // LLM Run dialog
        if self.state.show_llm_run {
            let mut open = true;
            egui::Window::new("Run Pipeline Stage").open(&mut open).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Start index:");
                    ui.add(egui::DragValue::new(&mut self.state.llm_run_start).clamp_range(0..=999999usize));
                    ui.label("End index:");
                    ui.add(egui::DragValue::new(&mut self.state.llm_run_end).clamp_range(0..=999999usize));
                });
                ui.horizontal(|ui| {
                    ui.label("Stage:");
                    egui::ComboBox::from_id_source("stage_select")
                        .selected_text(&self.state.llm_run_prompt_name)
                        .show_ui(ui, |ui| {
                            const STAGES: &[(&str, &str)] = &[
                                ("GenerateBasicBase",        "Basic Base (English simplify)"),
                                ("GenerateAdvancedTarget",   "Advanced Target (translate)"),
                                ("GenerateModerateTarget",   "Moderate Target (segment simplify)"),
                                ("GenerateBasicTarget",      "Basic Target (simplify)"),
                                ("GeneratePhraseMap",        "Phrase Map"),
                                ("GenerateInversePhraseMap", "Inverse Phrase Map"),
                            ];
                            for (key, label) in STAGES {
                                ui.selectable_value(
                                    &mut self.state.llm_run_prompt_name,
                                    key.to_string(),
                                    *label,
                                );
                            }
                        });
                });

                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Start").clicked() {
                        let doc_len = self.state.document.len();
                        let start = min(self.state.llm_run_start, doc_len.saturating_sub(1));
                        let end   = min(self.state.llm_run_end,   doc_len.saturating_sub(1));
                        let (s, e) = if start <= end { (start, end) } else { (end, start) };
                        let cmd = format!("run generate {} {} {}", self.state.llm_run_prompt_name, s, e);
                        self.state.pending_terminal_command = Some(cmd);
                        self.state.show_llm_run = false;
                    }
                    if ui.button("Use current").clicked() {
                        let cur = self.state.selected_sentence_idx;
                        self.state.llm_run_start = cur;
                        self.state.llm_run_end = cur;
                    }
                    if ui.button("Cancel").clicked() {
                        self.state.show_llm_run = false;
                    }
                });
            });
            self.state.show_llm_run = open;
        }

        // If a job is active, show quick controls in the main UI
        if self.state.llm_results_receiver.is_some() {
            egui::TopBottomPanel::top("llm_progress_panel").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("LLM job running — results will apply when ready.");
                    if ui.button("Cancel Job").clicked() {
                        // Show confirmation dialog offering to keep or revert current progress
                        self.state.show_cancel_confirm = true;
                    }
                });
            });
        }
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            self.render_menu_bar(ui);
            ui.separator();
            components::top_bar::render(ui, &mut self.state);
        });

        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            components::info_bar::render(ui, &mut self.state);
        });

        egui::TopBottomPanel::bottom("terminal_panel")
            .resizable(true)
            .default_height(150.0)
            .show(ctx, |ui| {
                self.render_terminal(ui);
            });

        egui::SidePanel::left("left_panel")
            .resizable(true)
            .default_width(300.0)
            .show(ctx, |ui| {
                components::navigator::render(ui, &mut self.state);
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            components::detail_view::render(ui, &mut self.state);
        });

        // Cancel confirmation dialog
        if self.state.show_cancel_confirm {
            let mut open = true;
            egui::Window::new("Cancel LLM Job").open(&mut open).show(ctx, |ui| {
                ui.label("Do you want to stop the running LLM job?");
                ui.label("You can either keep the progress applied so far, or revert all changes from this job.");
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Keep current progress").clicked() {
                        // Set cancel flag to stop further work but keep applied results
                        if let Some(flag) = &self.state.llm_cancel_flag {
                            flag.store(true, std::sync::atomic::Ordering::SeqCst);
                        }
                        self.state.show_cancel_confirm = false;
                        self.status_message = "LLM job cancelling; keeping applied progress.".to_string();
                    }
                    if ui.button("Revert progress and stop").clicked() {
                        // Set cancel flag and restore backups, then drop receiver
                        if let Some(flag) = &self.state.llm_cancel_flag {
                            flag.store(true, std::sync::atomic::Ordering::SeqCst);
                        }
                        for (idx, tier_id, prior_text) in &self.state.llm_job_backup {
                            if *idx < self.state.document.len() {
                                if let Some(sent) = self.state.document.get_mut(*idx) {
                                    sent.update_tier_text(tier_id, prior_text.clone());
                                }
                            }
                        }
                        self.state.llm_job_backup.clear();
                        self.state.llm_results_receiver = None;
                        self.state.llm_cancel_flag = None;
                        self.state.show_cancel_confirm = false;
                        self.status_message = "LLM job cancelled and progress reverted.".to_string();
                    }
                    if ui.button("Close").clicked() {
                        self.state.show_cancel_confirm = false;
                    }
                });
            });
            self.state.show_cancel_confirm = open;
        }
    }
}
