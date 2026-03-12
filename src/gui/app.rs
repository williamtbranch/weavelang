// src/gui/app.rs

use eframe::{egui, App, Frame};
use std::path::PathBuf;

use crate::gui::components;
use crate::app::state::AppState;
use crate::app::terminal::apply_llm_result;
use crate::app::server_relay::{self, RelayReceiver, RelayRequest, RelayResponse, ContentType};
use crate::app::server::{build_state_summary_from_state, build_sentence_summary, ApiResponse};
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
    dirty: bool,
    status_message: String,
    terminal_history: Vec<String>,
    terminal_input: String,
    current_title: String,
    // Co-pilot relay server
    relay_rx: Option<RelayReceiver>,
    copilot_server_name: String,
    copilot_server_port: u16,
    // Save prompt dialogs
    show_exit_save_prompt: bool,
    show_close_save_prompt: bool,
    pending_action_after_save: Option<String>,
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

        // --- Co-pilot server ---
        let copilot_name = state.config.as_ref()
            .and_then(|c| c.copilot_server_name.clone())
            .unwrap_or_else(|| "weavelang".to_string());
        let copilot_port = state.config.as_ref()
            .and_then(|c| c.copilot_server_port)
            .unwrap_or(3030);

        let relay_rx = match server_relay::start_relay_server(server_relay::RelayConfig {
            port: copilot_port,
            name: copilot_name.clone(),
        }) {
            Ok((rx, _handle)) => {
                status.push("Copilot: OK");
                state.copilot_server_info = Some((copilot_name.clone(), copilot_port));
                Some(rx)
            }
            Err(e) => {
                eprintln!("[WARN] Copilot server failed to start: {}", e);
                None
            }
        };

        let mut app = Self {
            state,
            current_file_path: None,
            dirty: false,
            status_message: format!("Ready. [{}]", status.join(", ")),
            terminal_history: vec!["WeaveLang Terminal initialized.".to_string()],
            terminal_input: String::new(),
            current_title: String::new(),
            relay_rx,
            copilot_server_name: copilot_name,
            copilot_server_port: copilot_port,
            show_exit_save_prompt: false,
            show_close_save_prompt: false,
            pending_action_after_save: None,
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

    fn execute_terminal_command(&mut self, cmd: &str) -> String {
        self.execute_terminal_command_from(cmd, ">")
    }

    fn execute_terminal_command_from(&mut self, cmd: &str, prompt: &str) -> String {
        self.terminal_history.push(format!("{} {}", prompt, cmd));

        // Intercept `$` prefix — copilot message from the user.
        let trimmed = cmd.trim();
        if trimmed.starts_with('$') {
            let msg = trimmed[1..].trim();
            let reply = format!("[copilot] Received: \"{}\" — AI agent routing is not yet connected. This will be forwarded to an LLM in a future release.", msg);
            self.terminal_history.push(reply.clone());
            return reply;
        }

        // Intercept `server info` — the relay server details are only known here.
        if trimmed == "server info" {
            let info = if self.relay_rx.is_some() {
                format!(
                    "Copilot server '{}' running on http://127.0.0.1:{}",
                    self.copilot_server_name, self.copilot_server_port
                )
            } else {
                "Copilot server is not running.".to_string()
            };
            self.terminal_history.push(info.clone());
            return info;
        }
        
        // Temporarily take state to run engine
        let mut engine = crate::app::engine::Engine::new(std::mem::take(&mut self.state));
        engine.current_file_path = self.current_file_path.clone();
        
        let was_err;
        let result = match crate::app::terminal::run_terminal_command(&mut engine, cmd) {
            Ok(Some(output)) => {
                was_err = false;
                for line in output.lines() {
                    self.terminal_history.push(line.to_string());
                }
                output
            }
            Ok(None) => {
                was_err = true;
                self.terminal_history.push("Exit command ignored in GUI.".to_string());
                "Exit command ignored in GUI.".to_string()
            }
            Err(e) => {
                was_err = true;
                let msg = format!("Error: {}", e);
                self.terminal_history.push(msg.clone());
                msg
            }
        };
        
        self.state = engine.state;
        self.current_file_path = engine.current_file_path;

        // Track dirty state based on command type
        if !was_err {
            let cmd_lower = trimmed.to_lowercase();
            if cmd_lower.starts_with("save project") {
                self.dirty = false;
            } else if cmd_lower.starts_with("load project") {
                self.dirty = false;
            } else if cmd_lower.starts_with("new project") || cmd_lower.starts_with("close project") {
                self.dirty = false;
            } else if cmd_lower.starts_with("import json") || cmd_lower.starts_with("import source") {
                // Imports are treated as unsaved new data
                self.dirty = true;
                self.current_file_path = None;
            } else if Self::is_state_changing_command(&cmd_lower) {
                self.dirty = true;
            }
        }

        result
    }

    /// Returns true if a terminal command modifies project data (for dirty tracking).
    fn is_state_changing_command(cmd: &str) -> bool {
        cmd.starts_with("update text")
            || cmd.starts_with("add sentence")
            || cmd.starts_with("remove sentence")
            || cmd.starts_with("edit ")
            || cmd.starts_with("approve")
            || cmd.starts_with("discard")
            || cmd.starts_with("run generate")
            || cmd.starts_with("set languages")
            || cmd.starts_with("set book_name")
            || cmd.starts_with("add pn_lemma")
            || cmd.starts_with("rm pn_lemma")
            || cmd.starts_with("add seg")
            || cmd.starts_with("rm seg")
            || cmd.starts_with("edit seg")
            || cmd.starts_with("edit_b")
            || cmd.starts_with("edit_word")
            || cmd.starts_with("edit_target")
            || cmd.starts_with("edit_targets")
            || cmd.starts_with("split")
            || cmd.starts_with("merge")
            || cmd.starts_with("insert")
            || cmd.starts_with("delete")
            || cmd.starts_with("accept map")
            || cmd.starts_with("init mapping")
            || cmd.starts_with("lemmatize")
            || cmd.starts_with("validate")
            || cmd.starts_with("calibrate")
            || cmd.starts_with("import level_map")
    }

    /// Execute a deferred action after the user responds to a save prompt.
    fn execute_pending_action(&mut self) {
        if let Some(action) = self.pending_action_after_save.take() {
            match action.as_str() {
                "__new_project__" => {
                    self.execute_terminal_command("new project Untitled");
                }
                "__import_json__" => {
                    self.import_json();
                }
                "__import_source__" => {
                    self.import_source_text();
                }
                "__open_binary__" => {
                    self.open_binary();
                }
                other => {
                    let cmd = other.to_string();
                    self.execute_terminal_command(&cmd);
                }
            }
        }
    }

    /// Handle a single relay request from the co-pilot server.
    fn handle_relay_request(&mut self, request: RelayRequest) -> RelayResponse {
        match request {
            RelayRequest::Terminal(cmd) => {
                let output = self.execute_terminal_command_from(&cmd, "[copilot]>");
                RelayResponse {
                    status: 200,
                    content_type: ContentType::Text,
                    body: output,
                }
            }
            RelayRequest::GetState => {
                let summary = build_state_summary_from_state(&self.state);
                self.terminal_history.push(format!(
                    "[copilot] query state → {} sentences loaded",
                    summary.sentence_count
                ));
                let envelope = ApiResponse {
                    success: true,
                    message: format!("{} sentences loaded", summary.sentence_count),
                    data: Some(summary),
                };
                RelayResponse {
                    status: 200,
                    content_type: ContentType::Json,
                    body: serde_json::to_string_pretty(&envelope).unwrap_or_default(),
                }
            }
            RelayRequest::GetSentence(num) => {
                // API is 1-based to match the terminal `select sentence N`
                if num == 0 {
                    self.terminal_history.push("[copilot] query sentence 0 → error: use 1-based numbering".to_string());
                    let envelope = ApiResponse::<()> {
                        success: false,
                        message: "Sentence numbers are 1-based. Use 1 for the first sentence.".to_string(),
                        data: None,
                    };
                    return RelayResponse {
                        status: 400,
                        content_type: ContentType::Json,
                        body: serde_json::to_string_pretty(&envelope).unwrap_or_default(),
                    };
                }
                let idx = num - 1;
                if let Some(sentence) = self.state.document.get(idx) {
                    let base_text = sentence.tiers.get("base")
                        .map(|t| t.full_text())
                        .unwrap_or_else(|| "<empty>".to_string());
                    let preview: String = base_text.chars().take(60).collect();
                    self.terminal_history.push(format!(
                        "[copilot] query sentence {} → {} \"{}\"",
                        num, sentence.id, preview
                    ));
                    let summary = build_sentence_summary(idx, sentence);
                    let envelope = ApiResponse {
                        success: true,
                        message: format!("Sentence {} (index {})", sentence.id, idx),
                        data: Some(summary),
                    };
                    RelayResponse {
                        status: 200,
                        content_type: ContentType::Json,
                        body: serde_json::to_string_pretty(&envelope).unwrap_or_default(),
                    }
                } else {
                    self.terminal_history.push(format!(
                        "[copilot] query sentence {} → out of range ({} sentences)",
                        num, self.state.document.len()
                    ));
                    let envelope = ApiResponse::<()> {
                        success: false,
                        message: format!(
                            "Sentence {} out of range (document has {} sentences)",
                            num,
                            self.state.document.len()
                        ),
                        data: None,
                    };
                    RelayResponse {
                        status: 404,
                        content_type: ContentType::Json,
                        body: serde_json::to_string_pretty(&envelope).unwrap_or_default(),
                    }
                }
            }
            RelayRequest::Ping => {
                self.terminal_history.push("[copilot] ping".to_string());
                RelayResponse {
                    status: 200,
                    content_type: ContentType::Json,
                    body: serde_json::to_string_pretty(&ApiResponse::<()> {
                        success: true,
                        message: format!(
                            "Co-pilot server '{}' is alive (GUI mode)",
                            self.copilot_server_name
                        ),
                        data: None,
                    })
                    .unwrap_or_default(),
                }
            }
            RelayRequest::Shutdown => {
                self.terminal_history.push("[copilot] shutdown requested".to_string());
                RelayResponse {
                    status: 200,
                    content_type: ContentType::Text,
                    body: "Shutdown acknowledged".to_string(),
                }
            }
        }
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
                        let rich = egui::RichText::new(line).family(egui::FontFamily::Monospace);
                        if line.starts_with("[copilot]>") {
                            ui.label(rich.color(egui::Color32::from_rgb(100, 180, 255)));
                        } else if line.starts_with("[copilot]") {
                            ui.label(rich.color(egui::Color32::from_rgb(140, 200, 140)));
                        } else {
                            ui.label(rich);
                        }
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
                if ui.button("New Project...").clicked() {
                    if self.dirty {
                        self.pending_action_after_save = Some("__new_project__".to_string());
                        self.show_close_save_prompt = true;
                    } else {
                        self.execute_terminal_command("new project Untitled");
                    }
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Import JSON...").clicked() {
                    if self.dirty {
                        self.pending_action_after_save = Some("__import_json__".to_string());
                        self.show_close_save_prompt = true;
                    } else {
                        self.import_json();
                    }
                    ui.close_menu();
                }
                if ui.button("Import Source Text...").clicked() {
                    if self.dirty {
                        self.pending_action_after_save = Some("__import_source__".to_string());
                        self.show_close_save_prompt = true;
                    } else {
                        self.import_source_text();
                    }
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Open .wvl...").clicked() {
                    if self.dirty {
                        self.pending_action_after_save = Some("__open_binary__".to_string());
                        self.show_close_save_prompt = true;
                    } else {
                        self.open_binary();
                    }
                    ui.close_menu();
                }
                if ui.button("Close Project").clicked() {
                    if self.dirty {
                        self.pending_action_after_save = Some("close project".to_string());
                        self.show_close_save_prompt = true;
                    } else {
                        self.execute_terminal_command("close project");
                    }
                    ui.close_menu();
                }
                ui.separator();
                let can_save = self.current_file_path.is_some();
                if ui.add_enabled(can_save, egui::Button::new("Save .wvl")).clicked() {
                    self.save_binary();
                    ui.close_menu();
                }
                if ui.button("Save .wvl As...").clicked() {
                    self.save_binary_as();
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
                    if self.dirty {
                        self.show_exit_save_prompt = true;
                    } else {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    ui.close_menu();
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
                let weave_ready = !self.state.document.is_empty()
                    && self.state.document.iter().all(|s| s.is_weave_ready())
                    && self.state.book_map.as_ref().map_or(false, |m| !m.is_empty());
                if ui.add_enabled(weave_ready, egui::Button::new("Generate Weave (All)")).clicked() {
                    self.execute_terminal_command("generate_weave all");
                    ui.close_menu();
                }
                if ui.add_enabled(weave_ready, egui::Button::new("Generate Weave (Level)...")).clicked() {
                    // Emit as terminal command; user can type the level in the terminal
                    self.execute_terminal_command("generate_weave all");
                    ui.close_menu();
                }
                if !weave_ready && !self.state.document.is_empty() {
                    let complete = self.state.document.iter().filter(|s| s.is_weave_ready()).count();
                    ui.label(format!("⚠ {}/{} sentences ready", complete, self.state.document.len()));
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

    fn save_binary_as(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("WeaveLang Binary", &["wvl"])
            .save_file()
        {
            let cmd = format!("save project {}", path.to_string_lossy());
            self.execute_terminal_command(&cmd);
        }
    }

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
        // ── Drain co-pilot relay requests ──────────────────────────────
        {
            let mut pending_requests = Vec::new();
            if let Some(rx) = &self.relay_rx {
                while let Ok(pending) = rx.try_recv() {
                    pending_requests.push(pending);
                }
            }
            for pending in pending_requests {
                let response = self.handle_relay_request(pending.request);
                let _ = pending.response_tx.send(response);
            }
            if self.relay_rx.is_some() {
                // Request continuous repaint so relay requests are served promptly
                ctx.request_repaint();
            }
        }

        // Intercept window close (X button) when dirty
        if ctx.input(|i| i.viewport().close_requested()) && self.dirty && !self.show_exit_save_prompt {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.show_exit_save_prompt = true;
        }

        // Update Title dynamically
        let dirty_marker = if self.dirty { " *" } else { "" };
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
            if self.state.book_name.is_empty() {
                format!("WeaveLang Studio ({}){}", ws_name, dirty_marker)
            } else {
                format!("WeaveLang Studio ({}) — {}{}", ws_name, self.state.book_name, dirty_marker)
            }
        } else {
            format!("WeaveLang Studio (Untitled){}", dirty_marker)
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
                                        apply_llm_result(sent, &tier_id, &text, bridge_ref, &lang, &target_lang);
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

                        if applied > 0 {
                            self.dirty = true;
                        }

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

                        // Clear edit buffers so mapping/segment panes re-sync
                        // with the freshly-updated model data on the next frame.
                        if applied > 0 {
                            self.state.seg_edit_buffers.clear();
                            self.state.mapping_selected_rows.clear();
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
                        let log_hint = self.state.logger.as_ref()
                            .map(|l| format!("\nLLM log: {}", l.log_file_path().display()))
                            .unwrap_or_default();
                        let done = self.state.llm_job_done;
                        let total = self.state.llm_job_total;
                        if done > 0 {
                            self.status_message = format!(
                                "LLM job failed after {}/{} items applied: {}",
                                done, total, err_str
                            );
                            self.state.last_log = format!(
                                "Error (partial: {}/{} applied): {}{}",
                                done, total, err_str, log_hint
                            );
                        } else {
                            self.status_message = format!("LLM job failed: {}", err_str);
                            self.state.last_log = format!("Error: {}{}", err_str, log_hint);
                        }
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
                // Check if this was a normal completion (not an error)
                let had_error = self.status_message.contains("failed") || self.status_message.contains("disconnected");
                self.state.llm_results_receiver = None;

                // Auto-advance from follow-up queue on successful completion
                if !had_error {
                    if let Some(next_cmd) = self.state.llm_followup_queue.pop_front() {
                        let remaining = self.state.llm_followup_queue.len();
                        self.status_message = format!(
                            "Starting follow-up: {} ({} remaining)",
                            next_cmd.split_whitespace().take(3).collect::<Vec<_>>().join(" "),
                            remaining,
                        );
                        self.state.pending_terminal_command = Some(next_cmd);
                    }
                } else {
                    // On error, clear the follow-up queue to prevent cascading failures
                    let cleared = self.state.llm_followup_queue.len();
                    self.state.llm_followup_queue.clear();
                    if cleared > 0 {
                        self.state.last_log.push_str(&format!(
                            " ({} queued follow-up steps cancelled.)",
                            cleared
                        ));
                    }
                }
            }
        }
        
        // --- Exit Save Prompt Dialog ---
        if self.show_exit_save_prompt {
            let mut open = true;
            egui::Window::new("Save Changes?")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("You have unsaved changes. Save before exiting?");
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            if self.current_file_path.is_some() {
                                self.save_binary();
                            } else {
                                self.save_binary_as();
                            }
                            self.show_exit_save_prompt = false;
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if ui.button("Don't Save").clicked() {
                            self.show_exit_save_prompt = false;
                            self.dirty = false;
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_exit_save_prompt = false;
                        }
                    });
                });
            if !open {
                self.show_exit_save_prompt = false;
            }
        }
        
        // --- Close/New/Open Save Prompt Dialog ---
        if self.show_close_save_prompt {
            let mut open = true;
            egui::Window::new("Save Changes?")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .id(egui::Id::new("close_save_prompt"))
                .show(ctx, |ui| {
                    ui.label("You have unsaved changes. Save before continuing?");
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            if self.current_file_path.is_some() {
                                self.save_binary();
                            } else {
                                self.save_binary_as();
                            }
                            self.show_close_save_prompt = false;
                            self.execute_pending_action();
                        }
                        if ui.button("Don't Save").clicked() {
                            self.dirty = false;
                            self.show_close_save_prompt = false;
                            self.execute_pending_action();
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_close_save_prompt = false;
                            self.pending_action_after_save = None;
                        }
                    });
                });
            if !open {
                self.show_close_save_prompt = false;
                self.pending_action_after_save = None;
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
            let mut pending_commands: Vec<String> = Vec::new();

            egui::Window::new("LLM / Stage Settings")
                .open(&mut open)
                .default_width(500.0)
                .show(ctx, |ui| {
                if let Some(draft) = &mut self.state.draft_config {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.heading("Models");
                        ui.label("Model aliases are referenced by stage configurations below. You can add, rename, or remove them freely.");
                        ui.add_space(4.0);

                        if !draft.models.is_empty() {
                            let mut keys: Vec<String> = draft.models.keys().cloned().collect();
                            keys.sort();
                            let mut remove_key: Option<String> = None;

                            for k in &keys {
                                ui.group(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new(k).strong().size(14.0));
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            if ui.small_button("Remove").clicked() {
                                                remove_key = Some(k.clone());
                                            }
                                        });
                                    });
                                    let v = draft.models.get_mut(k).unwrap();
                                    
                                    ui.horizontal(|ui| {
                                        ui.label("Name:");
                                        ui.text_edit_singleline(&mut v.name);
                                        ui.label("(?)").on_hover_text(format!("The actual API model identifier.\nCommand: config set models.{}.name <string>", k));
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Provider:");
                                        egui::ComboBox::from_id_source(format!("provider_{}", k))
                                            .selected_text(&v.provider)
                                            .show_ui(ui, |ui| {
                                                ui.selectable_value(&mut v.provider, "gemini".to_string(), "gemini");
                                                ui.selectable_value(&mut v.provider, "claude".to_string(), "claude");
                                            });
                                        ui.label("(?)").on_hover_text(format!("API provider: 'gemini' or 'claude'.\nCommand: config set models.{}.provider <string>", k));
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Max Tokens:");
                                        ui.add(egui::DragValue::new(&mut v.max_input_tokens).clamp_range(1..=10000000));
                                        ui.label("(?)").on_hover_text(format!("Command: config set models.{}.max_input_tokens <num>", k));
                                    });
                                });
                            }

                            // Process removal
                            if let Some(key_to_remove) = remove_key {
                                pending_commands.push(format!("config remove_model {}", key_to_remove));
                                draft.models.remove(&key_to_remove);
                            }
                        } else {
                            ui.label("No models defined. Add one below.");
                        }

                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label("New alias:");
                            ui.text_edit_singleline(&mut self.state.new_model_alias_input);
                            let alias_valid = !self.state.new_model_alias_input.trim().is_empty()
                                && !draft.models.contains_key(self.state.new_model_alias_input.trim());
                            if ui.add_enabled(alias_valid, egui::Button::new("+ Add Model")).clicked() {
                                let alias = self.state.new_model_alias_input.trim().to_string();
                                pending_commands.push(format!("config add_model {}", alias));
                                draft.models.insert(alias, crate::config::ModelConfig {
                                    provider: String::new(),
                                    name: String::new(),
                                    max_input_tokens: 10000,
                                });
                                self.state.new_model_alias_input.clear();
                            }
                        });

                        ui.separator();
                        ui.heading("Stage Configurations");
                        ui.label("Each stage uses a model alias from the list above.");
                        ui.add_space(4.0);
                        
                        let model_aliases: Vec<String> = draft.models.keys().cloned().collect();
                        let mut stage_keys: Vec<String> = draft.stages.keys().cloned().collect();
                        stage_keys.sort();

                        for k in stage_keys {
                            ui.group(|ui| {
                                ui.label(egui::RichText::new(&k).strong());
                                let stage = draft.stages.get_mut(&k).unwrap();
                                
                                ui.horizontal(|ui| {
                                    ui.label("Primary Model:");
                                    egui::ComboBox::from_id_source(format!("primary_{}", k))
                                        .selected_text(&stage.primary_model)
                                        .show_ui(ui, |ui| {
                                            for alias in &model_aliases {
                                                ui.selectable_value(&mut stage.primary_model, alias.clone(), alias);
                                            }
                                        });
                                    ui.label("(?)").on_hover_text(format!("Command: config set stages.{}.primary_model <alias>", k));
                                });

                                // Fallback model
                                let mut fb = stage.fallback_model.clone().unwrap_or_default();
                                ui.horizontal(|ui| {
                                    ui.label("Fallback Model:");
                                    egui::ComboBox::from_id_source(format!("fallback_{}", k))
                                        .selected_text(if fb.is_empty() { "(none)" } else { &fb })
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(&mut fb, String::new(), "(none)");
                                            for alias in &model_aliases {
                                                ui.selectable_value(&mut fb, alias.clone(), alias);
                                            }
                                        });
                                    ui.label("(?)").on_hover_text(format!("Command: config set stages.{}.fallback_model <alias>", k));
                                });
                                stage.fallback_model = if fb.is_empty() { None } else { Some(fb) };

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
                                    // Detect new models (added via UI)
                                    for (k, v) in &draft.models {
                                        if !real.models.contains_key(k) {
                                            // New model — emit add + set commands
                                            pending_commands.push(format!("config add_model {}", k));
                                            if !v.name.is_empty() {
                                                pending_commands.push(format!("config set models.{}.name {}", k, v.name));
                                            }
                                            if !v.provider.is_empty() {
                                                pending_commands.push(format!("config set models.{}.provider {}", k, v.provider));
                                            }
                                            if v.max_input_tokens != 10000 {
                                                pending_commands.push(format!("config set models.{}.max_input_tokens {}", k, v.max_input_tokens));
                                            }
                                        } else if let Some(real_v) = real.models.get(k) {
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
                                    // Detect removed models
                                    for k in real.models.keys() {
                                        if !draft.models.contains_key(k) {
                                            pending_commands.push(format!("config remove_model {}", k));
                                        }
                                    }
                                    for (k, stage) in &draft.stages {
                                        if let Some(real_stage) = real.stages.get(k) {
                                            if stage.primary_model != real_stage.primary_model {
                                                pending_commands.push(format!("config set stages.{}.primary_model {}", k, stage.primary_model));
                                            }
                                            if stage.fallback_model != real_stage.fallback_model {
                                                let fb_val = stage.fallback_model.as_deref().unwrap_or("none");
                                                pending_commands.push(format!("config set stages.{}.fallback_model {}", k, fb_val));
                                            }
                                            if stage.batch_size_in_items != real_stage.batch_size_in_items {
                                                pending_commands.push(format!("config set stages.{}.batch_size_in_items {}", k, stage.batch_size_in_items));
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
                let max_num = self.state.document.len().max(1);
                ui.horizontal(|ui| {
                    ui.label("Start:");
                    let mut start_num = self.state.llm_run_start + 1;
                    if ui.add(egui::DragValue::new(&mut start_num).clamp_range(1..=max_num)).changed() {
                        self.state.llm_run_start = start_num.saturating_sub(1);
                    }
                    ui.label("End:");
                    let mut end_num = self.state.llm_run_end + 1;
                    if ui.add(egui::DragValue::new(&mut end_num).clamp_range(1..=max_num)).changed() {
                        self.state.llm_run_end = end_num.saturating_sub(1);
                    }
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
                        // Terminal now expects 1-based sentence numbers
                        let cmd = format!("run generate {} {} {}", self.state.llm_run_prompt_name, s + 1, e + 1);
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
                    let queue_len = self.state.llm_followup_queue.len();
                    if queue_len > 0 {
                        ui.label(format!(
                            "LLM job running — {} follow-up step{} queued.",
                            queue_len,
                            if queue_len == 1 { "" } else { "s" },
                        ));
                    } else {
                        ui.label("LLM job running — results will apply when ready.");
                    }
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
                        self.state.llm_followup_queue.clear();
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
                        self.state.llm_followup_queue.clear();
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
