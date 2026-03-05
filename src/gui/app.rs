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
    current_title: String,
}

impl WeaveLangApp {
    pub fn new(
        _cc: &eframe::CreationContext<'_>,
        bridge: Option<BridgeService>,
        llm: Option<LlmService>,
        prompts: Option<PromptManager>,
        logger: Option<LlmLogger>,
        initial_config: Option<crate::config::Config>,
    ) -> Self {
        let mut state = AppState::default();
        state.bridge = bridge;
        state.llm = llm;
        state.prompts = prompts;
        state.logger = logger;
        state.config = initial_config;

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

        let mut app = Self {
            state,
            current_file_path: None,
            status_message: format!("Ready. [{}]", status.join(", ")),
            terminal_history: vec!["WeaveLang Terminal initialized.".to_string()],
            terminal_input: String::new(),
            current_title: String::new(),
        };

        let gs = crate::global_settings::GlobalSettings::load();
        if let Some(ws) = gs.last_workspace {
            if std::path::Path::new(&ws).exists() {
                app.execute_terminal_command(&format!("open workspace {}", ws));
                
                // Also open last wvl file if preferred
                let mut should_load_project = None;
                if let Some(cfg) = &app.state.config {
                    if cfg.open_last_project.unwrap_or(true) {
                        if let Some(proj) = &cfg.last_project_file {
                            if std::path::Path::new(proj).exists() {
                                should_load_project = Some(proj.clone());
                            }
                        }
                    }
                }
                if let Some(proj) = should_load_project {
                    app.execute_terminal_command(&format!("load project {}", proj));
                }
            }
        }

        app
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
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
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
                        let batch_size = self.state.config.as_ref()
                            .and_then(|c| c.stages.get(*stage_key))
                            .map(|s| s.batch_size_in_items)
                            .unwrap_or(20);
                        let end   = min(start + batch_size.saturating_sub(1),
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
                if ui.button("Project Settings...").clicked() {
                    self.state.draft_config = self.state.config.clone();
                    self.state.show_project_settings = true;
                    ui.close_menu();
                }
                if ui.button("LLM Settings...").clicked() {
                    self.state.draft_config = self.state.config.clone();
                    self.state.show_llm_settings = true;
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
        // Update Title dynamically
        let expected_title = if let Some(cfg) = &self.state.config {
            let ws_name = if cfg.content_project_dir.is_empty() {
                "Untitled".to_string()
            } else {
                std::path::PathBuf::from(&cfg.content_project_dir)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Untitled")
                    .to_string()
            };
            format!("WeaveLang Studio ({})", ws_name)
        } else {
            "WeaveLang Studio (Untitled)".to_string()
        };

        if self.current_title != expected_title {
            self.current_title = expected_title.clone();
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(expected_title));
        }

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
                        
                        let batch_size = self.state.config.as_ref()
                            .and_then(|c| c.stages.get(&self.state.llm_run_prompt_name))
                            .map(|s| s.batch_size_in_items)
                            .unwrap_or(20);
                        let is_bulk_run = self.state.llm_job_total > batch_size.max(10);
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
        
        // Project Settings Window
        if self.state.show_project_settings {
            let mut open = true;
            let mut pending_commands = Vec::new();

            egui::Window::new("Project Settings").open(&mut open).show(ctx, |ui| {
                if let Some(draft) = &mut self.state.draft_config {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.heading("General");
                        
                        ui.horizontal(|ui| {
                            let mut b = draft.open_last_project.unwrap_or(true);
                            if ui.checkbox(&mut b, "Open Last Project/Workspace on Startup").changed() {
                                draft.open_last_project = Some(b);
                            }
                            ui.label("(?)").on_hover_text("Command: config set open_last_project true/false");
                        });

                        ui.horizontal(|ui| {
                            ui.label("Network Retry Delay (ms):");
                            ui.add(egui::DragValue::new(&mut draft.pipeline.retry_delay).clamp_range(0..=10000));
                            ui.label("(?)").on_hover_text("Command: config set pipeline.retry_delay <num>");
                        });

                        ui.horizontal(|ui| {
                            ui.label("Max API Retries:");
                            ui.add(egui::DragValue::new(&mut draft.pipeline.max_api_retries).clamp_range(0..=100));
                            ui.label("(?)").on_hover_text("Command: config set pipeline.max_api_retries <num>");
                        });

                        ui.separator();
                        
                        ui.horizontal(|ui| {
                            if ui.button("Apply").clicked() {
                                if let Some(real) = &self.state.config {
                                    if draft.open_last_project != real.open_last_project {
                                        pending_commands.push(format!("config set open_last_project {}", draft.open_last_project.unwrap_or(true)));
                                    }
                                    if draft.pipeline.retry_delay != real.pipeline.retry_delay {
                                        pending_commands.push(format!("config set pipeline.retry_delay {}", draft.pipeline.retry_delay));
                                    }
                                    if draft.pipeline.max_api_retries != real.pipeline.max_api_retries {
                                        pending_commands.push(format!("config set pipeline.max_api_retries {}", draft.pipeline.max_api_retries));
                                    }
                                }
                                self.state.show_project_settings = false;
                            }
                            if ui.button("Cancel").clicked() {
                                self.state.show_project_settings = false;
                            }
                        });
                    });
                } else {
                    ui.label("No active config to edit. Ensure you've run 'load project'.");
                }
            });

            for cmd in pending_commands {
                self.execute_terminal_command(&cmd);
            }
            
            if !open {
                self.state.show_project_settings = false;
            }
        }

        // LLM Settings window
        if self.state.show_llm_settings {
            let mut open = true;
            let mut pending_commands = Vec::new();

            egui::Window::new("LLM / Stage Settings").open(&mut open).show(ctx, |ui| {
                if let Some(draft) = &mut self.state.draft_config {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.heading("Models");
                        if !draft.models.is_empty() {
                            let mut keys: Vec<String> = draft.models.keys().cloned().collect();
                            keys.sort();
                            
                            for k in keys {
                                ui.group(|ui| {
                                    ui.label(egui::RichText::new(&k).strong());
                                    let v = draft.models.get_mut(&k).unwrap();
                                    
                                    ui.horizontal(|ui| {
                                        ui.label("Name:");
                                        ui.text_edit_singleline(&mut v.name);
                                        ui.label("(?)").on_hover_text(format!("Command: config set models.{}.name <string>", k));
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Provider:");
                                        ui.text_edit_singleline(&mut v.provider);
                                        ui.label("(?)").on_hover_text(format!("Command: config set models.{}.provider <string>", k));
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Max Tokens:");
                                        ui.add(egui::DragValue::new(&mut v.max_input_tokens).clamp_range(1..=10000000));
                                        ui.label("(?)").on_hover_text(format!("Command: config set models.{}.max_input_tokens <num>", k));
                                    });
                                });
                            }
                        } else {
                            ui.label("No models defined.");
                        }

                        ui.separator();
                        ui.heading("Stage Configurations");
                        
                        let mut stage_keys: Vec<String> = draft.stages.keys().cloned().collect();
                        stage_keys.sort();

                        for k in stage_keys {
                            ui.group(|ui| {
                                ui.label(egui::RichText::new(&k).strong());
                                let stage = draft.stages.get_mut(&k).unwrap();
                                
                                ui.horizontal(|ui| {
                                    ui.label("Primary Model:");
                                    ui.text_edit_singleline(&mut stage.primary_model);
                                    ui.label("(?)").on_hover_text(format!("Command: config set stages.{}.primary_model <val>", k));
                                });
                                
                                ui.horizontal(|ui| {
                                    ui.label("Batch Size:");
                                    ui.add(egui::DragValue::new(&mut stage.batch_size_in_items).clamp_range(1..=1000));
                                    ui.label("(?)").on_hover_text(format!("Command: config set stages.{}.batch_size <num>", k));
                                });
                            });
                        }
                        
                        ui.separator();
                        
                        ui.horizontal(|ui| {
                            if ui.button("Apply").clicked() {
                                if let Some(real) = &self.state.config {
                                    for (k, v) in &draft.models {
                                        if let Some(real_v) = real.models.get(k) {
                                            if v.name != real_v.name {
                                                pending_commands.push(format!("config set models.{}.name {}", k, v.name));
                                            }
                                            if v.provider != real_v.provider {
                                                pending_commands.push(format!("config set models.{}.provider {}", k, v.provider));
                                            }
                                            if v.max_input_tokens != real_v.max_input_tokens {
                                                pending_commands.push(format!("config set models.{}.max_input_tokens {}", k, v.max_input_tokens));
                                            }
                                        }
                                    }
                                    for (k, stage) in &draft.stages {
                                        if let Some(real_stage) = real.stages.get(k) {
                                            if stage.primary_model != real_stage.primary_model {
                                                pending_commands.push(format!("config set stages.{}.primary_model {}", k, stage.primary_model));
                                            }
                                            if stage.batch_size_in_items != real_stage.batch_size_in_items {
                                                pending_commands.push(format!("config set stages.{}.batch_size {}", k, stage.batch_size_in_items));
                                            }
                                        }
                                    }
                                }
                                self.state.show_llm_settings = false;
                            }
                            if ui.button("Cancel").clicked() {
                                self.state.show_llm_settings = false;
                            }
                        });
                    });
                } else {
                    ui.label("No active config to edit. Ensure you've run 'load project'.");
                }
            });

            for cmd in pending_commands {
                self.execute_terminal_command(&cmd);
            }
            
            if !open {
                self.state.show_llm_settings = false;
            }
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
            if self.state.document.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() * 0.25);
                    ui.heading("No Document Open");
                    ui.add_space(8.0);
                    ui.label("Use File › Open .wvl... or File › Import Source Text...");
                });
            } else {
                components::detail_view::render(ui, &mut self.state);
            }
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
