use crate::services::prompt_manager::PromptManager;
use crate::services::llm_client::LlmService;
use crate::services::llm_logger::LlmLogger;
use regex::Regex;
use std::thread::sleep;
use std::time::Duration;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct LlmStageService {
    prompts: PromptManager,
    llm: LlmService,
    logger: LlmLogger,
}

impl LlmStageService {
    pub fn new(prompts: PromptManager, llm: LlmService, logger: LlmLogger) -> Self {
        Self { prompts, llm, logger }
    }

    /// items: Vec of (sentence_index_in_document, s_id, text)
    /// Returns Vec of (sentence_index_in_document, s_id, generated_text)
    pub fn generate_for_items(
        &self,
        base_code: &str,
        target_code: &str,
        prompt_name: &str,
        items: Vec<(usize, String, String)>,
        batch_size: usize,
        model: &str,
        fallback_model: Option<&str>,
        cancel_flag: Option<&AtomicBool>,
    ) -> Result<Vec<(usize, String, String)>, String> {
        let mut results: Vec<(usize, String, String)> = Vec::new();
        let id_re = Regex::new(r"^\s*([A-Za-z0-9_:-]+)\s*:\s*(.+)$").map_err(|e| e.to_string())?;

        // Load system prompt once per call (prompt templates shouldn't change during the run)
        let system_prompt = self
            .prompts
            .get_prompt(prompt_name, base_code, target_code)
            .map_err(|e| format!("Failed to load prompt {}: {}", prompt_name, e))?;

        // Set context so MockLlmProvider knows which canned file to read.
        self.llm.set_context(prompt_name);

        for chunk in items.chunks(batch_size) {
            // Check for cancellation between batches
            if let Some(cf) = cancel_flag {
                if cf.load(Ordering::SeqCst) {
                    // stop early and return results gathered so far
                    break;
                }
            }
            // Build user prompt lines like "S123: <text>"
            let user_prompt = format!(
                "STRICT REQUIREMENT: Provide exactly one line for each ID provided below. Do not merge sentences. Do not skip IDs.\n\n{}",
                chunk
                    .iter()
                    .map(|(_, s_id, text)| format!("{}: {}", s_id, text))
                    .collect::<Vec<_>>()
                    .join("\n")
            );

            // Try primary then fallback model with simple retry/backoff
            let mut response_text: Option<String> = None;
            let mut last_err = String::new();

            let mut try_models = vec![model.to_string()];
            if let Some(fb) = fallback_model {
                try_models.push(fb.to_string());
            }

            for m in try_models.iter() {
                for attempt in 0..3 {
                    match self.llm.complete(m, &system_prompt, &user_prompt) {
                        Ok(resp) => {
                            response_text = Some(resp);
                            break;
                        }
                        Err(e) => {
                            last_err = e;
                            // exponential backoff
                            let backoff = Duration::from_secs(1 << attempt);
                            sleep(backoff);
                        }
                    }
                }
                if response_text.is_some() {
                    break;
                }
            }

            let resp = match response_text {
                Some(r) => r,
                None => return Err(format!("LLM calls failed for batch: {}", last_err)),
            };

            // Parse id-prefixed lines
            for line in resp.lines() {
                if let Some(cap) = id_re.captures(line) {
                    let sid = cap[1].trim().to_string();
                    let gen = cap[2].trim().to_string();

                    // Only accept IDs that were part of this chunk
                    if let Some((idx, _s_id, _)) = chunk.iter().find(|(_, s, _)| s == &sid) {
                        results.push((*idx, sid.clone(), gen.clone()));
                    }
                }
            }

            // Log the interaction
            let _ = self.logger.log_interaction(
                &format!("Stage: {}", prompt_name),
                &system_prompt,
                &user_prompt,
                &resp,
            );
        }

        Ok(results)
    }
}

// ------------ Test helper & unit tests ------------
#[cfg(test)]
pub fn generate_for_items_with_prompt_text<F>(
    system_prompt: &str,
    items: Vec<(usize, String, String)>,
    batch_size: usize,
    llm_fn: F,
    logger: &crate::services::llm_logger::LlmLogger,
) -> Result<Vec<(usize, String, String)>, String>
where
    F: Fn(&str) -> Result<String, String>,
{
    let mut results: Vec<(usize, String, String)> = Vec::new();
    let id_re = Regex::new(r"^\s*([A-Za-z0-9_:-]+)\s*:\s*(.+)$").map_err(|e| e.to_string())?;

    for chunk in items.chunks(batch_size) {
        let user_prompt = chunk
            .iter()
            .map(|(_, s_id, text)| format!("{}: {}", s_id, text))
            .collect::<Vec<_>>()
            .join("\n");

        let resp = llm_fn(&user_prompt)?;

        for line in resp.lines() {
            if let Some(cap) = id_re.captures(line) {
                let sid = cap[1].trim().to_string();
                let gen = cap[2].trim().to_string();
                if let Some((idx, _s_id, _)) = chunk.iter().find(|(_, s, _)| s == &sid) {
                    results.push((*idx, sid.clone(), gen.clone()));
                }
            }
        }

        let _ = logger.log_interaction("test", system_prompt, &user_prompt, &resp);
    }

    Ok(results)
}

        #[test]
        fn test_logging_writes_file() {
            use std::env;
            use std::fs;
            use std::time::{SystemTime, UNIX_EPOCH};

            let system_prompt = "SYS";
            let items = vec![(0usize, "S1".to_string(), "Hello".to_string())];

            // make a unique temp dir
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let tmp = env::temp_dir().join(format!("weavelang_llm_test_{now}"));
            let _ = fs::create_dir_all(&tmp);

            let logger = crate::services::llm_logger::LlmLogger::new(tmp.clone());

            let llm_fn = |user: &str| -> Result<String, String> {
                Ok(format!("S1: {}-out", user))
            };

            let res = generate_for_items_with_prompt_text(system_prompt, items, 10, llm_fn, &logger)
                .expect("should succeed");

            assert_eq!(res.len(), 1);

            // verify log file exists and contains the system prompt
            let log_path = tmp.join("studio_llm.log");
            let contents = fs::read_to_string(&log_path).expect("log file should exist");
            assert!(contents.contains(system_prompt), "log should contain system prompt");
        }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::llm_logger::LlmLogger;
    use std::env;

    #[test]
    fn test_generate_for_items_with_prompt_text_single() {
        let system_prompt = "SYS";
        let items = vec![(0usize, "S1".to_string(), "Hello world.".to_string())];
        let logger = LlmLogger::new(env::temp_dir());

        let llm_fn = |user: &str| -> Result<String, String> {
            assert_eq!(user, "S1: Hello world.");
            Ok("S1: Hello simple.".to_string())
        };

        let res = generate_for_items_with_prompt_text(system_prompt, items, 10, llm_fn, &logger)
            .expect("should succeed");

        assert_eq!(res.len(), 1);
        assert_eq!(res[0].1, "S1");
        assert_eq!(res[0].2, "Hello simple.");
    }

    #[test]
    fn test_ignores_unknown_ids_in_response() {
        let system_prompt = "SYS";
        let items = vec![(0usize, "S1".to_string(), "A".to_string())];
        let logger = LlmLogger::new(std::env::temp_dir());

        let llm_fn = |user: &str| -> Result<String, String> {
            // Respond with an unknown ID S999 which should be ignored
            Ok("S999: should be ignored\nS1: A-simple".to_string())
        };

        let res = generate_for_items_with_prompt_text(system_prompt, items, 10, llm_fn, &logger)
            .expect("should succeed");

        assert_eq!(res.len(), 1);
        assert_eq!(res[0].1, "S1");
        assert_eq!(res[0].2, "A-simple");
    }

    #[test]
    fn test_malformed_lines_are_ignored() {
        let system_prompt = "SYS";
        let items = vec![(0usize, "S1".to_string(), "A".to_string())];
        let logger = LlmLogger::new(std::env::temp_dir());

        let llm_fn = |_: &str| -> Result<String, String> {
            Ok("This line has no id prefix\nS1: Good".to_string())
        };

        let res = generate_for_items_with_prompt_text(system_prompt, items, 10, llm_fn, &logger)
            .expect("should succeed");

        assert_eq!(res.len(), 1);
        assert_eq!(res[0].2, "Good");
    }

    #[test]
    fn test_partial_batch_response() {
        let system_prompt = "SYS";
        let items = vec![
            (0usize, "S1".to_string(), "A".to_string()),
            (1usize, "S2".to_string(), "B".to_string()),
        ];
        let logger = LlmLogger::new(std::env::temp_dir());

        let llm_fn = |user: &str| -> Result<String, String> {
            // Only respond for S2
            Ok("S2: B-simple".to_string())
        };

        let res = generate_for_items_with_prompt_text(system_prompt, items, 10, llm_fn, &logger)
            .expect("should succeed");

        assert_eq!(res.len(), 1);
        assert_eq!(res[0].1, "S2");
        assert_eq!(res[0].2, "B-simple");
    }

    #[test]
    fn test_generate_for_items_with_prompt_text_batching() {
        let system_prompt = "SYS";
        let items = vec![
            (0usize, "S1".to_string(), "A".to_string()),
            (1usize, "S2".to_string(), "B".to_string()),
            (2usize, "S3".to_string(), "C".to_string()),
        ];
        let logger = LlmLogger::new(env::temp_dir());

        let llm_fn = |user: &str| -> Result<String, String> {
            // echo back with simplified text
            Ok(user
                .lines()
                .map(|l| {
                    let parts: Vec<&str> = l.splitn(2, ':').collect();
                    format!("{}: {}-simple", parts[0].trim(), parts[1].trim())
                })
                .collect::<Vec<_>>()
                .join("\n"))
        };

        let res = generate_for_items_with_prompt_text(system_prompt, items, 2, llm_fn, &logger)
            .expect("should succeed");

        // Should have results for all 3 items
        assert_eq!(res.len(), 3);
        assert_eq!(res[0].2, "A-simple");
        assert_eq!(res[1].2, "B-simple");
        assert_eq!(res[2].2, "C-simple");
    }
}
