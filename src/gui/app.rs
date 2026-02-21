// src/gui/app.rs

use eframe::{egui, App, Frame};
use std::fs;
use std::path::PathBuf;

use crate::domain::bridge;
use crate::gui::components;
use crate::gui::state::AppState;
use crate::types::json_types::JsonChapter;
use crate::services::python_bridge::BridgeService;
use crate::services::llm_client::LlmService;
use crate::services::prompt_manager::PromptManager;
use crate::services::llm_logger::LlmLogger;
use crate::parsing::source_parser;

pub struct WeaveLangApp {
    state: AppState,
    current_file_path: Option<PathBuf>,
    status_message: String,
}

impl WeaveLangApp {
    pub fn new(
        _cc: &eframe::CreationContext<'_>, 
        bridge: Option<BridgeService>, 
        llm: Option<LlmService>,
        prompts: Option<PromptManager>,
        logger: Option<LlmLogger>
    ) -> Self {
        let mut state = AppState::default();
        state.bridge = bridge;
        state.llm = llm;
        state.prompts = prompts;
        state.logger = logger;

        let mut status = Vec::new();
        if state.bridge.is_some() { status.push("Bridge: OK"); } else { status.push("Bridge: OFF"); }
        if state.llm.is_some() { status.push("LLM: OK"); } else { status.push("LLM: OFF"); }

        Self {
            state,
            current_file_path: None,
            status_message: format!("Ready. [{}]", status.join(", ")),
        }
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
                if ui.button("Exit").clicked() {
                    std::process::exit(0);
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
            match fs::read_to_string(&path) {
                Ok(json_content) => {
                    match serde_json::from_str::<JsonChapter>(&json_content) {
                        Ok(json_chapter) => {
                            self.state.document.clear();
                            self.state.book_map = Some(json_chapter.u_level_maps.clone());
                            
                            self.state.project_languages = (
                                json_chapter.book_meta.base_language.clone(),
                                json_chapter.book_meta.target_language.clone()
                            );

                            let mut error_count = 0;
                            for block in json_chapter.content_blocks {
                                if let crate::types::json_types::JsonContentBlock::Sentence(json_sentence) = block {
                                    match bridge::json_to_domain_sentence(&json_sentence) {
                                        Ok(domain_sentence) => {
                                            self.state.document.push(domain_sentence);
                                        }
                                        Err(e) => {
                                            eprintln!("Skipping invalid sentence: {}", e);
                                            error_count += 1;
                                        }
                                    }
                                }
                            }

                            self.current_file_path = None;
                            self.state.selected_sentence_idx = 0;
                            self.status_message = format!(
                                "Imported {} sentences ({} errors).",
                                self.state.document.len(),
                                error_count
                            );
                        }
                        Err(e) => self.status_message = format!("JSON Parse Error: {}", e),
                    }
                }
                Err(e) => self.status_message = format!("File Read Error: {}", e),
            }
        }
    }

    fn import_source_text(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Text File", &["txt"])
            .pick_file()
        {
            match fs::read_to_string(&path) {
                Ok(content) => {
                    match source_parser::parse_source_file(&content) {
                        Ok(docs) => {
                            self.state.document = docs;
                            self.state.book_map = None;
                            self.state.selected_sentence_idx = 0;
                            self.current_file_path = None; 
                            self.status_message = format!("Imported {} sentences from source.", self.state.document.len());
                        },
                        Err(e) => self.status_message = format!("Source Parse Error: {}", e),
                    }
                },
                Err(e) => self.status_message = format!("File Read Error: {}", e),
            }
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
            match fs::File::create(&path) {
                Ok(file) => {
                    match bincode::serialize_into(file, &self.state) {
                        Ok(_) => {
                            self.current_file_path = Some(path);
                            self.status_message = "Document saved successfully.".to_string();
                        }
                        Err(e) => self.status_message = format!("Serialization Error: {}", e),
                    }
                }
                Err(e) => self.status_message = format!("File Create Error: {}", e),
            }
        }
    }

    // --- FIX IS HERE ---
    fn open_binary(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("WeaveLang Binary", &["wvl"])
            .pick_file()
        {
            match fs::File::open(&path) {
                Ok(file) => match bincode::deserialize_from(file) {
                    Ok(state) => {
                        // 1. Backup all runtime services from the current state
                        let bridge = self.state.bridge.clone();
                        let llm = self.state.llm.clone();
                        let prompts = self.state.prompts.clone();
                        let logger = self.state.logger.clone(); // <--- This was missing before!
                        
                        // 2. Overwrite state with data from disk
                        self.state = state;
                        
                        // 3. Restore services
                        self.state.bridge = bridge;
                        self.state.llm = llm;
                        self.state.prompts = prompts;
                        self.state.logger = logger; // <--- Restore logger
                        
                        self.current_file_path = Some(path);
                        self.status_message = format!("Loaded state.");
                    }
                    Err(e) => self.status_message = format!("Deserialization Error: {}", e),
                },
                Err(e) => self.status_message = format!("File Open Error: {}", e),
            }
        }
    }
}

impl App for WeaveLangApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            self.render_menu_bar(ui);
            ui.separator();
            components::top_bar::render(ui, &mut self.state);
        });

        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            components::info_bar::render(ui, &mut self.state);
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
    }
}