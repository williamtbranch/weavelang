use crate::services::llm_logger::LlmLogger;
use crate::services::llm_stage::LlmStageService;
use crate::services::prompt_manager::PromptManager;
use crate::services::llm_client::LlmService;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc};
use std::sync::atomic::AtomicBool;

/// Spawn a background worker to run `LlmStageService::generate_for_items` and
/// return a Receiver that will yield the Vec of results when ready.
///
/// When `segment_level` is true, items are expected to have segment-level IDs
/// (e.g. "S5_S1", "S5_S2") and results will be reassembled back into
/// sentence-level results before being sent through the channel.  This prevents
/// LLM segment-boundary drift while keeping the result protocol unchanged.
pub fn spawn_llm_job(
    prompts: PromptManager,
    llm: LlmService,
    logger: LlmLogger,
    base_code: String,
    target_code: String,
    prompt_name: String,
    target_tier_id: String,
    items: Vec<(usize, String, String)>,
    batch_size: usize,
    model: String,
    fallback_model: Option<String>,
    segment_level: bool,
) -> (Receiver<Result<Vec<(usize, String, String, String)>, String>>, Arc<AtomicBool>) {
    let (tx, rx) = mpsc::channel();
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let cancel_thread_flag = cancel_flag.clone();

    std::thread::spawn(move || {
        let svc = LlmStageService::new(prompts, llm, logger);

        let fb_ref = fallback_model.as_deref();

        let res = svc.generate_for_items(
            &base_code,
            &target_code,
            &prompt_name,
            items,
            batch_size,
            &model,
            fb_ref,
            Some(cancel_thread_flag.as_ref()),
        );

        // Map results to include target_tier_id so the UI knows which tier to update.
        // When segment_level is true, reassemble Sn_Sm results into one result per sentence.
        let mapped: Result<Vec<(usize, String, String, String)>, String> = match res {
            Ok(v) if segment_level => Ok(reassemble_segment_results(v, &target_tier_id)),
            Ok(v) => Ok(v
                .into_iter()
                .map(|(idx, sid, gen)| (idx, sid, target_tier_id.clone(), gen))
                .collect()),
            Err(e) => Err(e),
        };

        // Ignore send errors (receiver dropped) for now
        let _ = tx.send(mapped);
    });

    (rx, cancel_flag)
}

/// Reassemble segment-level LLM results (Sn_Sm) into sentence-level results.
///
/// Groups results by sentence index, sorts segments by their ordinal within
/// each sentence, and joins segment texts with spaces (trimming trailing
/// whitespace from the final result).  Each sentence emits one result tuple
/// with the base sentence ID (e.g. "S5").
fn reassemble_segment_results(
    segment_results: Vec<(usize, String, String)>,
    target_tier_id: &str,
) -> Vec<(usize, String, String, String)> {
    use std::collections::BTreeMap;

    // Group by sentence index.  Within each group, store (segment_ordinal, text).
    let mut groups: BTreeMap<usize, Vec<(usize, String, String)>> = BTreeMap::new();
    for (idx, sid, gen) in segment_results {
        groups.entry(idx).or_default().push((idx, sid, gen));
    }

    let mut out = Vec::new();
    for (_idx, mut entries) in groups {
        // Sort by the segment ordinal — the number after the underscore in "S5_S3".
        entries.sort_by_key(|(_, sid, _)| {
            sid.rsplit('_')
                .next()
                .and_then(|s| s.trim_start_matches('S').parse::<usize>().ok())
                .unwrap_or(0)
        });

        // Derive sentence-level ID by stripping the _Sm suffix from the first entry.
        let sent_id = entries[0]
            .1
            .split('_')
            .next()
            .unwrap_or(&entries[0].1)
            .to_string();
        let sent_idx = entries[0].0;

        // Join segment texts with spaces, trim trailing whitespace.
        let full_text: String = entries
            .iter()
            .enumerate()
            .map(|(i, (_, _, text))| {
                if i < entries.len() - 1 {
                    // Non-final segment: ensure a trailing space for separation
                    if text.ends_with(' ') {
                        text.clone()
                    } else {
                        format!("{} ", text)
                    }
                } else {
                    text.clone()
                }
            })
            .collect::<String>()
            .trim_end()
            .to_string();

        out.push((sent_idx, sent_id, target_tier_id.to_string(), full_text));
    }
    out
}

#[cfg(test)]
pub fn spawn_test_llm_job<F>(
    prompts: crate::services::prompt_manager::PromptManager,
    logger: crate::services::llm_logger::LlmLogger,
    base_code: String,
    target_code: String,
    prompt_name: String,
    target_tier_id: String,
    items: Vec<(usize, String, String)>,
    batch_size: usize,
    llm_fn: F,
) -> std::sync::mpsc::Receiver<Result<Vec<(usize, String, String, String)>, String>>
where
    F: Fn(&str) -> Result<String, String> + Send + 'static + Clone,
{
    use regex::Regex;
    use std::sync::mpsc;

    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        let id_re = Regex::new(r"^\s*([A-Za-z0-9_:-]+)\s*:\s*(.+)$").unwrap();

        for chunk in items.chunks(batch_size) {
            let user_prompt = chunk
                .iter()
                .map(|(_, s_id, text)| format!("{}: {}", s_id, text))
                .collect::<Vec<_>>()
                .join("\n");

            let resp = match llm_fn(&user_prompt) {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(Err(e));
                    return;
                }
            };

            let mut results: Vec<(usize, String, String, String)> = Vec::new();
            for line in resp.lines() {
                if let Some(cap) = id_re.captures(line) {
                    let sid = cap[1].trim().to_string();
                    let gen = cap[2].trim().to_string();
                    if let Some((idx, _s_id, _)) = chunk.iter().find(|(_, s, _)| s == &sid) {
                        results.push((*idx, sid.clone(), target_tier_id.clone(), gen.clone()));
                    }
                }
            }

            let _ = logger.log_interaction(&format!("Stage: {}", prompt_name), "SYS", &user_prompt, &resp);

            let _ = tx.send(Ok(results));
        }
    });

    rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::prompt_manager::PromptManager;
    use crate::services::llm_logger::LlmLogger;
    use std::env;
    use std::fs;

    #[test]
    fn test_spawn_test_llm_job() {
        // prepare prompt manager and logger
        let tmp = env::temp_dir().join("weavelang_spawn_test");
        let _ = fs::create_dir_all(&tmp);
        let prompts = PromptManager::new(tmp.clone());
        let logger = LlmLogger::new(tmp.clone());

        let items = vec![
            (0usize, "S1".to_string(), "Hello".to_string()),
            (1usize, "S2".to_string(), "World".to_string()),
        ];

        let llm_fn = |user: &str| -> Result<String, String> {
            // echo with -out suffix
            Ok(user
                .lines()
                .map(|l| {
                    let parts: Vec<&str> = l.splitn(2, ':').collect();
                    format!("{}: {}-out", parts[0].trim(), parts[1].trim())
                })
                .collect::<Vec<_>>()
                .join("\n"))
        };

        let rx = spawn_test_llm_job(
            prompts,
            logger,
            "en".to_string(),
            "es".to_string(),
            "simplify_test".to_string(),
            "basic_base".to_string(),
            items,
            10,
            llm_fn,
        );

        // receive results
        let mut collected: Vec<(usize, String, String, String)> = Vec::new();
        while let Ok(msg) = rx.recv() {
            match msg {
                Ok(mut v) => collected.append(&mut v),
                Err(e) => panic!("Job failed: {}", e),
            }
        }

        // Expect two results
        assert_eq!(collected.len(), 2);
        assert_eq!(collected[0].1, "S1");
        assert_eq!(collected[1].1, "S2");
        assert!(collected[0].3.ends_with("-out"));
    }

    #[test]
    fn test_reassemble_segment_results_basic() {
        // Two sentences, S5 has 3 segments, S6 has 2
        let segments = vec![
            (5, "S5_S1".to_string(), "Una mañana,".to_string()),
            (5, "S5_S2".to_string(), "cuando Gregor despertó".to_string()),
            (5, "S5_S3".to_string(), "de sueños malos.".to_string()),
            (6, "S6_S1".to_string(), "Estaba sobre".to_string()),
            (6, "S6_S2".to_string(), "su espalda dura.".to_string()),
        ];

        let result = reassemble_segment_results(segments, "moderate_target");

        assert_eq!(result.len(), 2);
        // Sentence S5
        assert_eq!(result[0].0, 5);
        assert_eq!(result[0].1, "S5");
        assert_eq!(result[0].2, "moderate_target");
        assert_eq!(result[0].3, "Una mañana, cuando Gregor despertó de sueños malos.");
        // Sentence S6
        assert_eq!(result[1].0, 6);
        assert_eq!(result[1].1, "S6");
        assert_eq!(result[1].3, "Estaba sobre su espalda dura.");
    }

    #[test]
    fn test_reassemble_segment_results_preserves_trailing_space() {
        // Segment already has trailing space — should not double-space
        let segments = vec![
            (0, "S1_S1".to_string(), "Hello ".to_string()),
            (0, "S1_S2".to_string(), "world.".to_string()),
        ];

        let result = reassemble_segment_results(segments, "moderate_target");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].3, "Hello world.");
    }

    #[test]
    fn test_reassemble_segment_results_out_of_order() {
        // Segments arrive in reverse order — should still sort correctly
        let segments = vec![
            (3, "S3_S3".to_string(), "third.".to_string()),
            (3, "S3_S1".to_string(), "first".to_string()),
            (3, "S3_S2".to_string(), "second".to_string()),
        ];

        let result = reassemble_segment_results(segments, "mod");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].3, "first second third.");
    }

    #[test]
    fn test_reassemble_single_segment_sentence() {
        let segments = vec![
            (9, "S9_S1".to_string(), "No era un sueño.".to_string()),
        ];

        let result = reassemble_segment_results(segments, "moderate_target");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].1, "S9");
        assert_eq!(result[0].3, "No era un sueño.");
    }
}
