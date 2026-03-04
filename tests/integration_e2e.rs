// tests/integration_e2e.rs
//
// End-to-end integration test: drives the full pipeline through the terminal
// command interface using MockLlmProvider and canned responses.
//
// Marked #[ignore] so `cargo test` skips it by default.
// Run explicitly:
//   cargo test --test integration_e2e -- --ignored
// Or include everything:
//   cargo test -- --include-ignored

use std::path::PathBuf;

use weavelang_rust_gui::types::json_types::{JsonChapter, JsonContentBlock};
use weavelang_rust_gui::app::engine::Engine;
use weavelang_rust_gui::app::state::AppState;
use weavelang_rust_gui::app::terminal::run_terminal_command;
use weavelang_rust_gui::services::llm_client::LlmService;
use weavelang_rust_gui::services::llm_logger::LlmLogger;
use weavelang_rust_gui::services::mock_llm::MockLlmProvider;
use weavelang_rust_gui::services::prompt_manager::PromptManager;

/// Resolve the test_case directory relative to the workspace root.
fn test_case_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.join("test_case").join("test_01")
}

/// Build an Engine wired to MockLlmProvider and the project's real prompt files.
fn build_test_engine() -> Engine {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let test_dir = test_case_dir();

    let mut state = AppState::default();

    // MockLlmProvider pointed at canned responses
    let responses_dir = test_dir.join("LLM_responses");
    let mock = MockLlmProvider::new(responses_dir);
    state.llm = Some(LlmService::from_provider(Box::new(mock)));

    // PromptManager needs the project root so it can find assets/prompts/
    state.prompts = Some(PromptManager::new(root.clone()));

    // LlmLogger — write to a temp dir so we don't pollute the project
    let log_dir = test_dir.join("test_temp_logs");
    let _ = std::fs::create_dir_all(&log_dir);
    state.logger = Some(LlmLogger::new(log_dir));

    // Config — try loading; non-fatal if missing
    let config_path = root.join("config.toml");
    if let Ok(cfg) = weavelang_rust_gui::config::load_config_from_file(
        config_path.to_str().unwrap_or("config.toml"),
    ) {
        state.config = Some(cfg);
    }

    // Try to initialise the Python bridge.  If Python / the venv is not
    // available we skip it — import will fall back to parse_source_file()
    // which needs {S1: …} markup.  Without the bridge AND without markup
    // the import step will fail and the test will report why.
    match weavelang_rust_gui::services::python_bridge::BridgeService::new(root.clone()) {
        Ok(b) => state.bridge = Some(b),
        Err(e) => eprintln!("[integration_e2e] Bridge not available: {e}"),
    }

    // Load frequency list (best-effort)
    let freq_path = root.join("assets/frequency_lists/es_master_frequency_list.txt");
    if freq_path.exists() {
        let _ = weavelang_rust_gui::simulation::frequency_manager::load_master_frequency_list(&freq_path);
    }

    Engine::new(state)
}

/// Feed a single terminal command and return the output string.
/// Panics on error so each step acts as an assertion.
fn exec(engine: &mut Engine, cmd: &str) -> String {
    match run_terminal_command(engine, cmd) {
        Ok(Some(output)) => output,
        Ok(None) => panic!("Unexpected Exit from command: {cmd}"),
        Err(e) => panic!("Command failed: [{cmd}] → {e}"),
    }
}

/// Read test_batch.txt lines, skipping comments and blanks.
fn load_batch_commands() -> Vec<String> {
    let batch_path = test_case_dir().join("test_batch.txt");
    let content = std::fs::read_to_string(&batch_path)
        .unwrap_or_else(|e| panic!("Cannot read {}: {e}", batch_path.display()));

    content
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
}

/// Rewrite relative paths in certain commands to absolute paths rooted at the
/// test_case directory.  e.g. `import source source_input.txt` becomes
/// `import source E:\...\test_case\test_01\source_input.txt`.
fn resolve_paths(cmd: &str, test_dir: &PathBuf) -> String {
    let parts: Vec<&str> = cmd.splitn(3, ' ').collect();
    match (parts.get(0), parts.get(1)) {
        // import source <relative_path>
        (Some(&"import"), Some(&"source")) if parts.len() == 3 => {
            let abs = test_dir.join(parts[2]);
            format!("import source {}", abs.display())
        }
        // import level_map <relative_path>
        (Some(&"import"), Some(&"level_map")) if parts.len() == 3 => {
            let abs = test_dir.join(parts[2]);
            format!("import level_map {}", abs.display())
        }
        // set output_dir <relative_path>
        (Some(&"set"), Some(&"output_dir")) if parts.len() == 3 => {
            let abs = test_dir.join(parts[2]);
            format!("set output_dir {}", abs.display())
        }
        // debug_dump <s> <e> <relative_path>
        (Some(&"debug_dump"), _) => {
            let sub_parts: Vec<&str> = cmd.split_whitespace().collect();
            if sub_parts.len() > 3 {
                let abs = test_dir.join(sub_parts[3..].join(" "));
                format!("debug_dump {} {} {}", sub_parts[1], sub_parts[2], abs.display())
            } else {
                cmd.to_string()
            }
        }
        _ => cmd.to_string(),
    }
}

#[test]
#[ignore]
fn test_01_full_pipeline() {
    let test_dir = test_case_dir();
    let mut engine = build_test_engine();

    let commands = load_batch_commands();
    assert!(!commands.is_empty(), "test_batch.txt yielded no commands");

    let mut outputs: Vec<(String, String)> = Vec::new();

    for raw_cmd in &commands {
        let cmd = resolve_paths(raw_cmd, &test_dir);
        eprintln!("[e2e] > {}", cmd);
        let out = exec(&mut engine, &cmd);
        eprintln!("[e2e]   {}", out);
        outputs.push((cmd.clone(), out));
    }

    // --- Assertions ---

    // 1. We should have imported sentences
    assert!(
        !engine.state.document.is_empty(),
        "Document is empty after import"
    );

    // 2. Every non-header sentence should have at minimum a base tier and
    //    the tiers produced by the LLM stages.
    let expected_tiers = ["base", "basic_base", "advanced_target", "basic_target", "moderate_target"];
    for (i, sent) in engine.state.document.iter().enumerate() {
        for tier_name in &expected_tiers {
            assert!(
                sent.get_tier(tier_name).is_some(),
                "Sentence {} ({}) missing tier '{}'",
                i, sent.id, tier_name,
            );
        }
    }

    // 3. The debug_dump command should have produced a file.
    let dump_path = test_dir.join("expects").join("debug_dump_test.txt");
    assert!(
        dump_path.exists(),
        "Expected debug dump file at {}",
        dump_path.display()
    );

    // 4. Verify Exported JSON against Golden JSON
    let weave_dir = test_dir.join("weave_output");
    let exported_json_path = weave_dir.join("exported.json");
    assert!(
        exported_json_path.exists(),
        "Expected exported JSON at {}",
        exported_json_path.display()
    );

    let golden_json_path = test_dir.parent().unwrap().join("Metamorphosis_12.json");
    let golden_str = std::fs::read_to_string(&golden_json_path).expect("Could not read golden JSON");
    let exported_str = std::fs::read_to_string(&exported_json_path).expect("Could not read exported JSON");

    let expected: JsonChapter = serde_json::from_str(&golden_str).expect("Failed to parse golden JSON");
    let actual: JsonChapter = serde_json::from_str(&exported_str).expect("Failed to parse exported JSON");

    let expected_sentences: Vec<_> = expected.content_blocks.iter().filter(|b| matches!(b, JsonContentBlock::Sentence(_))).collect();
    let actual_sentences: Vec<_> = actual.content_blocks.iter().filter(|b| matches!(b, JsonContentBlock::Sentence(_))).collect();

    assert_eq!(
        expected_sentences.len(),
        actual_sentences.len(),
        "Mismatch in number of sentence blocks!"
    );

    for (e_s, a_s) in expected_sentences.into_iter().zip(actual_sentences.into_iter()) {
        if let (JsonContentBlock::Sentence(e_sent), JsonContentBlock::Sentence(a_sent)) = (e_s, a_s) {
            assert_eq!(e_sent.s_id, a_sent.s_id, "Sentence ID mismatch");
            for e_tier in &e_sent.tiers {
                let a_tier = a_sent.tiers.iter().find(|t| t.tier_id == e_tier.tier_id)
                    .unwrap_or_else(|| panic!("Tier {} missing in sentence {}", e_tier.tier_id, e_sent.s_id));
                assert_eq!(
                    e_tier.full_text, 
                    a_tier.full_text,
                    "Text mismatch in tier {} of sentence {}", e_tier.tier_id, e_sent.s_id
                );
            }
        }
    }
    eprintln!("[e2e] JSON Chapter comparison: PASS");

    // Clean up generated test output
    let _ = std::fs::remove_file(&dump_path);
    let _ = std::fs::remove_dir_all(test_dir.join("test_temp_logs"));
    let _ = std::fs::remove_dir_all(test_dir.join("weave_output"));
}
