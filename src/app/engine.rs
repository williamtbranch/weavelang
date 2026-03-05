use crate::app::commands::AppCommand;
use crate::app::state::AppState;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use crate::parsing::source_parser;
use crate::types::json_types::JsonChapter;

pub struct Engine {
    pub state: AppState,
    pub current_file_path: Option<PathBuf>,
}

impl Engine {
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            current_file_path: None,
        }
    }

    pub fn execute(&mut self, command: AppCommand) -> Result<String, String> {
        match command {
            AppCommand::SelectSentence { id, index } => {
                if let Some(idx) = index {
                    if idx < self.state.document.len() {
                        self.state.selected_sentence_idx = idx;
                        self.state.selected_range = None;
                        return Ok(format!("Selected sentence index {}", idx));
                    }
                    return Err("Index out of bounds".to_string());
                } else if let Some(sid) = id {
                    if let Some(idx) = self.state.document.iter().position(|s| s.id == sid) {
                        self.state.selected_sentence_idx = idx;
                        self.state.selected_range = None;
                        return Ok(format!("Selected sentence id {}", sid));
                    }
                    return Err("Sentence ID not found".to_string());
                }
                Err("Must provide id or index".to_string())
            }
            AppCommand::SelectRange { start_id: _start_id, end_id: _end_id, start_index: _start_index, end_index: _end_index } => {
                // TODO: Implement range selection
                Ok("Range selected".to_string())
            }
            AppCommand::SetRightView { view } => {
                use crate::app::state::{DetailView, TierView};
                match view.as_str() {
                    "base" => self.state.right_view = DetailView::Tier(TierView::Base),
                    "advanced_target" => self.state.right_view = DetailView::Tier(TierView::AdvancedTarget),
                    "moderate_target" => self.state.right_view = DetailView::Tier(TierView::ModerateTarget),
                    "basic_target" => self.state.right_view = DetailView::Tier(TierView::BasicTarget),
                    "basic_base" => self.state.right_view = DetailView::Tier(TierView::BasicBase),
                    "simulation" => self.state.right_view = DetailView::Tier(TierView::Simulation),
                    "token_base" => self.state.right_view = DetailView::Token(TierView::Base),
                    "token_advanced_target" => self.state.right_view = DetailView::Token(TierView::AdvancedTarget),
                    "token_moderate_target" => self.state.right_view = DetailView::Token(TierView::ModerateTarget),
                    "token_basic_target" => self.state.right_view = DetailView::Token(TierView::BasicTarget),
                    "token_basic_base" => self.state.right_view = DetailView::Token(TierView::BasicBase),
                    "token_simulation" => self.state.right_view = DetailView::Token(TierView::Simulation),
                    "mapping_diglot" => self.state.right_view = DetailView::MappingDiglot,
                    "mapping_inverse" => self.state.right_view = DetailView::MappingInverse,
                    _ => return Err(format!("Unknown view: {}", view)),
                }
                Ok(format!("Right view set to {}", view))
            }
            AppCommand::SetLeftView { view } => {
                use crate::app::state::TierView;
                match view.as_str() {
                    "base" => self.state.left_view = TierView::Base,
                    "advanced_target" => self.state.left_view = TierView::AdvancedTarget,
                    "moderate_target" => self.state.left_view = TierView::ModerateTarget,
                    "basic_target" => self.state.left_view = TierView::BasicTarget,
                    "basic_base" => self.state.left_view = TierView::BasicBase,
                    "simulation" => self.state.left_view = TierView::Simulation,
                    _ => return Err(format!("Unknown view: {}", view)),
                }
                Ok(format!("Left view set to {}", view))
            }
            AppCommand::AddSentence => {
                use crate::domain::sentence::Sentence;
                use crate::domain::tier::Tier;
                use crate::domain::segment::Segment;
                use crate::domain::token_stream::TokenStream;

                let new_id = format!("S{}", self.state.document.len() + 1);
                let mut sentence = Sentence::new(new_id.clone());

                let mut tier = Tier::new("base".to_string());
                tier.add_segment(Segment::from_stream(
                    "S1".to_string(),
                    TokenStream::new(""),
                    vec![],
                ));
                sentence.add_tier(tier);

                self.state.document.push(sentence);
                self.state.selected_sentence_idx = self.state.document.len() - 1;
                self.state.selected_range = None;
                Ok(format!("Added new sentence {}", new_id))
            }
            AppCommand::UpdateText { sentence_id, index, tier_id, new_text } => {
                let idx = if let Some(i) = index {
                    i
                } else if let Some(sid) = sentence_id {
                    self.state.document.iter().position(|s| s.id == sid).ok_or("Sentence ID not found")?
                } else {
                    return Err("Must provide id or index".to_string());
                };

                if let Some(sent) = self.state.document.get_mut(idx) {
                    sent.update_tier_text(&tier_id, new_text);
                    Ok(format!("Updated text for sentence index {}, tier {}", idx, tier_id))
                } else {
                    Err("Index out of bounds".to_string())
                }
            }
            AppCommand::ApproveEdits { sentence_id, index, tier_id } => {
                let idx = if let Some(i) = index {
                    i
                } else if let Some(sid) = sentence_id {
                    self.state.document.iter().position(|s| s.id == sid).ok_or("Sentence ID not found")?
                } else {
                    return Err("Must provide id or index".to_string());
                };

                if let Some(sent) = self.state.document.get_mut(idx) {
                    if let Some(tier) = sent.get_tier(&tier_id) {
                        let text = tier.full_text();
                        sent.update_tier_text_as_clean(&tier_id, text);
                        Ok(format!("Approved edits for sentence index {}, tier {}", idx, tier_id))
                    } else {
                        Err("Tier not found".to_string())
                    }
                } else {
                    Err("Index out of bounds".to_string())
                }
            }
            AppCommand::GenerateTier { sentence_id: _sentence_id, index: _index, tier_id: _tier_id } => {
                // TODO: Implement tier generation
                Ok("Tier generation started".to_string())
            }
            AppCommand::GenerateMapping { sentence_id: _sentence_id, index: _index, source_tier: _source_tier, target_tier: _target_tier } => {
                // TODO: Implement mapping generation
                Ok("Mapping generation started".to_string())
            }
            AppCommand::OpenWorkspace { path } => {
                let workspace_path = std::path::PathBuf::from(&path);

                // Create directory if it doesn't exist (allows bootstrapping a new workspace)
                if !workspace_path.exists() {
                    std::fs::create_dir_all(&workspace_path)
                        .map_err(|e| format!("Cannot create workspace directory: {e}"))?;
                }

                if !workspace_path.is_dir() {
                    return Err(format!("'{}' is not a directory", path));
                }

                let config_path = workspace_path.join("config.toml");
                let config = if config_path.exists() {
                    crate::config::load_config_from_file(config_path.to_str().unwrap_or(""))?
                } else {
                    // Scaffold a default config.toml for the new workspace
                    let mut cfg = crate::config::Config::default();
                    cfg.content_project_dir = path.clone();
                    let toml_content = toml::to_string_pretty(&cfg)
                        .map_err(|e| format!("Cannot serialise default config: {e}"))?;
                    std::fs::write(&config_path, &toml_content)
                        .map_err(|e| format!("Cannot write config.toml: {e}"))?;
                    cfg
                };

                // Persist last-used workspace for auto-load on next launch
                let mut gs = crate::global_settings::GlobalSettings::load();
                gs.set_workspace(&path);
                let _ = gs.save();

                // Point the LLM logger to the workspace directory
                self.state.logger = Some(crate::services::llm_logger::LlmLogger::new(
                    std::path::PathBuf::from(&path),
                ));

                // Sync model definitions to the routing provider
                if let Some(llm) = &self.state.llm {
                    llm.update_models(config.models.clone());
                }

                self.state.config = Some(config);
                Ok(format!("Workspace opened: {}", path))
            }
            AppCommand::LoadProject { path } => {
                let path_buf = PathBuf::from(&path);
                if let Ok(bytes) = fs::read(&path_buf) {
                    if let Ok(mut loaded_state) = bincode::deserialize::<AppState>(&bytes) {
                        // Restore runtime services
                        loaded_state.bridge = self.state.bridge.clone();
                        loaded_state.llm = self.state.llm.clone();
                        loaded_state.prompts = self.state.prompts.clone();
                        loaded_state.logger = self.state.logger.clone();
                        
                        // Workspace config should NOT be overwritten by the .wvl file
                        loaded_state.config = self.state.config.clone();

                        // Default batch size from stage
                        if let Some(cfg) = &self.state.config {
                            if let Some(stage) = cfg.stages.get("GenerateBasicBase") {
                                loaded_state.llm_run_batch_size = stage.batch_size_in_items;
                            }
                        }

                        self.state = loaded_state;
                        self.current_file_path = Some(path_buf);

                        if let Some(cfg) = &mut self.state.config {
                            cfg.last_project_file = Some(path.clone());
                            let config_path = PathBuf::from(&cfg.content_project_dir).join("config.toml");
                            let _ = crate::config::save_config_to_file(cfg, &config_path);
                        }

                        return Ok(format!("Loaded project from {}", path));
                    }
                    return Err("Failed to deserialize project".to_string());
                }
                Err(format!("Failed to read file {}", path))
            }
            AppCommand::SaveProject { path } => {
                let save_path = if let Some(p) = path {
                    PathBuf::from(p)
                } else if let Some(p) = &self.current_file_path {
                    p.clone()
                } else {
                    return Err("No path provided and no current file path".to_string());
                };

                if let Ok(bytes) = bincode::serialize(&self.state) {
                    if fs::write(&save_path, bytes).is_ok() {
                        self.current_file_path = Some(save_path.clone());

                        if let Some(cfg) = &mut self.state.config {
                            cfg.last_project_file = Some(save_path.to_string_lossy().to_string());
                            let config_path = PathBuf::from(&cfg.content_project_dir).join("config.toml");
                            let _ = crate::config::save_config_to_file(cfg, &config_path);
                        }

                        return Ok(format!("Saved project to {:?}", save_path));
                    }
                    return Err("Failed to write file".to_string());
                }
                Err("Failed to serialize project".to_string())
            }
            AppCommand::ImportSource { path } => {
                let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
                let sentences = source_parser::parse_source_file(&content).map_err(|e| e.to_string())?;
                
                if !sentences.is_empty() {
                    self.state.document = sentences;
                } else if let Some(bridge) = &self.state.bridge {
                    let path_buf = PathBuf::from(&path);
                    let book_name = path_buf.file_name().and_then(|s| s.to_str()).unwrap_or("Unnamed");
                    
                    let chap = crate::services::importer::BookImporter::import_from_text_with_service(
                        &content,
                        book_name,
                        bridge,
                    )?;
                    
                    self.state.document.clear();
                    for block in chap.content_blocks {
                        if let crate::types::json_types::JsonContentBlock::Sentence(json_sentence) = block {
                            match crate::domain::bridge::json_to_domain_sentence(&json_sentence) {
                                Ok(domain_sentence) => self.state.document.push(domain_sentence),
                                Err(e) => eprintln!("Skipping invalid sentence: {e}"),
                            }
                        }
                    }
                } else {
                    return Err("No recognizable sentence markup and Python bridge not configured.".to_string());
                }
                
                self.state.book_map = None;
                self.state.book_name = PathBuf::from(&path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Unnamed")
                    .to_string();
                self.state.selected_sentence_idx = 0;
                self.state.selected_range = None;
                Ok(format!("Imported {} sentences from source", self.state.document.len()))
            }
            AppCommand::ImportJson { path } => {
                let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
                let chapter: JsonChapter = serde_json::from_str(&content).map_err(|e| e.to_string())?;
                
                self.state.document.clear();
                self.state.book_map = Some(chapter.u_level_maps.clone());
                self.state.book_name = chapter.book_meta.book_name.clone();
                self.state.project_languages = (
                    chapter.book_meta.base_language.clone(),
                    chapter.book_meta.target_language.clone(),
                );

                let mut error_count = 0;
                for block in chapter.content_blocks {
                    if let crate::types::json_types::JsonContentBlock::Sentence(json_sentence) = block {
                        match crate::domain::bridge::json_to_domain_sentence(&json_sentence) {
                            Ok(domain_sentence) => self.state.document.push(domain_sentence),
                            Err(e) => {
                                eprintln!("Skipping invalid sentence: {e}");
                                error_count += 1;
                            }
                        }
                    }
                }

                self.state.selected_sentence_idx = 0;
                self.state.selected_range = None;
                Ok(format!("Imported {} sentences from JSON ({} errors)", self.state.document.len(), error_count))
            }
            AppCommand::ExportJson { path } => {
                self.execute_export_json(&path)
            }
            AppCommand::ExportLevelMap { path } => {
                self.execute_export_level_map(&path)
            }
            AppCommand::ImportLevelMap { path } => {
                self.execute_import_level_map(&path)
            }
            AppCommand::SetOutputDir { path } => {
                let dir = PathBuf::from(&path);
                if !dir.exists() {
                    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create directory '{}': {}", path, e))?;
                }
                self.state.output_dir = Some(path.clone());
                Ok(format!("Output directory set to '{}'", path))
            }
            AppCommand::GenerateWeave { level } => {
                self.execute_generate_weave(&level)
            }
            AppCommand::ConfigSet { key, value } => {
                if let Some(config) = &mut self.state.config {
                    let parts: Vec<&str> = key.split('.').collect();
                    if parts.len() == 1 {
                        match parts[0] {
                            "open_last_project" => {
                                if let Ok(v) = value.parse::<bool>() {
                                    config.open_last_project = Some(v);
                                    Ok(format!("Updated open_last_project to {}", v))
                                } else {
                                    Err("Invalid boolean".to_string())
                                }
                            }
                            "custom_frequency_list_path" => {
                                if value.trim().is_empty() || value == "none" {
                                    config.custom_frequency_list_path = None;
                                    Ok("Clear custom_frequency_list_path".to_string())
                                } else {
                                    config.custom_frequency_list_path = Some(value.clone());
                                    Ok(format!("Updated custom_frequency_list_path to {}", value))
                                }
                            }
                            _ => Err(format!("Unknown root field: {}", parts[0]))
                        }
                    } else if parts.len() == 2 && parts[0] == "pipeline" {
                        let field = parts[1];
                        match field {
                            "max_api_retries" => {
                                if let Ok(v) = value.parse::<u32>() {
                                    config.pipeline.max_api_retries = v;
                                    Ok(format!("Updated pipeline.max_api_retries to {}", v))
                                } else { Err("Invalid u32".to_string()) }
                            }
                            "max_validation_retries" => {
                                if let Ok(v) = value.parse::<u32>() {
                                    config.pipeline.max_validation_retries = v;
                                    Ok(format!("Updated pipeline.max_validation_retries to {}", v))
                                } else { Err("Invalid u32".to_string()) }
                            }
                            "retry_delay" => {
                                if let Ok(v) = value.parse::<u32>() {
                                    config.pipeline.retry_delay = v;
                                    Ok(format!("Updated pipeline.retry_delay to {}", v))
                                } else { Err("Invalid u32".to_string()) }
                            }
                            "thinking_budget_tokens" => {
                                if value.trim().is_empty() || value == "none" {
                                    config.pipeline.thinking_budget_tokens = None;
                                    Ok("Cleared pipeline.thinking_budget_tokens".to_string())
                                } else if let Ok(v) = value.parse::<u32>() {
                                    config.pipeline.thinking_budget_tokens = Some(v);
                                    Ok(format!("Updated pipeline.thinking_budget_tokens to {}", v))
                                } else { Err("Invalid u32".to_string()) }
                            }
                            _ => Err(format!("Unknown pipeline field: {}", field)),
                        }
                    } else if parts.len() == 3 && parts[0] == "stages" {
                        let stage_name = parts[1];
                        let field = parts[2];
                        if let Some(stage) = config.stages.get_mut(stage_name) {
                            match field {
                                "primary_model" => {
                                    stage.primary_model = value.clone();
                                    Ok(format!("Updated {}.primary_model to {}", stage_name, value))
                                }
                                "fallback_model" => {
                                    if value.trim().is_empty() || value == "none" {
                                        stage.fallback_model = None;
                                        Ok(format!("Cleared {}.fallback_model", stage_name))
                                    } else {
                                        stage.fallback_model = Some(value.clone());
                                        Ok(format!("Updated {}.fallback_model to {}", stage_name, value))
                                    }
                                }
                                "batch_size_in_items" => {
                                    if let Ok(v) = value.parse::<usize>() {
                                        stage.batch_size_in_items = v;
                                        Ok(format!("Updated {}.batch_size_in_items to {}", stage_name, v))
                                    } else {
                                        Err("Invalid number".to_string())
                                    }
                                }
                                "thinking_budget_tokens" => {
                                    if value.trim().is_empty() || value == "none" {
                                        stage.thinking_budget_tokens = None;
                                        Ok(format!("Cleared {}.thinking_budget_tokens", stage_name))
                                    } else if let Ok(v) = value.parse::<u32>() {
                                        stage.thinking_budget_tokens = Some(v);
                                        Ok(format!("Updated {}.thinking_budget_tokens to {}", stage_name, v))
                                    } else { Err("Invalid u32".to_string()) }
                                }
                                "thinking_on_first_attempt" => {
                                    if value.trim().is_empty() || value == "none" {
                                        stage.thinking_on_first_attempt = None;
                                        Ok(format!("Cleared {}.thinking_on_first_attempt", stage_name))
                                    } else if let Ok(v) = value.parse::<bool>() {
                                        stage.thinking_on_first_attempt = Some(v);
                                        Ok(format!("Updated {}.thinking_on_first_attempt to {}", stage_name, v))
                                    } else { Err("Invalid boolean".to_string()) }
                                }
                                _ => Err(format!("Unknown stage field: {}", field)),
                            }
                        } else {
                            Err(format!("Stage '{}' not found", stage_name))
                        }
                    } else if parts.len() == 3 && parts[0] == "models" {
                        let model_name = parts[1];
                        let field = parts[2];
                        if let Some(model) = config.models.get_mut(model_name) {
                            let result = match field {
                                "provider" => {
                                    model.provider = value.clone();
                                    Ok(format!("Updated {}.provider to {}", model_name, value))
                                }
                                "name" => {
                                    model.name = value.clone();
                                    Ok(format!("Updated {}.name to {}", model_name, value))
                                }
                                "max_input_tokens" => {
                                    if let Ok(v) = value.parse::<usize>() {
                                        model.max_input_tokens = v;
                                        Ok(format!("Updated {}.max_input_tokens to {}", model_name, v))
                                    } else {
                                        Err("Invalid number".to_string())
                                    }
                                }
                                _ => Err(format!("Unknown model field: {}", field)),
                            };
                            // Sync updated model definitions to the routing provider
                            if result.is_ok() {
                                if let Some(llm) = &self.state.llm {
                                    llm.update_models(config.models.clone());
                                }
                            }
                            result
                        } else {
                            Err(format!("Model '{}' not found", model_name))
                        }
                    } else {
                        Err("Invalid key format.".to_string())
                    }
                } else {
                    Err("Config not loaded".to_string())
                }
            }
            AppCommand::ConfigList => {
                 if let Some(config) = &self.state.config {
                     // Pretty print config
                     let toml_str = toml::to_string_pretty(config).map_err(|e| e.to_string())?;
                     Ok(toml_str)
                 } else {
                     Err("Config not loaded".to_string())
                 }
            }
            AppCommand::ConfigAddModel { alias } => {
                if let Some(config) = &mut self.state.config {
                    if config.models.contains_key(&alias) {
                        return Err(format!("Model alias '{}' already exists", alias));
                    }
                    config.models.insert(alias.clone(), crate::config::ModelConfig {
                        provider: String::new(),
                        name: String::new(),
                        max_input_tokens: 10000,
                    });
                    if let Some(llm) = &self.state.llm {
                        llm.update_models(config.models.clone());
                    }
                    Ok(format!("Added model '{}'", alias))
                } else {
                    Err("Config not loaded".to_string())
                }
            }
            AppCommand::ConfigRemoveModel { alias } => {
                if let Some(config) = &mut self.state.config {
                    if config.models.remove(&alias).is_some() {
                        if let Some(llm) = &self.state.llm {
                            llm.update_models(config.models.clone());
                        }
                        Ok(format!("Removed model '{}'", alias))
                    } else {
                        Err(format!("Model alias '{}' not found", alias))
                    }
                } else {
                    Err("Config not loaded".to_string())
                }
            }
            AppCommand::ConfigRenameModel { old_alias, new_alias } => {
                if let Some(config) = &mut self.state.config {
                    if !config.models.contains_key(&old_alias) {
                        return Err(format!("Model alias '{}' not found", old_alias));
                    }
                    if config.models.contains_key(&new_alias) {
                        return Err(format!("Model alias '{}' already exists", new_alias));
                    }
                    if let Some(model_cfg) = config.models.remove(&old_alias) {
                        config.models.insert(new_alias.clone(), model_cfg);
                        // Update any stages that reference the old alias
                        for stage in config.stages.values_mut() {
                            if stage.primary_model == old_alias {
                                stage.primary_model = new_alias.clone();
                            }
                            if stage.fallback_model.as_ref() == Some(&old_alias) {
                                stage.fallback_model = Some(new_alias.clone());
                            }
                        }
                        if let Some(llm) = &self.state.llm {
                            llm.update_models(config.models.clone());
                        }
                        Ok(format!("Renamed model '{}' to '{}'", old_alias, new_alias))
                    } else {
                        Err(format!("Model alias '{}' not found", old_alias))
                    }
                } else {
                    Err("Config not loaded".to_string())
                }
            }
            AppCommand::CheckStatus => {
                let bridge_status = if self.state.bridge.is_some() { "OK" } else { "OFF" };
                let llm_status = if self.state.llm.is_some() { "OK" } else { "OFF" };
                let config_status = if self.state.config.is_some() { "OK" } else { "OFF" };
                Ok(format!("Bridge: {}\nLLM: {}\nConfig: {}", bridge_status, llm_status, config_status))
            }
            AppCommand::GenerateStage { stage_name, start_index, end_index } => {
                if self.state.llm.is_none() || self.state.prompts.is_none() || self.state.logger.is_none() {
                    return Err("LLM pipeline services not ready (prompts or logger missing)".to_string());
                }
                if self.state.config.is_none() {
                    return Err("Config not loaded".to_string());
                }
                
                let config = self.state.config.as_ref().unwrap();
                let stage_config = config.get_stage_config(&stage_name).ok_or(format!("Stage '{}' not found in config", stage_name))?;
                
                let batch_size = stage_config.batch_size_in_items;
                let model_alias = stage_config.primary_model.clone();
                let fallback_alias = stage_config.fallback_model.clone();

                // Validate that the model aliases exist in [models] before spawning
                let model_cfg = config.get_model_config(&model_alias).ok_or_else(|| {
                    format!(
                        "Primary model alias '{}' for stage '{}' not found in [models] section of config.toml.\n\
                         Available models: [{}]",
                        model_alias,
                        stage_name,
                        config.models.keys().cloned().collect::<Vec<_>>().join(", ")
                    )
                })?;
                let model_display = format!("{} (alias '{}', provider: {})", model_cfg.name, model_alias, model_cfg.provider);

                if let Some(ref fb) = fallback_alias {
                    if config.get_model_config(fb).is_none() {
                        return Err(format!(
                            "Fallback model alias '{}' for stage '{}' not found in [models] section of config.toml.\n\
                             Available models: [{}]",
                            fb,
                            stage_name,
                            config.models.keys().cloned().collect::<Vec<_>>().join(", ")
                        ));
                    }
                }
                
                // Map Stage Name to Prompt Name and Target Tier
                // This logic should ideally be in a domain service, but hardcoding map here for now based on context
                let (prompt_name, target_tier, source_tier) = match stage_name.as_str() {
                    "GenerateBasicBase" => ("simplify_to_basic_english", "basic_base", "base"),
                    "GenerateBasicTarget" => ("translate_text_basic", "basic_target", "basic_base"),
                    "GenerateAdvancedTarget" => ("translate_text", "advanced_target", "base"),
                    "GenerateModerateTarget" => ("simplify_segments", "moderate_target", "advanced_target"),
                    "GeneratePhraseMap" => ("generate_phrase_map", "MAPPING:basic_base:basic_target", "basic_base"),
                    "GenerateInversePhraseMap" => ("generate_inverse_phrase_map", "MAPPING:basic_target:basic_base", "basic_target"),
                    _ => return Err(format!("Unknown stage mapping for '{}'", stage_name)),
                };

                let start = std::cmp::min(start_index, self.state.document.len().saturating_sub(1));
                let end = std::cmp::min(end_index, self.state.document.len().saturating_sub(1));
                let (s, e) = if start <= end { (start, end) } else { (end, start) };

                // Build items — segment-level for GenerateModerateTarget,
                // sentence-level for everything else.
                let segment_level = stage_name == "GenerateModerateTarget";
                let mut items: Vec<(usize, String, String)> = Vec::new();
                for idx in s..=e {
                    if let Some(sent) = self.state.document.get(idx) {
                        if segment_level {
                            // Emit one item per segment: (idx, "S5_S1", segment_text)
                            if let Some(tier) = sent.get_tier(source_tier) {
                                for (seg_i, seg) in tier.segments.iter().enumerate() {
                                    let seg_id = format!("{}_S{}", sent.id, seg_i + 1);
                                    items.push((idx, seg_id, seg.full_text()));
                                }
                            }
                        } else {
                            let source_text = sent.get_tier(source_tier).map(|t| t.full_text()).unwrap_or_default();
                            items.push((idx, sent.id.clone(), source_text));
                        }
                    }
                }

                if items.is_empty() {
                    return Ok("No items to process in range".to_string());
                }

                // Spawn Job
                let prompts = self.state.prompts.clone().unwrap();
                let llm = self.state.llm.clone().unwrap();
                let logger = self.state.logger.clone().unwrap();
                let log_file_path = logger.log_file_path().display().to_string();
                let config_obj = self.state.config.clone().unwrap();
                let (base_lang, target_lang) = self.state.project_languages.clone();

                let (rx, cancel) = crate::services::llm_worker::spawn_llm_job(
                    prompts,
                    llm,
                    logger,
                    config_obj,
                    base_lang,
                    target_lang,
                    prompt_name.to_string(),
                    target_tier.to_string(),
                    items,
                    batch_size,
                    model_alias.clone(),
                    fallback_alias,
                    segment_level,
                );

                self.state.llm_results_receiver = Some(rx);
                self.state.llm_cancel_flag = Some(cancel);
                self.state.llm_job_total = (e - s) + 1;
                self.state.llm_job_done = 0;
                self.state.llm_job_stage = stage_name.to_string();
                self.state.llm_job_target_tier = target_tier.to_string();
                self.state.llm_job_model = model_alias.clone();
                self.state.show_llm_run = false; // Hide UI dialog if open

                Ok(format!(
                    "Started stage '{}' for {} items\n  Model: {}\n  Batch size: {}\n  LLM log: {}",
                    stage_name,
                    self.state.llm_job_total,
                    model_display,
                    batch_size,
                    log_file_path,
                ))
            }
            AppCommand::MeasureAvd { path } => {
                self.execute_measure_avd(&path)
            }
            AppCommand::MeasureUserScore { path } => {
                self.execute_measure_user_score(&path)
            }
            AppCommand::SetKey { provider, value } => {
                crate::services::secrets::set_key(&provider, &value)
                    .map(|_| format!("API key for '{}' stored in OS keychain.", provider))
            }
            AppCommand::DeleteKey { provider } => {
                crate::services::secrets::delete_key(&provider)
                    .map(|_| format!("API key for '{}' removed from OS keychain.", provider))
            }
            AppCommand::KeyStatus => {
                Ok(crate::services::secrets::status_report())
            }
            AppCommand::DebugDump { start_index, end_index, path } => {
                self.execute_debug_dump(start_index, end_index, path.as_deref())
            }
            AppCommand::ApplyCollateral { accept } => {
                if accept {
                    let count = self.state.pending_collateral_updates.len();
                    let updates = std::mem::take(&mut self.state.pending_collateral_updates);
                    let (base_lang, target_lang) = self.state.project_languages.clone();
                    let bridge = self.state.bridge.as_ref();
                    for (idx, _s_id, tier_id, text) in updates {
                        if let Some(sent) = self.state.document.get_mut(idx) {
                            let lang = crate::services::tier_processor::lang_for_tier(&tier_id, &base_lang, &target_lang);
                            let segments = crate::services::tier_processor::tokenize_only(&text, &lang, bridge);
                            sent.update_tier_with_segments(&tier_id, segments);
                        }
                    }
                    Ok(format!("Applied {} collateral updates", count))
                } else {
                    self.state.pending_collateral_updates.clear();
                    Ok("Discarded collateral updates".to_string())
                }
            }
        }
    }

    fn execute_measure_avd(&self, path: &str) -> Result<String, String> {
        use crate::simulation::frequency_manager;
        use crate::simulation::metrics::TextMetrics;

        // Verify frequency list is loaded
        if frequency_manager::get_max_rank() == 0 {
            return Err("Frequency list not loaded. Cannot compute AVD.".to_string());
        }

        // Read the file
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read file '{}': {}", path, e))?;

        if content.trim().is_empty() {
            return Err("File is empty.".to_string());
        }

        // Tokenize via SpaCy bridge to get lemmas
        let bridge = self.state.bridge.as_ref()
            .ok_or("Python Bridge not available. Cannot tokenize text.")?;

        let raw_tokens = bridge.tokenize(content.trim(), "es")
            .map_err(|e| format!("SpaCy tokenization failed: {}", e))?;

        // Extract lemma instances (skip punctuation and whitespace)
        let mut lemma_instances: Vec<String> = Vec::new();
        let mut unknown_lemmas: Vec<String> = Vec::new();
        let mut total_word_tokens = 0u32;

        for token in &raw_tokens {
            if token.is_punct || token.is_space {
                continue;
            }
            total_word_tokens += 1;
            let lemma = token.lemma.to_lowercase();
            if frequency_manager::get_rank_for_lemma(&lemma).is_none() {
                if !unknown_lemmas.contains(&lemma) {
                    unknown_lemmas.push(lemma.clone());
                }
            }
            lemma_instances.push(lemma);
        }

        if lemma_instances.is_empty() {
            return Err("No word tokens found in text.".to_string());
        }

        // Compute AVD using TextMetrics (english_word_count = 0 for pure Spanish text)
        let metrics = TextMetrics::new(&lemma_instances, 0);
        let avd_score = metrics.calculate_avd_score();

        // Find the highest-ranked lemma for context
        let mut max_rank: u32 = 0;
        let mut max_rank_lemma = String::new();
        for lemma in &lemma_instances {
            if let Some(rank) = frequency_manager::get_rank_for_lemma(lemma) {
                if rank > max_rank {
                    max_rank = rank;
                    max_rank_lemma = lemma.clone();
                }
            }
        }

        // Build output report
        let found_count = lemma_instances.iter()
            .filter(|l| frequency_manager::get_rank_for_lemma(l).is_some())
            .count();

        let mut out = String::new();
        out.push_str(&format!("--- AVD Measurement for '{}' ---\n", path));
        out.push_str(&format!("  Total word tokens:    {}\n", total_word_tokens));
        out.push_str(&format!("  In frequency list:    {}\n", found_count));
        out.push_str(&format!("  Unknown lemmas:       {}\n", unknown_lemmas.len()));
        out.push_str(&format!("  AVD Score:            {:.2}\n", avd_score));
        out.push_str(&format!("  Highest ranked lemma: '{}' (rank {})\n", max_rank_lemma, max_rank));

        if !unknown_lemmas.is_empty() {
            unknown_lemmas.sort();
            let display: Vec<&str> = unknown_lemmas.iter().map(|s| s.as_str()).take(20).collect();
            out.push_str(&format!("  Unknown sample:       {}", display.join(", ")));
            if unknown_lemmas.len() > 20 {
                out.push_str(&format!(" ... (+{} more)", unknown_lemmas.len() - 20));
            }
        }

        Ok(out)
    }

    fn execute_measure_user_score(&self, path: &str) -> Result<String, String> {
        use crate::simulation::frequency_manager;
        use crate::simulation::metrics::TextMetrics;

        // AVD-to-User-Level inverse formula constants (from calibrator.rs)
        const A_FIT: f64 = 4.15;
        const B_FIT: f64 = 0.02;

        // Verify frequency list is loaded
        if frequency_manager::get_max_rank() == 0 {
            return Err("Frequency list not loaded. Cannot compute AVD.".to_string());
        }

        // Read and tokenize
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read file '{}': {}", path, e))?;

        if content.trim().is_empty() {
            return Err("File is empty.".to_string());
        }

        let bridge = self.state.bridge.as_ref()
            .ok_or("Python Bridge not available. Cannot tokenize text.")?;

        let raw_tokens = bridge.tokenize(content.trim(), "es")
            .map_err(|e| format!("SpaCy tokenization failed: {}", e))?;

        let mut lemma_instances: Vec<String> = Vec::new();
        let mut total_word_tokens = 0u32;

        for token in &raw_tokens {
            if token.is_punct || token.is_space {
                continue;
            }
            total_word_tokens += 1;
            lemma_instances.push(token.lemma.to_lowercase());
        }

        if lemma_instances.is_empty() {
            return Err("No word tokens found in text.".to_string());
        }

        // Compute AVD
        let metrics = TextMetrics::new(&lemma_instances, 0);
        let avd_score = metrics.calculate_avd_score();

        // Inverse mapping: User Level = A_FIT * ln(AVD + 1) + B_FIT
        let user_level = A_FIT * (avd_score + 1.0).ln() + B_FIT;

        // Find unique lemma count and coverage stats
        let mut unique_lemmas: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for lemma in &lemma_instances {
            unique_lemmas.insert(lemma.as_str());
        }

        let in_freq_list = lemma_instances.iter()
            .filter(|l| frequency_manager::get_rank_for_lemma(l).is_some())
            .count();

        let mut out = String::new();
        out.push_str(&format!("--- User Score Measurement for '{}' ---\n", path));
        out.push_str(&format!("  Total word tokens:    {}\n", total_word_tokens));
        out.push_str(&format!("  Unique lemmas:        {}\n", unique_lemmas.len()));
        out.push_str(&format!("  In frequency list:    {} / {}\n", in_freq_list, lemma_instances.len()));
        out.push_str(&format!("  AVD Score:            {:.2}\n", avd_score));
        out.push_str(&format!("  Estimated User Level: {:.1}\n", user_level));
        out.push_str(&format!("  (Rounded):            UL{}", user_level.round() as u32));

        Ok(out)
    }

    fn execute_export_json(&self, path: &str) -> Result<String, String> {
        let mut path_buf = PathBuf::from(path);

        if path_buf.is_relative() {
            if let Some(ref out_dir) = self.state.output_dir {
                // If it's a relative path, we might resolve it relative to output_dir
                // Or maybe relative to output_dir's parent, but let's test if it starts with out_dir's stem?
                // For simplicity, just use current dir if we passed a path, BUT in tests we usually want to write into out_dir...
                // Wait, in integration_e2e.rs we passed "export json weave_output/exported.json"
                // Actually let's just write to it straight away since the test cwd is right, or if it isn't, 
                // just test using out_dir if it's there.
                // Wait, integration tests run in the crate root. So `weave_output/exported.json` will write to `E:\Bill\development\weavelang\weave_output\exported.json`!
                // But the test is checking `test_case/test_01/weave_output/exported.json`.
                // In Python, they probably do it relative to project or output_dir. 
                // Let's just resolve relative to current dir? 
                // In my old code I had:
                let out_dir_path = PathBuf::from(out_dir);
                if let Some(parent) = out_dir_path.parent() {
                    path_buf = parent.join(path_buf);
                }
            }
        }

        use crate::domain::bridge::domain_sentences_to_json_chapter;
        let (base_lang, target_lang) = &self.state.project_languages;
        let json_chapter = domain_sentences_to_json_chapter(
            &self.state.document,
            &self.state.book_name,
            base_lang,
            target_lang,
            self.state.book_map.as_ref(),
        );

        let json_str = serde_json::to_string_pretty(&json_chapter)
            .map_err(|e| format!("Failed to serialize JSON: {}", e))?;
            
        if let Some(parent) = path_buf.parent() {
            if !parent.as_os_str().is_empty() {
                let _ = std::fs::create_dir_all(parent);
            }
        }

        std::fs::write(&path_buf, json_str)
            .map_err(|e| format!("Failed to write to {}: {}", path_buf.display(), e))?;

        Ok(format!("Exported JSON to {}", path_buf.display()))
    }

    fn execute_export_level_map(&self, path: &str) -> Result<String, String> {
        // Validate that we have a level map
        let book_map = self.state.book_map.as_ref()
            .ok_or("No level map available. Load a calibrated project or JSON file first.")?;

        if book_map.is_empty() {
            return Err("Level map is empty. The calibrator has not been run on this project.".to_string());
        }

        // AVD formula constants (from calibrator.rs)
        const A_FIT: f64 = 4.15;
        const B_FIT: f64 = 0.02;

        // Detect the natural peak: scan ALL map entries across ALL start_levels
        // to find the last micro-level where at least one recipe tier is NOT
        // u32::MAX.  Each start_level key's map only covers its own range,
        // so we must look at all of them.
        let natural_peak: u32;
        let mut peak_micro_level: f64 = 1.0;

        for (_key, curriculum_map) in book_map.iter() {
            for entry in &curriculum_map.map {
                let all_maxed = entry.recipe.bas == u32::MAX
                    && entry.recipe.mod_v == u32::MAX
                    && entry.recipe.adv == u32::MAX;
                if !all_maxed {
                    let lvl = entry.level as f64;
                    if lvl > peak_micro_level {
                        peak_micro_level = lvl;
                    }
                }
            }
        }
        natural_peak = peak_micro_level.floor() as u32;
        let peak_avd_from_map = ((peak_micro_level - B_FIT) / A_FIT).exp() - 1.0;

        // The fractional peak user score is the exact last non-exhausted micro-level
        let peak_user_score: f64 = peak_micro_level;

        let score_int = peak_user_score.floor() as u32;
        let score_frac = ((peak_user_score - score_int as f64) * 10.0).round() as u32;

        // Determine book name
        let book_name = if self.state.book_name.is_empty() {
            "Unknown"
        } else {
            &self.state.book_name
        };

        // Build the total start_levels count (only include non-exhausted ones)
        let total_start_levels = book_map.keys()
            .filter_map(|k| k.parse::<u32>().ok())
            .filter(|&k| k <= natural_peak)
            .count() as u32;

        // Build the LevelMapFile with metadata
        use crate::types::json_types::{LevelMapFile, LevelMapMeta};

        // Filter out levels past the natural peak
        let trimmed_levels: HashMap<String, crate::types::json_types::JsonCurriculumMap> = book_map
            .iter()
            .filter(|(k, _)| {
                k.parse::<u32>().map_or(false, |kv| kv <= natural_peak)
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let lm_file = LevelMapFile {
            meta: LevelMapMeta {
                book_name: book_name.to_string(),
                base_language: self.state.project_languages.0.clone(),
                target_language: self.state.project_languages.1.clone(),
                natural_peak_level: natural_peak,
                peak_avd: (peak_avd_from_map * 100.0).round() / 100.0,
                peak_user_score: (peak_user_score * 10.0).round() / 10.0,
                total_start_levels,
                schema_version: "1.0".to_string(),
            },
            levels: trimmed_levels,
        };

        // Resolve output path
        let path_buf = PathBuf::from(path);
        let output_path = if path_buf.extension().map_or(false, |ext| ext == "lm") {
            // User provided a full filename
            path_buf
        } else {
            // User provided a directory (or ".") — generate default name
            let dir = if path == "." {
                std::env::current_dir().map_err(|e| e.to_string())?
            } else {
                PathBuf::from(path)
            };
            let default_name = format!("{}_UL{}p{}.lm", book_name, score_int, score_frac);
            dir.join(default_name)
        };

        // Serialize the level map file
        let json = serde_json::to_string_pretty(&lm_file)
            .map_err(|e| format!("Failed to serialize level map: {}", e))?;

        // Write the file
        fs::write(&output_path, json)
            .map_err(|e| format!("Failed to write '{}': {}", output_path.display(), e))?;

        let entry_count: usize = lm_file.levels.values().map(|m| m.map.len()).sum();
        Ok(format!(
            "Exported level map to '{}'\n  Book: {}\n  Natural peak: UL{}\n  Peak AVD: {:.2}\n  Peak user score: {:.1}\n  Start levels: {}\n  Total map entries: {}",
            output_path.display(),
            book_name,
            natural_peak,
            peak_avd_from_map,
            peak_user_score,
            total_start_levels,
            entry_count
        ))
    }

    fn execute_import_level_map(&mut self, path: &str) -> Result<String, String> {
        use crate::types::json_types::LevelMapFile;

        let content = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read level map '{}': {}", path, e))?;
        let lm_file: LevelMapFile = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse level map: {}", e))?;

        let level_count = lm_file.levels.len();
        self.state.book_map = Some(lm_file.levels);

        // Update project metadata from the level map if available
        if self.state.book_name.is_empty() {
            self.state.book_name = lm_file.meta.book_name.clone();
        }

        Ok(format!(
            "Imported level map from '{}' — {} levels (peak UL{})",
            path,
            level_count,
            lm_file.meta.natural_peak_level,
        ))
    }

    fn execute_generate_weave(&self, level_arg: &str) -> Result<String, String> {
        use crate::domain::bridge::domain_sentences_to_json_chapter;
        use crate::simulation::dictionary::GlobalLemmaDictionary;
        use crate::simulation::preprocessor;
        use crate::corpus_generator;
        use crate::simulation::text_generator;

        if self.state.document.is_empty() {
            return Err("No document loaded.".to_string());
        }

        let output_dir = self.state.output_dir.as_ref()
            .ok_or("Output directory not set. Use 'set output_dir <path>' first.")?;
        let output_path = PathBuf::from(output_dir);

        let book_map = self.state.book_map.as_ref()
            .ok_or("No level map loaded. Use 'import level_map <path>' first.")?;

        // Determine which levels to generate
        let levels: Vec<u32> = if level_arg == "all" {
            let mut lvls: Vec<u32> = book_map.keys()
                .filter_map(|k| k.parse::<u32>().ok())
                .collect();
            lvls.sort();
            lvls
        } else {
            let lvl = level_arg.parse::<u32>()
                .map_err(|_| format!("Invalid level '{}'. Use a number or 'all'.", level_arg))?;
            vec![lvl]
        };

        // Build JsonChapter from domain sentences
        let (base_lang, target_lang) = &self.state.project_languages;
        let json_chapter = domain_sentences_to_json_chapter(
            &self.state.document,
            &self.state.book_name,
            base_lang,
            target_lang,
            self.state.book_map.as_ref(),
        );

        // Build NumericalChapter + dictionary
        let mut dictionary = GlobalLemmaDictionary::new();
        let (numerical_chapter, _eng_word_counts) =
            preprocessor::json_chapter_to_numerical(&json_chapter, &mut dictionary);

        let mut generated_files: Vec<String> = Vec::new();

        for level in &levels {
            let level_key = level.to_string();
            let recipe = book_map.get(&level_key)
                .and_then(|cm| cm.map.first())
                .ok_or(format!("No recipe found for level {}", level))?;

            let result = corpus_generator::generate_book_instance(
                &numerical_chapter,
                &json_chapter,
                &dictionary,
                recipe.recipe.bas,
                recipe.recipe.mod_v,
                recipe.recipe.adv,
                0.5, // inverse_diglot_threshold
                false, // debug_markers
            ).map_err(|e| format!("Generation failed for level {}: {}", level, e))?;

            // Assemble output text: join sentence texts with double newline
            let cleaned_parts: Vec<String> = result.final_text_parts
                .iter()
                .map(|p| text_generator::clean_text_for_tts(p))
                .collect();
            let output_text = cleaned_parts.join("\n\n");

            let file_name = format!("UL{}.txt", level);
            let file_path = output_path.join(&file_name);
            fs::write(&file_path, &output_text)
                .map_err(|e| format!("Failed to write '{}': {}", file_path.display(), e))?;

            generated_files.push(format!("UL{}.txt ({} sentences)", level, result.final_text_parts.len()));
        }

        Ok(format!(
            "Generated {} weave file(s) in '{}':\n  {}",
            generated_files.len(),
            output_dir,
            generated_files.join("\n  "),
        ))
    }

    fn execute_debug_dump(&self, start: usize, end: usize, path: Option<&str>) -> Result<String, String> {
        if self.state.document.is_empty() {
            return Err("No document loaded.".to_string());
        }

        let max_idx = self.state.document.len().saturating_sub(1);
        let s = start.min(max_idx);
        let e = end.min(max_idx);
        let (s, e) = if s <= e { (s, e) } else { (e, s) };

        // Tier display order (matching the project's tier hierarchy)
        let tier_order = [
            ("base",             "Base (Original)"),
            ("advanced_target",  "Advanced Target"),
            ("moderate_target",  "Moderate Target"),
            ("basic_target",     "Basic Target"),
            ("basic_base",       "Basic Base (Simplified)"),
        ];

        let mut out = String::new();
        out.push_str(&format!("=== Debug Dump: sentences {} to {} ===\n", s, e));
        out.push_str(&format!("=== Book: {} | Languages: {}/{} ===\n\n",
            if self.state.book_name.is_empty() { "Unknown" } else { &self.state.book_name },
            self.state.project_languages.0,
            self.state.project_languages.1,
        ));

        for idx in s..=e {
            let sent = &self.state.document[idx];
            let base_text = sent.get_tier("base")
                .map(|t| t.full_text())
                .unwrap_or_else(|| "(no base tier)".to_string());

            out.push_str(&format!(
                "================================================================\n=== {} (index {}): \"{}\" ===\n================================================================\n\n",
                sent.id,
                idx,
                if base_text.len() > 80 { format!("{}...", &base_text[..77]) } else { base_text }
            ));

            // Tiers
            for (tier_id, tier_label) in &tier_order {
                if let Some(tier) = sent.get_tier(tier_id) {
                    out.push_str(&format!("--- {} ({}) ---\n", tier_label, tier_id));
                    out.push_str(&format!("  Text: \"{}\"\n", tier.full_text()));
                    out.push_str(&format!("  State: {:?}\n", tier.state));

                    // Show segments if more than one
                    if tier.segments.len() > 1 {
                        out.push_str(&format!("  Segments ({}):\n", tier.segments.len()));
                        for (si, seg) in tier.segments.iter().enumerate() {
                            out.push_str(&format!("    [{}] \"{}\"\n", si, seg.full_text()));
                        }
                    }

                    // Show lemmas if present
                    if !tier.lemmas.is_empty() {
                        let display_count = tier.lemmas.len().min(20);
                        out.push_str(&format!("  Lemmas ({}): {}", tier.lemmas.len(),
                            tier.lemmas[..display_count].join(", ")));
                        if tier.lemmas.len() > 20 {
                            out.push_str(&format!(" ... (+{} more)", tier.lemmas.len() - 20));
                        }
                        out.push('\n');
                    }
                    out.push('\n');
                }
            }

            // Mappings
            if !sent.mappings.is_empty() {
                for mapping in &sent.mappings {
                    out.push_str(&format!("--- Mapping: {} → {} ({} entries) ---\n",
                        mapping.from_tier_id, mapping.to_tier_id, mapping.entries.len()));
                    for entry in &mapping.entries {
                        let viable_marker = if !entry.is_viable { " [NOT VIABLE]" } else { "" };
                        let proper_marker = if entry.is_proper_noun { " [PROPER]" } else { "" };
                        out.push_str(&format!("  w{}: \"{}\" lemmas=[{}]{}{}\n",
                            entry.source_word_id.0,
                            entry.target_text,
                            entry.target_lemmas.join(", "),
                            viable_marker,
                            proper_marker,
                        ));
                    }
                    out.push('\n');
                }
            } else {
                out.push_str("--- Mappings: (none) ---\n\n");
            }
        }

        // Write to file if path provided
        if let Some(file_path) = path {
            fs::write(file_path, &out)
                .map_err(|e| format!("Failed to write debug dump to '{}': {}", file_path, e))?;
            Ok(format!("Debug dump written to '{}' ({} sentences, {} bytes)",
                file_path, e - s + 1, out.len()))
        } else {
            Ok(out)
        }
    }
}
