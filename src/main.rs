// src/main.rs
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::{Parser, ValueEnum};
use eframe::{egui, App as EframeApp, NativeOptions};
use std::path::PathBuf;
use weavelang_rust_gui::{
    config, corpus_generator, Config, GenerationArgs,
};
use weavelang_rust_gui::simulation::global_settings::{ForceLevel, FORCE_LEVEL_OVERRIDE};

// --- CLI Argument Structures ---

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

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliForceLevel {
    As,
}

#[derive(clap::Args, Debug, Clone)]
struct GenerateCliArgs {
    #[arg(short, long, value_name = "FILE", help="Path to the sequence.txt file listing books to process.")]
    sequence: PathBuf,

    #[arg(long, value_name = "DIR", help="Directory containing the final stage8.json files.")]
    input_json_dir: PathBuf,

    #[arg(long, value_name = "DIR", help="Directory to save the final generated TTS text files.")]
    tts_output_dir: PathBuf,

    #[arg(long, value_name = "DIR", help="Directory to save output profiles and analysis logs.")]
    profiles_dir: PathBuf,
    
    #[arg(long, default_value_t = 0, help="The vocabulary level to start the batch with (e.g., 12 for V12). Overridden by sequence file commands.")]
    start_level: u32,

    #[arg(long, default_value_t = 10.0, help="The baseline number of new words to introduce per hour of content (~9000 generated words). Overridden by sequence file commands.")]
    ramp_rate: f32,

    #[arg(long, default_value_t = 10, help="The number of vocabulary words that constitute a single level.")]
    words_per_level: u32,

    #[arg(long, default_value_t = 2000, help="The number of initial words from the frequency list subject to the slower, tapering ramp-up.")]
    core_vocab_size: u32,

    #[arg(long, default_value_t = 0.5, help="Threshold of progress (0.0-1.0) into the next level to attempt 'stretching'.")]
    stretch_threshold: f32,

    #[arg(long, default_value_t = 0.15, help="Maximum compression ratio (e.g., 0.15 for 15%) allowed when stretching before reverting to rounding down.")]
    max_compression_ratio: f32,

    #[arg(long, value_enum, hide = true)]
    force_level: Option<CliForceLevel>,

    // --- ADDED THIS FLAG ---
    #[arg(long, help = "Add (%%...%%) and (%ED%...%) markers to the output for debugging.")]
    debug_markers: bool,
}

// --- GUI Application (Unchanged) ---
struct WeaveLangApp {
    _config: Option<Config>,
    _config_error: Option<String>,
    _content_path_display: String,
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
            _config: app_config,
            _config_error: config_error_msg,
            _content_path_display: content_path_display_val,
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

// --- Main Function ---
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
            // ... GUI logic ...
        }
        Commands::Generate(generate_cli_args) => {
            // ... generate logic ...
            let final_config_for_generate = config_for_generate_mode
                .ok_or_else(|| "Project config is required for generate mode but was not available.".to_string())?;
                
            let corpus_gen_args = GenerationArgs {
                sequence_path: generate_cli_args.sequence,
                input_json_dir: generate_cli_args.input_json_dir,
                tts_output_dir: generate_cli_args.tts_output_dir,
                profiles_dir: generate_cli_args.profiles_dir,
                start_level: generate_cli_args.start_level,
                ramp_rate: generate_cli_args.ramp_rate,
                words_per_level: generate_cli_args.words_per_level,
                core_vocab_size: generate_cli_args.core_vocab_size,
                stretch_threshold: generate_cli_args.stretch_threshold,
                max_compression_ratio: generate_cli_args.max_compression_ratio,
                // --- ADDED THIS LINE ---
                debug_markers: generate_cli_args.debug_markers,
            };

            if let Err(e) = corpus_generator::run_corpus_generation(&final_config_for_generate, &corpus_gen_args) {
                // ... error handling ...
            }
        }
    }
    Ok(())
}