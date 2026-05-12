// src/gui/app.rs

use eframe::{egui, App, Frame};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Instant;

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

/// Truncate verbose command output to a manageable size for copilot history.
/// Keeps the first `max_chars` characters and appends a truncation notice.
fn compact_command_output(output: &str, max_chars: usize) -> String {
    if output.len() <= max_chars {
        return output.to_string();
    }
    let truncated: String = output.chars().take(max_chars).collect();
    let remaining = output.len() - max_chars;
    format!("{}... [truncated, {} more chars]", truncated, remaining)
}

pub struct WeaveLangApp {
    state: AppState,
    current_file_path: Option<PathBuf>,
    dirty: bool,
    status_message: String,
    terminal_history: Vec<String>,
    terminal_input: String,
    command_history: Vec<String>,
    command_history_idx: Option<usize>,
    command_history_draft: String,
    current_title: String,
    // Co-pilot relay server
    relay_rx: Option<RelayReceiver>,
    copilot_server_name: String,
    copilot_server_port: u16,
    // Save prompt dialogs
    show_exit_save_prompt: bool,
    show_close_save_prompt: bool,
    pending_action_after_save: Option<String>,
    // AV job output tracking
    av_job_lines_seen: usize,
    // Auto-backup / crash-recovery
    last_backup_time: Instant,
    show_restore_prompt: bool,
    pending_restore_stale_lock: bool,
    // Generate-weave level picker dialog state
    show_weave_level_dialog: bool,
    weave_level_selected: BTreeSet<usize>,
    weave_level_anchor: Option<usize>,
    weave_level_force: bool,
    weave_level_frontier: bool,
    weave_level_frontier_pct: f32,
    weave_level_frontier_seed: u64,
    // Study-format specific controls
    weave_sf_step: u32,
    weave_sf_start_level: u32,
}

impl WeaveLangApp {
    pub fn new(
        _cc: &eframe::CreationContext<'_>,
        bridge: Option<BridgeService>,
        llm: Option<LlmService>,
        prompts: Option<PromptManager>,
        logger: Option<LlmLogger>,
        initial_config: Option<crate::config::Config>,
        tool_root_dir: Option<std::path::PathBuf>,
    ) -> Self {
        let mut state = AppState::default();
        state.tool_root_dir = tool_root_dir;
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
            command_history: Vec::new(),
            command_history_idx: None,
            command_history_draft: String::new(),
            current_title: String::new(),
            relay_rx,
            copilot_server_name: copilot_name,
            copilot_server_port: copilot_port,
            show_exit_save_prompt: false,
            show_close_save_prompt: false,
            pending_action_after_save: None,
            av_job_lines_seen: 0,
            last_backup_time: Instant::now(),
            show_restore_prompt: false,
            pending_restore_stale_lock: false,
            show_weave_level_dialog: false,
            weave_level_selected: BTreeSet::new(),
            weave_level_anchor: None,
            weave_level_force: false,
            weave_level_frontier: true,
            weave_level_frontier_pct: 5.0,
            weave_level_frontier_seed: 777,
            weave_sf_step: 2,
            weave_sf_start_level: 16,
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
            if msg.is_empty() {
                let reply = "[copilot] Usage: $ <message> — send a message to the co-pilot agent.".to_string();
                self.terminal_history.push(reply.clone());
                return reply;
            }
            return self.handle_copilot_message(msg);
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
                // Manual save → delete backup, keep lock
                if let Some(ref pp) = self.current_file_path {
                    crate::app::backup::remove_backup(pp);
                }
                self.last_backup_time = Instant::now();
            } else if cmd_lower.starts_with("load project") {
                self.dirty = false;
                // Check for crash-recovery backup
                if let Some(ref pp) = self.current_file_path {
                    if let Some(info) = crate::app::backup::check_recovery(pp) {
                        self.pending_restore_stale_lock = info.stale_lock;
                        self.show_restore_prompt = true;
                    }
                    // Write fresh lock (we now own this project)
                    crate::app::backup::write_lock(pp);
                }
                self.last_backup_time = Instant::now();
            } else if cmd_lower.starts_with("open workspace") {
                // Restore copilot session from disk
                self.load_copilot_session();
                // After workspace opens, check for pending copilot goal
                self.check_copilot_goal();
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
                || cmd == "audit"
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

                // Up/Down arrow command history navigation
                if response.has_focus() {
                    let up = ui.input(|i| i.key_pressed(egui::Key::ArrowUp));
                    let down = ui.input(|i| i.key_pressed(egui::Key::ArrowDown));
                    if up && !self.command_history.is_empty() {
                        match self.command_history_idx {
                            None => {
                                // Save current draft, move to most recent
                                self.command_history_draft = self.terminal_input.clone();
                                let idx = self.command_history.len() - 1;
                                self.command_history_idx = Some(idx);
                                self.terminal_input = self.command_history[idx].clone();
                            }
                            Some(idx) if idx > 0 => {
                                let new_idx = idx - 1;
                                self.command_history_idx = Some(new_idx);
                                self.terminal_input = self.command_history[new_idx].clone();
                            }
                            _ => {} // already at oldest
                        }
                    }
                    if down {
                        if let Some(idx) = self.command_history_idx {
                            if idx + 1 < self.command_history.len() {
                                let new_idx = idx + 1;
                                self.command_history_idx = Some(new_idx);
                                self.terminal_input = self.command_history[new_idx].clone();
                            } else {
                                // Past most recent → restore draft
                                self.command_history_idx = None;
                                self.terminal_input = self.command_history_draft.clone();
                            }
                        }
                    }
                }

                let submit = |app: &mut Self, refocus: &egui::Response| {
                    let cmd = app.terminal_input.clone();
                    if !cmd.trim().is_empty() {
                        app.command_history.push(cmd.trim().to_string());
                        app.command_history_idx = None;
                        app.command_history_draft.clear();
                        app.execute_terminal_command(&cmd);
                        app.terminal_input.clear();
                        refocus.request_focus();
                    }
                };

                if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    submit(self, &response);
                }
                
                if ui.button("Run").clicked() {
                    submit(self, &response);
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
                        self.cancel_running_llm_job();
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
                    if let Some(cfg) = &mut self.state.config {
                        cfg.ensure_required_defaults();
                    }
                    self.state.draft_config = self.state.config.clone();
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
                let has_level_map = self.state.book_map.as_ref().map_or(false, |m| !m.is_empty());
                let weave_ready = if self.state.chapter_mode {
                    // In chapter mode: check only the selected chapter
                    has_level_map && self.state.selected_chapter_idx
                        .and_then(|ci| self.state.chapters.get(ci))
                        .map_or(false, |ch| {
                            let s0 = ch.start.saturating_sub(1);
                            let e0 = ch.end.saturating_sub(1);
                            (s0..=e0).all(|i| self.state.document.get(i).map_or(false, |s| s.is_weave_ready()))
                        })
                } else {
                    !self.state.document.is_empty()
                        && self.state.document.iter().all(|s| s.is_weave_ready())
                        && has_level_map
                };
                if ui.add_enabled(weave_ready, egui::Button::new("Generate Weave (All)")).clicked() {
                    self.execute_terminal_command("generate_weave all");
                    ui.close_menu();
                }
                if ui.add_enabled(weave_ready, egui::Button::new("Generate Weave (Level)...")).clicked() {
                    self.show_weave_level_dialog = true;
                    self.weave_level_selected.clear();
                    self.weave_level_anchor = None;
                    self.weave_level_force = false;
                    self.weave_level_frontier = true;
                    self.weave_level_frontier_pct = 5.0;
                    self.weave_level_frontier_seed = 777;
                    self.weave_sf_step = 2;
                    self.weave_sf_start_level = 16;
                    ui.close_menu();
                }
                if !weave_ready && !self.state.document.is_empty() {
                    if self.state.chapter_mode {
                        if let Some(ch) = self.state.selected_chapter_idx.and_then(|ci| self.state.chapters.get(ci)) {
                            let s0 = ch.start.saturating_sub(1);
                            let e0 = ch.end.saturating_sub(1);
                            let ch_complete = (s0..=e0).filter(|&i| self.state.document.get(i).map_or(false, |s| s.is_weave_ready())).count();
                            let ch_total = e0 - s0 + 1;
                            ui.label(format!("⚠ Chapter: {}/{} ready", ch_complete, ch_total));
                        } else {
                            ui.label("⚠ No chapter selected");
                        }
                    } else {
                        let complete = self.state.document.iter().filter(|s| s.is_weave_ready()).count();
                        ui.label(format!("⚠ {}/{} sentences ready", complete, self.state.document.len()));
                    }
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
                    if let Some(cfg) = &mut self.state.config {
                        cfg.ensure_required_defaults();
                    }
                    self.state.draft_config = self.state.config.clone();
                    self.state.show_llm_settings = true;
                    ui.close_menu();
                }
                if ui.button("Project Flags...").clicked() {
                    self.state.show_project_flags = true;
                    ui.close_menu();
                }
                ui.separator();
                let is_book_mode = !self.state.chapter_mode;
                let is_chapter_mode = self.state.chapter_mode;
                if ui.selectable_label(is_book_mode, "Book Mode").clicked() {
                    self.execute_terminal_command("set chapter_mode false");
                    ui.close_menu();
                }
                if ui.selectable_label(is_chapter_mode, "Chapter Mode").clicked() {
                    self.execute_terminal_command("set chapter_mode true");
                    ui.close_menu();
                }
            });

            // --- Chapters menu ---
            ui.menu_button("Chapters", |ui| {
                let has_chapters = !self.state.chapters.is_empty();

                // Select Chapter submenu
                let mut pending_select_cmd: Option<String> = None;
                if has_chapters {
                    ui.menu_button("Select Chapter", |ui| {
                        for (i, ch) in self.state.chapters.iter().enumerate() {
                            let is_selected = self.state.selected_chapter_idx == Some(i);
                            let label = format!("{} ({}-{})", ch.name, ch.start, ch.end);
                            if ui.selectable_label(is_selected, &label).clicked() {
                                pending_select_cmd = Some(format!("select chapter \"{}\"", ch.name));
                                ui.close_menu();
                            }
                        }
                    });
                    ui.separator();
                }
                if let Some(cmd) = pending_select_cmd {
                    self.execute_terminal_command(&cmd);
                }

                if ui.button("New Chapter...").clicked() {
                    // For now emit a hint; a dialog could be added later
                    self.status_message = "Use terminal: new chapter \"Name\" <start> <end>".to_string();
                    ui.close_menu();
                }
                if has_chapters {
                    if ui.button("Delete Chapter...").clicked() {
                        self.status_message = "Use terminal: delete chapter \"Name\"".to_string();
                        ui.close_menu();
                    }
                }
                ui.separator();
                if ui.button("List Chapters").clicked() {
                    self.execute_terminal_command("list chapters");
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Init Media Workspace").clicked() {
                    self.execute_terminal_command("init media");
                    ui.close_menu();
                }
            });

            ui.separator();
            ui.label(format!("Status: {}", self.status_message));
        });
    }

    fn weave_level_options(&self) -> Vec<(String, String)> {
        let mut options: Vec<(String, String)> = Vec::new();

        // Numeric levels from the currently loaded map
        let mut numeric_levels: Vec<u32> = self
            .state
            .book_map
            .as_ref()
            .map(|m| {
                let mut lvls: Vec<u32> = m.keys().filter_map(|k| k.parse::<u32>().ok()).collect();
                lvls.sort_unstable();
                lvls.dedup();
                lvls
            })
            .unwrap_or_default();

        // UL0 is valid even when absent from map; show it first.
        if !numeric_levels.contains(&0) {
            numeric_levels.insert(0, 0);
        }

        for lvl in numeric_levels {
            options.push((lvl.to_string(), format!("UL{}", lvl)));
        }

        // Special outputs
        options.push(("b".to_string(), "ULb (basic-heavy)".to_string()));
        options.push(("m".to_string(), "ULm (moderate-heavy)".to_string()));
        options.push(("a".to_string(), "ULa (advanced-heavy)".to_string()));
        options.push(("i".to_string(), "ULi (interlinear)".to_string()));
        options.push(("r".to_string(), "ULr (raw source)".to_string()));
        options.push(("sf".to_string(), "ULsf (study format)".to_string()));

        options
    }

    fn run_selected_weave_levels(&mut self, options: &[(String, String)]) {
        if self.weave_level_selected.is_empty() {
            self.status_message = "No weave level selected.".to_string();
            return;
        }

        let mut selected_indices: Vec<usize> = self.weave_level_selected.iter().copied().collect();
        selected_indices.sort_unstable();

        for idx in selected_indices {
            if let Some((arg, _label)) = options.get(idx) {
                let mut flags = String::new();
                if arg == "sf" {
                    // Study format: frontier is always off; use sf-specific flags
                    if self.weave_sf_step != 2 {
                        flags.push_str(&format!(" --sf-step {}", self.weave_sf_step));
                    }
                    if self.weave_sf_start_level != 16 {
                        flags.push_str(&format!(" --sf-start {}", self.weave_sf_start_level));
                    }
                    if self.weave_level_force {
                        flags.push_str(" --force");
                    }
                } else {
                    if self.weave_level_force {
                        flags.push_str(" --force");
                    }
                    if !self.weave_level_frontier {
                        flags.push_str(" --no-frontier");
                    } else {
                        if (self.weave_level_frontier_pct - 5.0).abs() > 0.001 {
                            flags.push_str(&format!(" --frontier-pct {}", self.weave_level_frontier_pct));
                        }
                        if self.weave_level_frontier_seed != 777 {
                            flags.push_str(&format!(" --frontier-seed {}", self.weave_level_frontier_seed));
                        }
                    }
                }
                let cmd = format!("generate_weave {}{}", arg, flags);
                self.execute_terminal_command(&cmd);
            }
        }
    }

    // ── Auto-backup helpers ────────────────────────────────────────────

    /// Write a backup of the current state to `<project>.backup`.
    /// No-op if the project is not dirty or has no file path.
    fn maybe_write_backup(&mut self) {
        if !self.dirty {
            return;
        }
        if let Some(ref pp) = self.current_file_path {
            match crate::app::backup::write_backup(&self.state, pp) {
                Ok(()) => {
                    self.last_backup_time = Instant::now();
                    eprintln!("[BACKUP] Auto-backup written to {:?}", crate::app::backup::backup_path(pp));
                }
                Err(e) => eprintln!("[BACKUP] {}", e),
            }
        }
    }

    /// Restore state from the `.backup` file, replacing the current state.
    fn restore_from_backup(&mut self) {
        let project_path = match self.current_file_path.clone() {
            Some(p) => p,
            None => return,
        };
        let bp = crate::app::backup::backup_path(&project_path);
        match crate::app::backup::load_backup(&bp, &self.state) {
            Ok(restored) => {
                self.state = restored;
                self.dirty = true; // so the user will eventually save
                crate::app::backup::remove_backup(&project_path);
                self.terminal_history.push("[BACKUP] State restored from backup.".to_string());
                self.status_message = "Restored from backup — remember to save.".to_string();
            }
            Err(e) => {
                let msg = format!("[BACKUP] Restore failed: {}", e);
                self.terminal_history.push(msg.clone());
                self.status_message = msg;
                // Clean up the unusable backup
                crate::app::backup::remove_backup(&project_path);
            }
        }
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

    /// Handle a `$`-prefixed copilot message from the terminal.
    /// Spawns a background thread for the LLM call so the UI stays responsive.
    /// Results are polled in `update()` via `poll_copilot_llm()`.
    fn handle_copilot_message(&mut self, user_msg: &str) -> String {
        use crate::services::copilot;

        // Check if copilot model is configured
        let model_alias = match self.state.config.as_ref()
            .and_then(|c| c.copilot.as_ref())
            .and_then(|cp| cp.model.as_ref())
            .filter(|m| !m.is_empty())
        {
            Some(m) => m.clone(),
            None => {
                let reply = "[copilot] No copilot model configured. Set one in Preferences → LLM Settings, or run: config set copilot.model <alias>".to_string();
                self.terminal_history.push(reply.clone());
                return reply;
            }
        };

        // Check if LLM service is available
        let llm = match &self.state.llm {
            Some(l) => l.clone(),
            None => {
                let reply = "[copilot] LLM service not available. Check API key configuration.".to_string();
                self.terminal_history.push(reply.clone());
                return reply;
            }
        };

        // Check turn limit
        let max_turns = self.state.config.as_ref()
            .and_then(|c| c.copilot.as_ref())
            .and_then(|cp| cp.max_turns)
            .unwrap_or(50);

        if self.state.copilot_turns >= max_turns {
            let reply = format!("[copilot] Turn limit reached ({}/{}). Start a new session or increase copilot.max_turns.", self.state.copilot_turns, max_turns);
            self.terminal_history.push(reply.clone());
            return reply;
        }

        // Already waiting for a copilot response — don't stack requests
        if self.state.copilot_llm_rx.is_some() {
            let reply = "[copilot] Already processing a request. Please wait.".to_string();
            self.terminal_history.push(reply.clone());
            return reply;
        }

        // Copilot is waiting for a background generation/AV job — allow the new message
        // by discarding any remaining pending commands from the previous turn.
        if self.state.copilot_awaiting_llm_job || self.state.copilot_awaiting_av {
            self.terminal_history.push("[copilot] Interrupting current wait to handle your message.".to_string());
            self.state.copilot_awaiting_llm_job = false;
            self.state.copilot_awaiting_av = false;
            self.state.copilot_pending_cmds.clear();
            // Keep accumulated outputs so far — they'll be part of the next turn context
            if !self.state.copilot_cmd_outputs.is_empty() {
                let results_summary = self.state.copilot_cmd_outputs.join("\n");
                self.state.copilot_cmd_outputs.clear();
                self.state.copilot_history.push((
                    "user".to_string(),
                    format!("[System: partial command results before interruption]\n{}", results_summary),
                ));
            }
        }

        // Build workspace context (reads copilot/ files)
        let workspace_context = self.state.config.as_ref()
            .map(|c| {
                let ws_path = std::path::Path::new(&c.content_project_dir);
                copilot::build_workspace_context(ws_path)
            })
            .unwrap_or_default();

        // Add user message to history
        self.state.copilot_history.push(("user".to_string(), user_msg.to_string()));

        // Reset auto-continue counter for this new user message
        self.state.copilot_auto_turns = 0;

        // Spawn background thread for LLM call
        self.terminal_history.push("[copilot] Thinking...".to_string());
        let history_snapshot = self.state.copilot_history.clone();
        let user_msg_owned = user_msg.to_string();
        let (tx, rx) = std::sync::mpsc::channel();

        std::thread::spawn(move || {
            let result = copilot::send_copilot_message(
                &llm,
                &model_alias,
                copilot::COPILOT_SYSTEM_PROMPT,
                &workspace_context,
                &history_snapshot,
                &user_msg_owned,
            );
            let _ = tx.send(result);
        });

        self.state.copilot_llm_rx = Some(rx);
        self.state.copilot_running = true;
        "[copilot] Thinking...".to_string()
    }

    /// Spawn a background follow-up LLM call for the auto-continue loop.
    fn spawn_copilot_followup(&mut self) {
        use crate::services::copilot;

        let model_alias = match self.state.config.as_ref()
            .and_then(|c| c.copilot.as_ref())
            .and_then(|cp| cp.model.as_ref())
            .filter(|m| !m.is_empty())
        {
            Some(m) => m.clone(),
            None => return,
        };
        let llm = match &self.state.llm {
            Some(l) => l.clone(),
            None => return,
        };
        let workspace_context = self.state.config.as_ref()
            .map(|c| copilot::build_workspace_context(std::path::Path::new(&c.content_project_dir)))
            .unwrap_or_default();

        let history_snapshot = self.state.copilot_history.clone();
        // Use the last user message (the system command-results entry) as the user message
        let last_user_msg = history_snapshot.iter().rev()
            .find(|(role, _)| role == "user")
            .map(|(_, text)| text.clone())
            .unwrap_or_default();

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = copilot::send_copilot_message(
                &llm,
                &model_alias,
                copilot::COPILOT_SYSTEM_PROMPT,
                &workspace_context,
                &history_snapshot,
                &last_user_msg,
            );
            let _ = tx.send(result);
        });

        self.state.copilot_llm_rx = Some(rx);
    }

    /// Poll the copilot LLM background channel. Called each frame from `update()`.
    fn poll_copilot_llm(&mut self) {
        use crate::services::copilot;
        use std::sync::mpsc::TryRecvError;

        let rx = match &self.state.copilot_llm_rx {
            Some(r) => r,
            None => return,
        };

        let msg = match rx.try_recv() {
            Ok(v) => v,
            Err(TryRecvError::Empty) => return, // still waiting
            Err(TryRecvError::Disconnected) => {
                self.terminal_history.push("[copilot] LLM channel disconnected.".to_string());
                self.state.copilot_llm_rx = None;
                self.state.copilot_running = false;
                return;
            }
        };

        // We have a response — clear the receiver
        self.state.copilot_llm_rx = None;

        match msg {
            Ok(reply_text) => {
                self.state.copilot_turns += 1;
                self.state.copilot_history.push(("assistant".to_string(), reply_text.clone()));

                // Display the reply
                for line in reply_text.lines() {
                    let trimmed = line.trim();
                    if trimmed.starts_with("CMD:") {
                        let cmd = trimmed[4..].trim();
                        self.terminal_history.push(format!("[copilot] Executing: {}", cmd));
                    } else if !trimmed.is_empty() {
                        self.terminal_history.push(format!("[copilot] {}", line));
                    }
                }

                // Extract CMD: lines and execute them non-blockingly.
                // If a command starts a background LLM job, defer remaining commands
                // and resume when the job completes (instead of blocking with watch_job).
                let commands = copilot::extract_commands(&reply_text);
                if !commands.is_empty() {
                    self.state.copilot_cmd_outputs.clear();
                    self.state.copilot_pending_cmds = commands;
                    self.execute_copilot_pending_cmds();
                    return; // keep copilot_running true; completion handled elsewhere
                }

                // No commands — copilot turn is done
                self.finish_copilot_turn();
            }
            Err(e) => {
                self.terminal_history.push(format!("[copilot] Error: {}", e));
                // Remove the failed user message from history
                self.state.copilot_history.pop();
                self.state.copilot_running = false;
            }
        }

        // Save session to disk after each completed turn
        self.save_copilot_session();
    }

    /// Execute queued copilot CMD: lines one at a time, non-blockingly.
    /// If a command starts a background LLM job, we pause and set
    /// `copilot_awaiting_llm_job` so `update()` can resume us when the job finishes.
    /// `watch_job` commands are skipped (we wait for the job non-blockingly instead).
    fn execute_copilot_pending_cmds(&mut self) {
        while let Some(cmd) = self.state.copilot_pending_cmds.first().cloned() {
            self.state.copilot_pending_cmds.remove(0);

            // Skip watch_job — we handle job completion non-blockingly
            if cmd.trim() == "watch_job" || cmd.trim() == "job_status" {
                continue;
            }

            self.terminal_history.push(format!("[copilot] Executing: {}", cmd));
            let output = self.execute_terminal_command_from(&cmd, "[copilot]>");

            // Compact the output for history (keep first 500 chars)
            let compact_output = compact_command_output(&output, 500);
            self.state.copilot_cmd_outputs.push(format!("Command `{}` → {}", cmd, compact_output));

            // Check if this command started a background LLM job
            if self.state.llm_results_receiver.is_some() {
                self.state.copilot_awaiting_llm_job = true;
                self.terminal_history.push("[copilot] Waiting for generation job to finish...".to_string());
                return; // pause — update() will resume us when the job completes
            }

            // Check if this command started an AV job
            if self.state.av_job.is_some() {
                self.state.copilot_awaiting_av = true;
                self.terminal_history.push("[copilot] Waiting for AV job to finish...".to_string());
                return; // pause — update() will resume us when the AV job completes
            }
        }

        // All CMD: lines executed — finish this copilot turn
        self.finish_copilot_cmd_turn();
    }

    /// Called when all copilot CMD: lines for the current turn are done.
    /// Pushes the accumulated results into history and decides whether to auto-continue.
    fn finish_copilot_cmd_turn(&mut self) {
        if !self.state.copilot_cmd_outputs.is_empty() {
            let results_summary = self.state.copilot_cmd_outputs.join("\n");
            self.state.copilot_cmd_outputs.clear();
            self.state.copilot_history.push((
                "user".to_string(),
                format!("[System: command results]\n{}", results_summary),
            ));

            // Auto-continue if within limits
            let max_turns = self.state.config.as_ref()
                .and_then(|c| c.copilot.as_ref())
                .and_then(|cp| cp.max_turns)
                .unwrap_or(50);
            let max_auto = 5u32;

            if self.state.copilot_auto_turns < max_auto && self.state.copilot_turns < max_turns {
                self.state.copilot_auto_turns += 1;
                self.terminal_history.push("[copilot] Thinking...".to_string());
                self.spawn_copilot_followup();
                self.save_copilot_session();
                return; // keep copilot_running true
            }
        }

        // Done — no more auto-continue
        self.finish_copilot_turn();
    }

    /// Final cleanup when a copilot interaction is fully complete.
    fn finish_copilot_turn(&mut self) {
        // If an AV job is still running, park the copilot to resume when it finishes
        if self.state.av_job.is_some() && !self.state.copilot_history.is_empty() {
            self.state.copilot_awaiting_av = true;
            self.terminal_history.push("[copilot] Waiting for AV job to finish...".to_string());
        }
        self.state.copilot_running = false;
        self.save_copilot_session();
    }

    /// Persist copilot conversation history to disk.
    fn save_copilot_session(&self) {
        use crate::services::copilot;
        if self.state.copilot_history.is_empty() {
            return;
        }
        if let Some(cfg) = &self.state.config {
            if !cfg.content_project_dir.is_empty() {
                copilot::save_session(
                    std::path::Path::new(&cfg.content_project_dir),
                    &self.state.copilot_history,
                    self.state.copilot_turns,
                );
            }
        }
    }

    /// Restore copilot session from disk (called on workspace open).
    fn load_copilot_session(&mut self) {
        use crate::services::copilot;
        if let Some(cfg) = &self.state.config {
            if !cfg.content_project_dir.is_empty() {
                let (history, turns) = copilot::load_session(
                    std::path::Path::new(&cfg.content_project_dir),
                );
                if !history.is_empty() {
                    let count = history.len();
                    self.state.copilot_history = history;
                    self.state.copilot_turns = turns;
                    self.terminal_history.push(format!(
                        "[copilot] Restored previous session ({} messages, {} turns).",
                        count, turns
                    ));
                }
            }
        }
    }

    /// Check for a pending goal at startup (called once after workspace opens).
    /// If copilot/_goal.toml has actionable content, notify the user.
    fn check_copilot_goal(&mut self) {
        use crate::services::copilot;

        let has_model = self.state.config.as_ref()
            .and_then(|c| c.copilot.as_ref())
            .and_then(|cp| cp.model.as_ref())
            .map(|m| !m.is_empty())
            .unwrap_or(false);

        if !has_model {
            return;
        }

        let has_goal = self.state.config.as_ref()
            .map(|c| {
                let ws_path = std::path::Path::new(&c.content_project_dir);
                copilot::has_pending_goal(ws_path)
            })
            .unwrap_or(false);

        if has_goal {
            self.terminal_history.push("[copilot] Pending goal detected in copilot/_goal.toml. Starting autonomous session...".to_string());
            self.handle_copilot_message("A goal file is ready. Please read it and begin executing the plan.");
        } else {
            self.terminal_history.push("[copilot] No pending goal. Use $ <message> to chat, or edit copilot/_goal.toml to set a production goal.".to_string());
        }
    }

    /// Cancel any in-flight LLM job and clear the follow-up queue.
    fn cancel_running_llm_job(&mut self) {
        eprintln!("[APP] cancel_running_llm_job called — setting cancel flag");
        if let Some(flag) = &self.state.llm_cancel_flag {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        self.state.llm_followup_queue.clear();
        // Also cancel any in-flight copilot LLM call
        self.state.copilot_llm_rx = None;
        self.state.copilot_running = false;
        self.state.copilot_awaiting_av = false;
        self.state.copilot_awaiting_llm_job = false;
        self.state.copilot_pending_cmds.clear();
        self.state.copilot_cmd_outputs.clear();
    }
}

impl Drop for WeaveLangApp {
    fn drop(&mut self) {
        eprintln!("[APP] Drop impl fired — cleaning up LLM threads");
        self.save_copilot_session();
        self.cancel_running_llm_job();
        // Remove lock file (clean shutdown signal)
        if let Some(ref pp) = self.current_file_path {
            crate::app::backup::remove_lock(pp);
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
        // If close was NOT intercepted (not dirty), cancel any running LLM job
        if ctx.input(|i| i.viewport().close_requested()) && !self.dirty {
            self.cancel_running_llm_job();
        }

        // Update Title dynamically
        let dirty_marker = if self.dirty { " *" } else { "" };
        let simple_marker = if self.state.simple_mode { " [simple mode]" } else { "" };
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
                format!("WeaveLang Studio ({}){}{}", ws_name, simple_marker, dirty_marker)
            } else {
                format!("WeaveLang Studio ({}) — {}{}{}", ws_name, self.state.book_name, simple_marker, dirty_marker)
            }
        } else {
            format!("WeaveLang Studio (Untitled){}{}", simple_marker, dirty_marker)
        };

        if self.current_title != expected_title {
            self.current_title = expected_title.clone();
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(expected_title));
        }

        // Check for pending terminal commands from UI components
        if let Some(cmd) = self.state.pending_terminal_command.take() {
            self.execute_terminal_command(&cmd);
        }

        // Poll copilot LLM background channel
        self.poll_copilot_llm();

        // Poll background AV job for new output lines
        if let Some(ref job) = self.state.av_job {
            let j = job.lock().unwrap();
            // Drain any new lines since the last poll
            while self.av_job_lines_seen < j.output_lines.len() {
                self.terminal_history.push(j.output_lines[self.av_job_lines_seen].clone());
                self.av_job_lines_seen += 1;
            }
            if j.finished {
                if let Some(ref msg) = j.result_message {
                    self.state.last_log = msg.clone();
                }
            }
            let finished = j.finished;
            let result_msg = if finished { j.result_message.clone() } else { None };
            let job_label = j.label.clone();
            drop(j);
            if finished {
                self.state.av_job = None;
                self.av_job_lines_seen = 0;

                // If copilot was waiting for this AV job, wake it up
                if self.state.copilot_awaiting_av {
                    self.state.copilot_awaiting_av = false;
                    let status = result_msg.as_deref().unwrap_or("AV job finished.");
                    self.state.copilot_cmd_outputs.push(
                        format!("AV job completed: {} — {}", job_label, status)
                    );
                    self.terminal_history.push("[copilot] AV job done. Resuming...".to_string());

                    if !self.state.copilot_pending_cmds.is_empty() {
                        // Continue executing remaining copilot CMD: lines
                        self.execute_copilot_pending_cmds();
                    } else {
                        // No more pending cmds — finish the turn (will auto-continue to LLM)
                        self.state.copilot_auto_turns = 0;
                        self.finish_copilot_cmd_turn();
                    }
                }
            }
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
                        let selected_idx = self.state.selected_sentence_idx;
                        let num_results = results.len();

                        let (base_lang, target_lang) = self.state.project_languages.clone();
                        let bridge_ref = self.state.bridge.as_ref();
                        let friendly_lemmas = self.state.friendly_lemmas.clone();
                        let friendly_enabled = self.state.friendly_shielding_enabled;

                        for (idx, _s_id, tier_id, text) in results {
                            if idx < self.state.document.len() {
                                let lang = lang_for_tier(&tier_id, &base_lang, &target_lang);
                                if let Some(sent) = self.state.document.get_mut(idx) {
                                    apply_llm_result(sent, &tier_id, &text, bridge_ref, &lang, &target_lang, &friendly_lemmas, friendly_enabled);
                                    if idx == selected_idx {
                                         if tier_id.starts_with("MAPPING:") {
                                             last_applied_text = Some("Mapping Generated".to_string());
                                         } else {
                                             last_applied_text = Some(sent.get_tier(&tier_id).map(|t| t.full_text()).unwrap_or_default());
                                         }
                                    }
                                    applied += 1;
                                }
                            }
                        }
                        self.state.llm_job_done = self.state.llm_job_done.saturating_add(num_results);

                        if applied > 0 {
                            self.dirty = true;
                            self.state.audit_passed = false;
                        }

                        // Update visible logs
                        if let Some(txt) = last_applied_text {
                            self.state.last_log = format!("LLM result: {}", txt);
                        } else if applied > 0 {
                            self.state.last_log = format!("LLM applied {} items.", applied);
                        }

                        // Clear edit buffers only for the currently-selected sentence
                        // so that in-progress edits on other sentences aren't lost.
                        if applied > 0 {
                            let sel = self.state.selected_sentence_idx;
                            self.state.seg_edit_buffers.retain(|k, _| {
                                // Buffer keys contain the sentence index as the second
                                // underscore-separated component (e.g. "mt_basic_target_42_3").
                                // Clear only buffers for the selected sentence.
                                !k.contains(&format!("_{}_", sel))
                            });
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
                    // Auto-backup on LLM job completion (important checkpoint)
                    self.maybe_write_backup();

                    if let Some(next_cmd) = self.state.llm_followup_queue.pop_front() {
                        let remaining = self.state.llm_followup_queue.len();
                        self.status_message = format!(
                            "Starting follow-up: {} ({} remaining)",
                            next_cmd.split_whitespace().take(3).collect::<Vec<_>>().join(" "),
                            remaining,
                        );
                        self.state.pending_terminal_command = Some(next_cmd);
                    } else if self.state.copilot_awaiting_llm_job {
                        // Follow-up queue drained and copilot was waiting — resume copilot
                        self.state.copilot_awaiting_llm_job = false;
                        let done = self.state.llm_job_done;
                        let total = self.state.llm_job_total;
                        let stage = self.state.llm_job_stage.clone();
                        self.state.copilot_cmd_outputs.push(
                            format!("LLM job completed. {}/{} items applied (stage: {}).", done, total, stage)
                        );
                        self.terminal_history.push(format!(
                            "[copilot] Generation complete ({}/{} items, stage: {}). Continuing...",
                            done, total, stage
                        ));
                        // Continue executing any remaining copilot CMD: lines
                        self.execute_copilot_pending_cmds();
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
                    // If copilot was waiting, give it the error and let it decide
                    if self.state.copilot_awaiting_llm_job {
                        self.state.copilot_awaiting_llm_job = false;
                        self.state.copilot_cmd_outputs.push(
                            format!("LLM job failed: {}", self.status_message)
                        );
                        // Skip remaining commands, finish the turn so copilot can react
                        self.state.copilot_pending_cmds.clear();
                        self.finish_copilot_cmd_turn();
                    }
                }
            }
        }
        
        // ── Periodic auto-backup (every 10 minutes while dirty) ──────────
        if self.dirty
            && self.current_file_path.is_some()
            && self.last_backup_time.elapsed().as_secs() >= 600
        {
            self.maybe_write_backup();
        }

        // ── Restore-from-Backup prompt dialog ────────────────────────────
        if self.show_restore_prompt {
            let mut open = true;
            let stale = self.pending_restore_stale_lock;
            egui::Window::new("Restore from Backup?")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .id(egui::Id::new("restore_backup_prompt"))
                .show(ctx, |ui| {
                    if stale {
                        ui.label(
                            "The application was not shut down properly. \
                             A backup with more recent changes was found.",
                        );
                    } else {
                        ui.label(
                            "A backup file with more recent changes than the \
                             saved project was found.",
                        );
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Restore from Backup").clicked() {
                            self.restore_from_backup();
                            self.show_restore_prompt = false;
                        }
                        if ui.button("Discard Backup").clicked() {
                            if let Some(ref pp) = self.current_file_path.clone() {
                                crate::app::backup::remove_backup(pp);
                            }
                            self.show_restore_prompt = false;
                            self.terminal_history
                                .push("[BACKUP] Backup discarded.".to_string());
                        }
                    });
                });
            if !open {
                // Closed via X → treat as discard
                if let Some(ref pp) = self.current_file_path.clone() {
                    crate::app::backup::remove_backup(pp);
                }
                self.show_restore_prompt = false;
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

        // Project Flags Window (read-only with edit controls — Phase F)
        if self.state.show_project_flags {
            let mut open = true;
            let mut pending_commands: Vec<String> = Vec::new();

            let simple_mode = self.state.simple_mode;
            let lesson_realign = self.state.lesson_realign_enabled;
            let source_is_basic = self.state.source_is_basic;
            let frontier_enabled = self.state.frontier_enabled;
            let friendly_shielding = self.state.friendly_shielding_enabled;
            let teaching_active = simple_mode && lesson_realign && !frontier_enabled && friendly_shielding;
            let (src_lang, tgt_lang) = self.state.project_languages.clone();
            let source_is_target = self.state.source_is_target();
            let book_name = self.state.book_name.clone();
            let level_map_src = if self.state.level_map_embedded {
                "embedded (%%META lm_entry%%)"
            } else if self.state.book_map.as_ref().map_or(false, |m| !m.is_empty()) {
                "calibrated/imported"
            } else {
                "none"
            };
            let friendly_lemmas = self.state.friendly_lemmas.clone();

            egui::Window::new("Project Flags").open(&mut open)
                .default_width(420.0)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.heading("Languages");
                        egui::Grid::new("flags_lang_grid").num_columns(2).show(ui, |ui| {
                            ui.label("Source language:");
                            ui.label(if src_lang.is_empty() { "(unset)" } else { src_lang.as_str() });
                            ui.end_row();
                            ui.label("Target language:");
                            ui.label(if tgt_lang.is_empty() { "(unset)" } else { tgt_lang.as_str() });
                            ui.end_row();
                            ui.label("source_is_target:");
                            ui.label(if source_is_target { "yes (Spanish-source mode)" } else { "no" });
                            ui.end_row();
                            ui.label("Book name:");
                            ui.label(if book_name.is_empty() { "(unset)" } else { book_name.as_str() });
                            ui.end_row();
                            ui.label("Level map:");
                            ui.horizontal(|ui| {
                                ui.label(level_map_src);
                                let has_map = self.state.level_map_embedded
                                    || self.state.book_map.as_ref().map_or(false, |m| !m.is_empty());
                                if has_map {
                                    if ui.button("Strip").on_hover_text("Clear the loaded level map (use before re-calibrating an embedded map).").clicked() {
                                        pending_commands.push("strip_level_map".to_string());
                                    }
                                }
                            });
                            ui.end_row();
                        });
                        ui.label("(Use 'set languages <src> <tgt>' or 'set source_language' / 'set target_language' in the terminal.)")
                            .on_hover_text("Languages are edited via terminal commands.");

                        ui.separator();
                        ui.heading("Modes");

                        let mut sm = simple_mode;
                        if ui.checkbox(&mut sm, "Simple mode").changed() {
                            pending_commands.push(format!("set simple_mode {}", if sm { "on" } else { "off" }));
                        }
                        let mut lr = lesson_realign;
                        if ui.checkbox(&mut lr, "Lesson realign").changed() {
                            pending_commands.push(format!("set lesson_realign {}", if lr { "on" } else { "off" }));
                        }
                        let mut sib = source_is_basic;
                        if ui.checkbox(&mut sib, "Source is already basic").changed() {
                            pending_commands.push(format!("set source_is_basic {}", if sib { "on" } else { "off" }));
                        }
                        ui.label("When on, the same-language basic tier is copied from base verbatim. Requires simple mode for generation to use it.")
                            .on_hover_text("Used for already-simple source text. en-es skips GenerateBasicBase; es-es skips GenerateBasicTarget.");
                        let mut fs = friendly_shielding;
                        if ui.checkbox(&mut fs, "Friendly shielding").changed() {
                            pending_commands.push(format!("set friendly_shielding {}", if fs { "on" } else { "off" }));
                        }

                        ui.horizontal(|ui| {
                            ui.label(format!(
                                "Teaching mode (derived): {}",
                                if teaching_active { "ON" } else { "custom/off" }
                            ));
                            if ui.button("Apply teaching_mode preset").on_hover_text(
                                "Sets simple_mode=on, lesson_realign=on, frontier_enabled=off, friendly_shielding=on"
                            ).clicked() {
                                pending_commands.push("set teaching_mode on".to_string());
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label(format!(
                                "Frontier enabled: {}",
                                if frontier_enabled { "on" } else { "off" }
                            ));
                            if teaching_active {
                                ui.label("(read-only: teaching_mode active)");
                            }
                        });

                        ui.separator();
                        ui.heading(format!("Friendly lemmas ({})", friendly_lemmas.len()));

                        ui.horizontal(|ui| {
                            ui.label("Add:");
                            let resp = ui.text_edit_singleline(&mut self.state.project_flags_friendly_draft);
                            let submit = ui.button("+").clicked()
                                || (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                            if submit {
                                let v = self.state.project_flags_friendly_draft.trim().to_string();
                                if !v.is_empty() {
                                    pending_commands.push(format!("set_friendly_lemma {}", v));
                                    self.state.project_flags_friendly_draft.clear();
                                }
                            }
                        });

                        if friendly_lemmas.is_empty() {
                            ui.label("(none)");
                        } else {
                            for lemma in &friendly_lemmas {
                                ui.horizontal(|ui| {
                                    ui.label(lemma);
                                    if ui.small_button("✕").on_hover_text("Remove").clicked() {
                                        pending_commands.push(format!("unset_friendly_lemma {}", lemma));
                                    }
                                });
                            }
                            if ui.button("Clear all").clicked() {
                                pending_commands.push("clear_friendly_lemmas".to_string());
                            }
                        }

                        ui.separator();
                        ui.horizontal(|ui| {
                            if ui.button("Close").clicked() {
                                self.state.show_project_flags = false;
                            }
                        });
                    });
                });

            for cmd in pending_commands {
                self.execute_terminal_command(&cmd);
            }
            if !open {
                self.state.show_project_flags = false;
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
                        ui.heading("Co-pilot Agent");
                        ui.label("Select which model the $ prefix routes to for co-pilot chat and autonomous goals.");
                        ui.add_space(4.0);

                        // Ensure copilot config exists in draft
                        if draft.copilot.is_none() {
                            draft.copilot = Some(crate::config::CopilotConfig {
                                model: None,
                                max_turns: Some(50),
                            });
                        }
                        if let Some(ref mut cop) = draft.copilot {
                            let model_aliases_cop: Vec<String> = draft.models.keys().cloned().collect();
                            let mut current = cop.model.clone().unwrap_or_default();

                            ui.horizontal(|ui| {
                                ui.label("Model:");
                                egui::ComboBox::from_id_source("copilot_model")
                                    .selected_text(if current.is_empty() { "(disabled)" } else { &current })
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut current, String::new(), "(disabled)");
                                        for alias in &model_aliases_cop {
                                            ui.selectable_value(&mut current, alias.clone(), alias);
                                        }
                                    });
                                ui.label("(?)").on_hover_text("Model alias for the co-pilot agent.\nCommand: config set copilot.model <alias>");
                            });
                            cop.model = if current.is_empty() { None } else { Some(current) };

                            let mut mt = cop.max_turns.unwrap_or(50);
                            ui.horizontal(|ui| {
                                ui.label("Max Turns:");
                                ui.add(egui::DragValue::new(&mut mt).clamp_range(1..=500));
                                ui.label("(?)").on_hover_text("Safety cap: maximum LLM round-trips per session.\nCommand: config set copilot.max_turns <num>");
                            });
                            cop.max_turns = Some(mt);
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
                                    // Copilot config diff
                                    let draft_cop = draft.copilot.as_ref();
                                    let real_cop = real.copilot.as_ref();
                                    let draft_model = draft_cop.and_then(|c| c.model.as_deref()).unwrap_or("");
                                    let real_model = real_cop.and_then(|c| c.model.as_deref()).unwrap_or("");
                                    if draft_model != real_model {
                                        if draft_model.is_empty() {
                                            pending_commands.push("config set copilot.model none".to_string());
                                        } else {
                                            pending_commands.push(format!("config set copilot.model {}", draft_model));
                                        }
                                    }
                                    let draft_mt = draft_cop.and_then(|c| c.max_turns).unwrap_or(50);
                                    let real_mt = real_cop.and_then(|c| c.max_turns).unwrap_or(50);
                                    if draft_mt != real_mt {
                                        pending_commands.push(format!("config set copilot.max_turns {}", draft_mt));
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

        if self.show_weave_level_dialog {
            let mut open = true;
            let options = self.weave_level_options();

            egui::Window::new("Generate Weave Levels")
                .open(&mut open)
                .default_width(420.0)
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label("Select one or more weave outputs.");
                    ui.label("Click to select, Ctrl-click to toggle, Shift-click for range.");
                    ui.add_space(6.0);

                    egui::ScrollArea::vertical()
                        .max_height(280.0)
                        .show(ui, |ui| {
                            for (idx, (arg, label)) in options.iter().enumerate() {
                                let is_selected = self.weave_level_selected.contains(&idx);
                                let row_label = format!("{:>3}   {}", arg, label);
                                let resp = ui.selectable_label(is_selected, row_label);

                                if resp.clicked() {
                                    let mods = ui.ctx().input(|i| i.modifiers);
                                    if mods.shift {
                                        let anchor = self.weave_level_anchor.unwrap_or(idx);
                                        let (start, end) = if anchor <= idx { (anchor, idx) } else { (idx, anchor) };
                                        if !mods.ctrl {
                                            self.weave_level_selected.clear();
                                        }
                                        for i in start..=end {
                                            self.weave_level_selected.insert(i);
                                        }
                                    } else if mods.ctrl {
                                        if is_selected {
                                            self.weave_level_selected.remove(&idx);
                                        } else {
                                            self.weave_level_selected.insert(idx);
                                        }
                                        self.weave_level_anchor = Some(idx);
                                    } else {
                                        self.weave_level_selected.clear();
                                        self.weave_level_selected.insert(idx);
                                        self.weave_level_anchor = Some(idx);
                                    }
                                }
                            }
                        });

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Select All").clicked() {
                            self.weave_level_selected.clear();
                            for i in 0..options.len() {
                                self.weave_level_selected.insert(i);
                            }
                        }
                        if ui.button("Clear").clicked() {
                            self.weave_level_selected.clear();
                        }
                        ui.checkbox(&mut self.weave_level_force, "Skip DRC (--force)");
                    });

                    ui.add_space(6.0);
                    ui.separator();
                    // Study-format controls — only shown when sf is selected
                    let sf_selected = options.iter().enumerate()
                        .any(|(i, (arg, _))| arg == "sf" && self.weave_level_selected.contains(&i));
                    if sf_selected {
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label("SF Step:");
                            ui.add(egui::DragValue::new(&mut self.weave_sf_step)
                                .speed(1.0)
                                .clamp_range(1_u32..=10_u32));
                            ui.label("SF Start Level:");
                            ui.add(egui::DragValue::new(&mut self.weave_sf_start_level)
                                .speed(1.0)
                                .clamp_range(0_u32..=50_u32));
                        });
                        ui.label(egui::RichText::new("(Frontier is always off for study format)").weak().italics());
                    }

                    // Frontier controls — hidden when only sf is selected
                    let only_sf = !self.weave_level_selected.is_empty() && self.weave_level_selected.iter().all(|&i| {
                        options.get(i).map_or(false, |(arg, _)| arg == "sf")
                    });
                    if !only_sf {
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut self.weave_level_frontier, "Frontier filter");
                            if self.weave_level_frontier {
                                ui.label("Target %:");
                                ui.add(egui::DragValue::new(&mut self.weave_level_frontier_pct)
                                    .speed(0.1)
                                    .clamp_range(0.1_f32..=50.0_f32)
                                    .suffix("%"));
                                ui.label("Seed:");
                                ui.add(egui::DragValue::new(&mut self.weave_level_frontier_seed)
                                    .speed(1.0)
                                    .clamp_range(0_u64..=9_999_999_u64));
                            }
                        });
                    }
                    ui.add_space(4.0);
                    ui.separator();

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Generate Selected").clicked() {
                            self.run_selected_weave_levels(&options);
                            self.show_weave_level_dialog = false;
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_weave_level_dialog = false;
                        }
                    });
                });

            if !open {
                self.show_weave_level_dialog = false;
            }
        }

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
            if self.state.show_media_tab {
                components::media_view::render(ui, &mut self.state);
            } else if self.state.document.is_empty() {
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
