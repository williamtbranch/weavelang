use crate::services::llm_logger::LlmLogger;
use crate::services::llm_stage::LlmStageService;
use crate::services::prompt_manager::PromptManager;
use crate::services::llm_client::LlmService;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc};
use std::sync::atomic::AtomicBool;

/// Prefix prepended to LLM result text when segmentation failed gracefully.
/// `apply_llm_result` detects this, strips it, and marks the tier Stale
/// instead of Valid so the UI signals the partial failure.
pub const SEG_FAIL_PREFIX: &str = "\x01SEGFAIL\x01";

/// Split `items` into balanced chunks.
///
/// If the naïve last chunk would be smaller than half `batch_size`, merge the
/// last two chunks and redistribute them as evenly as possible.
///
/// Example: 22 items with batch 10 → naïve [10, 10, 2].
///   Last chunk (2) < ceil(10/2) = 5, so merge last two → 12 items,
///   split into [6, 6].  Final result: [10, 6, 6].
pub fn compute_balanced_chunks<T: Clone>(items: Vec<T>, batch_size: usize) -> Vec<Vec<T>> {
    if items.is_empty() || batch_size == 0 {
        return if items.is_empty() { vec![] } else { vec![items] };
    }

    let mut chunks: Vec<Vec<T>> = items.chunks(batch_size).map(|c| c.to_vec()).collect();

    if chunks.len() >= 2 {
        let last_len = chunks.last().map(|c| c.len()).unwrap_or(0);
        let threshold = (batch_size + 1) / 2; // ceil(batch_size / 2)
        if last_len < threshold {
            // Merge last two chunks and split evenly
            let last = chunks.pop().unwrap();
            let second_last = chunks.pop().unwrap();
            let mut merged: Vec<T> = second_last;
            merged.extend(last);
            let total = merged.len();
            let half = total / 2;
            let second_half = merged.split_off(half);
            chunks.push(merged);
            chunks.push(second_half);
        }
    }

    chunks
}

/// Split segment-level items into chunks that never break sentence boundaries.
///
/// Mirrors the Python pipeline's `generate_phrase_map.py` logic: group items by
/// their sentence index, then greedily pack whole sentences into batches up to
/// `batch_size` items.  A sentence that would overflow the current batch starts
/// a new batch (even if the sentence itself exceeds `batch_size` — we never
/// split a single sentence).
///
/// This is critical for `GenerateModerateTarget` where each item is a segment
/// (e.g. "S5_S3") and the LLM needs full sentence context to produce coherent
/// simplifications.
pub fn compute_sentence_aligned_chunks(
    items: Vec<(usize, String, String)>,
    batch_size: usize,
) -> Vec<Vec<(usize, String, String)>> {
    use std::collections::BTreeMap;

    if items.is_empty() {
        return vec![];
    }
    if batch_size == 0 {
        return vec![items];
    }

    // Group items by sentence index, preserving insertion order within each group.
    let mut groups: BTreeMap<usize, Vec<(usize, String, String)>> = BTreeMap::new();
    for item in items {
        groups.entry(item.0).or_default().push(item);
    }

    let mut chunks: Vec<Vec<(usize, String, String)>> = Vec::new();
    let mut current_batch: Vec<(usize, String, String)> = Vec::new();

    for (_sent_idx, sentence_items) in groups {
        // If adding this sentence would exceed the batch size, flush current batch.
        if !current_batch.is_empty()
            && current_batch.len() + sentence_items.len() > batch_size
        {
            chunks.push(std::mem::take(&mut current_batch));
        }
        current_batch.extend(sentence_items);
    }
    if !current_batch.is_empty() {
        chunks.push(current_batch);
    }

    chunks
}

/// Spawn a background worker that runs LLM generation **per-batch**, sending
/// each batch's results through the channel as soon as they arrive.
///
/// This ensures that if batch N fails, batches 1..N-1 have already been sent
/// and applied by the receiver, so no data is lost.
///
/// Batch sizes are tail-balanced: if the last batch would be a runt, the final
/// two batches are redistributed evenly (see `compute_balanced_chunks`).
///
/// When `segment_level` is true, items are expected to have segment-level IDs
/// (e.g. "S5_S1", "S5_S2") and results will be reassembled back into
/// sentence-level results before being sent through the channel.
pub fn spawn_llm_job(
    prompts: PromptManager,
    llm: LlmService,
    logger: LlmLogger,
    config: crate::config::Config,
    base_code: String,
    target_code: String,
    prompt_name: String,
    target_tier_id: String,
    items: Vec<(usize, String, String)>,
    // Optional per-document sentence texts (indexed by document index) used
    // to prepend a read-only CONTEXT block of the sentences preceding each
    // batch. Enables pro-drop pronoun/gender resolution in mapping stages.
    context_texts: Option<Vec<String>>,
    batch_size: usize,
    model: String,
    fallback_model: Option<String>,
    segment_level: bool,
    // When true, the advanced_target tier is NOT segmented: each sentence is
    // emitted as a single segment and the per-sentence segmentation LLM call
    // is skipped entirely. Used by simple_triple mode, where the advanced and
    // moderate tiers are never woven (so segment boundaries are irrelevant).
    skip_advanced_segmentation: bool,
) -> (Receiver<Result<Vec<(usize, String, String, String)>, String>>, Arc<AtomicBool>) {
    let (tx, rx) = mpsc::channel();
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let cancel_thread_flag = cancel_flag.clone();

    std::thread::spawn(move || {
        // ── Passthrough short-circuit ─────────────────────────────────────
        // When `prompt_name` is the passthrough sentinel, skip the LLM
        // entirely: each item's source text is echoed as the generated
        // text. Used when `source_is_basic: on` is asserted and the
        // in-source-language basic tier is just a verbatim copy of `base`.
        if prompt_name == crate::services::tier_graph::PROMPT_PASSTHROUGH_COPY {
            let cancel_on_disconnect = cancel_thread_flag.clone();
            // Send all items in one batch — there are no API calls and no
            // fragility benefit to chunking, but downstream code expects
            // batches so we still wrap once.
            let mapped: Vec<(usize, String, String, String)> = items
                .into_iter()
                .map(|(idx, sid, src)| (idx, sid, target_tier_id.clone(), src))
                .collect();
            if tx.send(Ok(mapped)).is_err() {
                cancel_on_disconnect.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            return;
        }

        let svc = LlmStageService::new(prompts.clone(), llm.clone(), logger.clone());
        let fb_ref = fallback_model.as_deref();

        // When segment_level is true, items are segments (e.g. S5_S1, S5_S2).
        // Use sentence-aligned batching so a sentence's segments are never split
        // across LLM calls — Python parity and critical for coherent simplification.
        let chunks = if segment_level {
            compute_sentence_aligned_chunks(items, batch_size)
        } else {
            compute_balanced_chunks(items, batch_size)
        };

        // Clone cancel flag for the callback — if the receiver is gone
        // (app closed/crashed), tx.send() fails and we set the flag so
        // the LLM loop stops before making more API calls.
        let cancel_on_disconnect = cancel_thread_flag.clone();

        let result = svc.generate_for_items_streaming(
            &base_code,
            &target_code,
            &prompt_name,
            chunks,
            context_texts,
            &model,
            fb_ref,
            Some(cancel_thread_flag.as_ref()),
            |batch_results| {
                // Post-process each batch and send immediately
                let mapped: Vec<(usize, String, String, String)> = if segment_level {
                    reassemble_segment_results(batch_results, &target_tier_id)
                } else if target_tier_id == "advanced_target" {
                    batch_results
                        .into_iter()
                        .map(|(idx, sid, gen)| {
                            // simple_triple: the advanced/moderate tiers are
                            // never woven, so segment boundaries don't matter.
                            // Emit the whole sentence as a single segment and
                            // skip the segmentation LLM call entirely.
                            if skip_advanced_segmentation {
                                return (idx, sid, target_tier_id.clone(), gen);
                            }
                            let seg_result = crate::services::llm_segmenter::segment_sentence(
                                &gen, &sid, &llm, &prompts, &logger, &config,
                                // advanced_target text is always in the target
                                // language, so segment from the `{target}-{target}`
                                // directory (e.g. es-es/segment.txt) regardless of
                                // the producing stage's translation direction.
                                &target_code, &target_code,
                            );
                            let (segs, failed) = match seg_result {
                                Ok(s) => (s, false),
                                Err(e) => {
                                    eprintln!("[Segmenter] {} failed: {}", sid, e);
                                    let _ = logger.log_interaction(
                                        &format!("LLMSegmenter FAILED S_ID={}", sid),
                                        "",
                                        "",
                                        &format!("ERROR: {}", e),
                                    );
                                    (vec![gen], true)
                                }
                            };
                            let mut text = segs.join("\0");
                            if failed {
                                text = format!("{}{}", SEG_FAIL_PREFIX, text);
                            }
                            (idx, sid, target_tier_id.clone(), text)
                        })
                        .collect()
                } else {
                    batch_results
                        .into_iter()
                        .map(|(idx, sid, gen)| (idx, sid, target_tier_id.clone(), gen))
                        .collect()
                };

                // If the receiver is gone, set cancel flag to stop further LLM calls
                if tx.send(Ok(mapped)).is_err() {
                    eprintln!("[LLM-THREAD] Dead-man's switch triggered! Channel disconnected — setting cancel flag");
                    cancel_on_disconnect.store(true, std::sync::atomic::Ordering::SeqCst);
                } else {
                    eprintln!("[LLM-THREAD] Batch results sent to GUI successfully");
                }
            },
        );

        // If the streaming method returned an error, forward it
        if let Err(e) = result {
            let _ = tx.send(Err(e));
        }
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

        // Join segment texts with \0 to preserve boundaries.
        // NOTE: we do NOT trim here. Trimming destroys the original spacings output by LLM.
        let full_text: String = entries
            .iter()
            .map(|(_, _, text)| text.to_string())
            .collect::<Vec<String>>()
            .join("\0");

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
        assert_eq!(result[0].3, "Una mañana,\0cuando Gregor despertó\0de sueños malos.");
        // Sentence S6
        assert_eq!(result[1].0, 6);
        assert_eq!(result[1].1, "S6");
        assert_eq!(result[1].3, "Estaba sobre\0su espalda dura.");
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
        assert_eq!(result[0].3, "Hello \0world.");
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
        assert_eq!(result[0].3, "first\0second\0third.");
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

    #[test]
    fn test_balanced_chunks_exact_multiple() {
        // 20 items batch 10 → [10, 10] — no rebalancing needed
        let items: Vec<i32> = (0..20).collect();
        let chunks = compute_balanced_chunks(items, 10);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 10);
        assert_eq!(chunks[1].len(), 10);
    }

    #[test]
    fn test_balanced_chunks_runt_rebalanced() {
        // 22 items batch 10 → naïve [10, 10, 2] → balanced [10, 6, 6]
        let items: Vec<i32> = (0..22).collect();
        let chunks = compute_balanced_chunks(items, 10);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].len(), 10);
        assert_eq!(chunks[1].len(), 6);
        assert_eq!(chunks[2].len(), 6);
    }

    #[test]
    fn test_balanced_chunks_no_runt() {
        // 27 items batch 10 → naïve [10, 10, 7] — 7 >= 5 (ceil(10/2)) → keep as is
        let items: Vec<i32> = (0..27).collect();
        let chunks = compute_balanced_chunks(items, 10);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].len(), 10);
        assert_eq!(chunks[1].len(), 10);
        assert_eq!(chunks[2].len(), 7);
    }

    #[test]
    fn test_balanced_chunks_single_batch() {
        // 8 items batch 10 → [8]
        let items: Vec<i32> = (0..8).collect();
        let chunks = compute_balanced_chunks(items, 10);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].len(), 8);
    }

    #[test]
    fn test_balanced_chunks_empty() {
        let items: Vec<i32> = vec![];
        let chunks = compute_balanced_chunks(items, 10);
        assert_eq!(chunks.len(), 0);
    }

    #[test]
    fn test_balanced_chunks_preserves_order() {
        // Verify that the actual items are preserved in order
        let items: Vec<i32> = (0..22).collect();
        let chunks = compute_balanced_chunks(items, 10);
        let flat: Vec<i32> = chunks.into_iter().flatten().collect();
        let expected: Vec<i32> = (0..22).collect();
        assert_eq!(flat, expected);
    }

    // ── Tests for sentence-aligned chunking ────────────────────────────────

    #[test]
    fn test_sentence_aligned_chunks_never_splits_sentences() {
        // S1 has 3 segments, S2 has 4, S3 has 2.  Batch size 5.
        // S1 (3 items) fits in batch 1.
        // S2 (4 items) would make batch 1 = 7 items > 5 → flush, S2 starts batch 2.
        // S3 (2 items) fits in batch 2 (4+2=6 > 5) → flush, S3 starts batch 3.
        let items = vec![
            (0, "S1_S1".into(), "a".into()),
            (0, "S1_S2".into(), "b".into()),
            (0, "S1_S3".into(), "c".into()),
            (1, "S2_S1".into(), "d".into()),
            (1, "S2_S2".into(), "e".into()),
            (1, "S2_S3".into(), "f".into()),
            (1, "S2_S4".into(), "g".into()),
            (2, "S3_S1".into(), "h".into()),
            (2, "S3_S2".into(), "i".into()),
        ];

        let chunks = compute_sentence_aligned_chunks(items, 5);
        assert_eq!(chunks.len(), 3);
        // Batch 1: S1 (3 items)
        assert_eq!(chunks[0].len(), 3);
        assert!(chunks[0].iter().all(|(idx, _, _)| *idx == 0));
        // Batch 2: S2 (4 items)
        assert_eq!(chunks[1].len(), 4);
        assert!(chunks[1].iter().all(|(idx, _, _)| *idx == 1));
        // Batch 3: S3 (2 items)
        assert_eq!(chunks[2].len(), 2);
        assert!(chunks[2].iter().all(|(idx, _, _)| *idx == 2));
    }

    #[test]
    fn test_sentence_aligned_chunks_packs_small_sentences() {
        // S1 has 2 segments, S2 has 2, S3 has 2. Batch size 5.
        // S1 (2) in batch → S2 (2+2=4 ≤ 5) in batch → S3 (4+2=6 > 5) → flush.
        let items = vec![
            (0, "S1_S1".into(), "a".into()),
            (0, "S1_S2".into(), "b".into()),
            (1, "S2_S1".into(), "c".into()),
            (1, "S2_S2".into(), "d".into()),
            (2, "S3_S1".into(), "e".into()),
            (2, "S3_S2".into(), "f".into()),
        ];

        let chunks = compute_sentence_aligned_chunks(items, 5);
        assert_eq!(chunks.len(), 2);
        // Batch 1: S1 + S2 (4 items)
        assert_eq!(chunks[0].len(), 4);
        // Batch 2: S3 (2 items)
        assert_eq!(chunks[1].len(), 2);
    }

    #[test]
    fn test_sentence_aligned_chunks_large_sentence_exceeds_batch() {
        // A single sentence has 8 segments, batch_size = 5.
        // We never split a sentence, so it becomes its own batch.
        let items = vec![
            (0, "S1_S1".into(), "a".into()),
            (0, "S1_S2".into(), "b".into()),
            (0, "S1_S3".into(), "c".into()),
            (0, "S1_S4".into(), "d".into()),
            (0, "S1_S5".into(), "e".into()),
            (0, "S1_S6".into(), "f".into()),
            (0, "S1_S7".into(), "g".into()),
            (0, "S1_S8".into(), "h".into()),
        ];

        let chunks = compute_sentence_aligned_chunks(items, 5);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].len(), 8);
    }

    #[test]
    fn test_sentence_aligned_chunks_empty() {
        let items: Vec<(usize, String, String)> = vec![];
        let chunks = compute_sentence_aligned_chunks(items, 10);
        assert_eq!(chunks.len(), 0);
    }

    #[test]
    fn test_sentence_aligned_chunks_single_segment_per_sentence() {
        // Falls back to simple batching when every sentence has 1 segment.
        let items: Vec<(usize, String, String)> = (0..5)
            .map(|i| (i, format!("S{}_S1", i + 1), "text".into()))
            .collect();

        let chunks = compute_sentence_aligned_chunks(items, 3);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 3); // S1, S2, S3
        assert_eq!(chunks[1].len(), 2); // S4, S5
    }
}
