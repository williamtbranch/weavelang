// src/main.rs
//#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser;
use eframe::{egui, App as EframeApp, NativeOptions};
use std::fs;
use std::path::PathBuf;
use weavelang_rust_gui::{
    config, corpus_generator, parse_chapter_from_json, Config, GenerationArgs, JsonChapter,
};

// --- CLI Argument Structures (UPDATED) ---
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
    #[arg(short, long, value_name = "FILE", default_value = "config.toml")]
    config: PathBuf,
}

#[derive(Parser, Debug)]
enum Commands {
    Gui,
    Generate(GenerateCliArgs),
}

#[derive(Parser, Debug, Clone)]
struct GenerateCliArgs {
    #[arg(short, long, value_name = "FILE")]
    sequence: PathBuf,
    #[arg(long, value_name = "DIR")]
    input_json_dir: PathBuf,
    #[arg(long, value_name = "DIR")]
    tts_output_dir: PathBuf,
    #[arg(long, value_name = "DIR")]
    profiles_dir: PathBuf,
    #[arg(long, value_name = "FILE")]
    start_profile: Option<PathBuf>,
    #[arg(long, default_value_t = 200)]
    sentences_per_block: usize,
    
    // --- ARGUMENT RENAMED FOR CLARITY ---
    #[arg(long, default_value_t = 50)]
    max_words_to_add_per_block: u32,
    
    #[arg(long, default_value_t = 0.97)]
    target_ct_threshold: f32,
    #[arg(long, default_value_t = 100)]
    words_per_level: u32,
}

// --- GUI Application (Unchanged) ---
struct WeaveLangApp {
    config: Option<Config>,
    config_error: Option<String>,
    content_path_display: String,
    json_files: Vec<PathBuf>,
    selected_json_file: Option<PathBuf>,
    selected_file_raw_content: String,
    current_json_chapter: Option<JsonChapter>,
    parse_error_display: Option<String>,
}
impl WeaveLangApp {
    fn new(
        _cc: &eframe::CreationContext<'_>,
        app_config: Option<Config>,
        config_error_msg: Option<String>,
    ) -> Self {
        let content_path_display_val = match &app_config {
            Some(conf) => format!("Content Dir: {}", conf.content_project_dir),
            None => {
                config_error_msg.clone().unwrap_or_else(|| "Config load error.".to_string())
            }
        };
        Self {
            config: app_config,
            config_error: config_error_msg,
            content_path_display: content_path_display_val,
            json_files: Vec::new(),
            selected_json_file: None,
            selected_file_raw_content: String::new(),
            current_json_chapter: None,
            parse_error_display: None,
        }
    }
    fn reset_selected_file_data(&mut self) {
        self.selected_file_raw_content.clear();
        self.current_json_chapter = None;
        self.parse_error_display = None;
    }
    fn scan_final_json_directory_gui(&mut self) {
        self.json_files.clear();
        self.selected_json_file = None;
        self.parse_error_display = None;
        self.reset_selected_file_data();
        if let Some(conf) = &self.config {
            let json_path = PathBuf::from(&conf.content_project_dir).join("stage/stage7");
            if !json_path.is_dir() {
                self.parse_error_display =
                    Some(format!("Final JSON directory not found: {:?}", json_path));
                return;
            }
            match fs::read_dir(json_path) {
                Ok(entries) => {
                    for entry in entries.filter_map(Result::ok) {
                        let path = entry.path();
                        if path.is_file() && path.extension().map_or(false, |ext| ext == "json") {
                            self.json_files.push(path);
                        }
                    }
                    if self.json_files.is_empty() {
                        self.parse_error_display =
                            Some("No .json files found in stage/stage7 directory.".to_string());
                    }
                    self.json_files.sort();
                }
                Err(e) => {
                    self.parse_error_display = Some(format!("Failed to read JSON directory: {}", e));
                }
            }
        } else {
            self.parse_error_display = Some("Config not loaded, cannot scan.".to_string());
        }
    }
    fn load_selected_json_file_gui(&mut self, path_to_load: &PathBuf) {
        self.reset_selected_file_data();
        self.selected_json_file = Some(path_to_load.clone());
        match fs::read_to_string(path_to_load) {
            Ok(contents) => {
                self.selected_file_raw_content = contents.clone();
                match parse_chapter_from_json(&contents) {
                    Ok(parsed_chapter) => {
                        self.current_json_chapter = Some(parsed_chapter);
                    }
                    Err(e) => {
                        self.parse_error_display = Some(format!("Parse error: {}", e));
                    }
                }
            }
            Err(e) => {
                self.parse_error_display = Some(format!(
                    "Error loading file {:?}: {}",
                    path_to_load.file_name().unwrap_or_default(),
                    e
                ));
            }
        }
    }
}
impl EframeApp for WeaveLangApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("WeaveLang Tool GUI (Functionality Pending)");
            ui.label("The core simulation logic has been significantly refactored.");
            ui.label("Full GUI functionality will be re-integrated in a future update.");
        });
    }
}


// --- Main Function (UPDATED) ---
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let project_app_config_result =
        config::load_config_from_file(cli.config.to_str().unwrap_or("config.toml"));

    let (app_config_for_gui, config_error_msg_for_gui, config_for_generate_mode) =
        match project_app_config_result {
            Ok(loaded_config) => {
                println!("Successfully loaded project configuration from: {:?}", cli.config);
                (Some(loaded_config.clone()), None, Some(loaded_config))
            }
            Err(err_msg) => {
                eprintln!("Error loading project configuration from {:?}: {}", cli.config, err_msg);
                if matches!(cli.command, Some(Commands::Generate(_))) {
                    return Err(Box::new(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("Config load failed for generate mode: {}", err_msg),
                    )));
                }
                (None, Some(err_msg), None)
            }
        };

    match cli.command.unwrap_or(Commands::Gui) {
        Commands::Gui => {
            println!("Launching GUI mode...");
            let options = NativeOptions {
                viewport: egui::ViewportBuilder::default()
                    .with_inner_size([1200.0, 800.0])
                    .with_min_inner_size([800.0, 600.0]),
                ..Default::default()
            };

            eframe::run_native(
                "WeaveLang Tool",
                options,
                Box::new(move |cc| {
                    Box::new(WeaveLangApp::new(cc, app_config_for_gui, config_error_msg_for_gui))
                }),
            )?;
        }
        Commands::Generate(generate_cli_args) => {
            println!("[DEBUG] Matched 'Generate' command.");
            println!("Starting Corpus Generation mode...");
            println!("  Sequence: {:?}", generate_cli_args.sequence);
            println!("  Input JSON Dir: {:?}", generate_cli_args.input_json_dir);

            let final_config_for_generate = config_for_generate_mode
                .ok_or_else(|| "Project config is required for generate mode but was not available.".to_string())?;

            let corpus_gen_args = GenerationArgs {
                sequence_path: generate_cli_args.sequence,
                input_json_dir: generate_cli_args.input_json_dir,
                tts_output_dir: generate_cli_args.tts_output_dir,
                profiles_dir: generate_cli_args.profiles_dir,
                start_profile_path: generate_cli_args.start_profile,
                sentences_per_block: generate_cli_args.sentences_per_block,
                // --- PASSING NEW ARGUMENT ---
                max_words_to_add_per_block: generate_cli_args.max_words_to_add_per_block,
                target_ct_threshold: generate_cli_args.target_ct_threshold,
                words_per_level: generate_cli_args.words_per_level,
            };

            if let Err(e) = corpus_generator::run_corpus_generation(&final_config_for_generate, &corpus_gen_args) {
                eprintln!("Corpus generation failed: {}", e);
                std::process::exit(1);
            } else {
                println!("Corpus generation completed successfully.");
            }
        }
    }
    Ok(())
}