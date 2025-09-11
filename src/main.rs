use clap::Parser;
use std::path::PathBuf;
use weavelang_rust_gui::{
    config, corpus_generator, Config,
};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Parser, Debug)]
enum Commands {
    Generate(GenerateCliArgs),
}

#[derive(clap::Args, Debug, Clone)]
struct GenerateCliArgs {
    #[arg(long, value_name = "DIR", help="Path to the tool's root directory, for finding assets.")]
    tool_root_dir: PathBuf,
    #[arg(short, long, value_name = "FILE", help="Path to the sequence.txt file listing books to process.")]
    sequence: PathBuf,
    #[arg(long, value_name = "DIR", help="Directory containing the final JSON files (relative to content_project_dir).")]
    input_json_dir: PathBuf,
    #[arg(long, value_name = "DIR", help="Directory to save the final generated TTS text files.")]
    tts_output_dir: PathBuf,
    #[arg(long, value_name = "DIR", help="Directory to save output profiles and analysis logs.")]
    profiles_dir: PathBuf,
    #[arg(long, default_value_t = 0.4, help="Max ratio of substitutions to total words in an L0 sentence to be considered a valid inverse diglot.")]
    inverse_diglot_threshold: f32,
    #[arg(long, help = "Add (%%...%%) markers to the output for debugging.")]
    debug_markers: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    
    // Hardcoded config path for CLI-only operation
    let config_path = "config.toml";
    let project_config = config::load_config_from_file(config_path)
        .map_err(|e| format!("Failed to load project configuration from '{}': {}", config_path, e))?;

    match cli.command {
        Commands::Generate(args) => {
            if let Err(e) = corpus_generator::run_corpus_generation(
                &project_config,
                &args.tool_root_dir,
                &args.sequence,
                &args.input_json_dir,
                &args.tts_output_dir,
                &args.profiles_dir,
                args.debug_markers,
                args.inverse_diglot_threshold,
            ) {
                eprintln!("[ERROR] Corpus generation failed: {}", e);
                std::process::exit(1);
            }
        }
    }
    Ok(())
}