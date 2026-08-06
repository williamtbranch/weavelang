// src/services/illustration/llm.rs
//
// Thin wrapper over GeminiClient for the illustration stages.
//
// Adds three things the raw client does not provide: robust JSON extraction
// (models wrap objects in prose or markdown fences), model fallback, and a
// bounded thread pool so per-segment calls run concurrently instead of the
// strictly sequential loop the Python implementation used.

use serde::de::DeserializeOwned;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::services::llm_client::GeminiClient;

pub struct IllustrationLlm {
    client: GeminiClient,
    model: String,
    fallbacks: Vec<String>,
}

impl IllustrationLlm {
    pub fn new(cache_root: Option<PathBuf>, model: &str, fallbacks: Vec<String>) -> Self {
        Self {
            client: GeminiClient::new(cache_root, None),
            model: model.to_string(),
            fallbacks,
        }
    }

    /// Raw completion with model fallback. Responses are cached on disk by the
    /// client, keyed on (model, system, user), so re-runs are free.
    pub fn complete(&self, system: &str, user: &str) -> Result<String, String> {
        let mut last_err = String::new();
        let mut models = vec![self.model.clone()];
        models.extend(self.fallbacks.iter().cloned());
        for m in models {
            match self.client.complete(&m, system, user) {
                Ok(text) => return Ok(text),
                Err(e) => last_err = format!("{}: {}", m, e),
            }
        }
        Err(last_err)
    }

    /// Completion parsed into `T`. Retries once with an explicit correction
    /// instruction when the first response does not parse.
    pub fn complete_json<T: DeserializeOwned>(&self, system: &str, user: &str) -> Result<T, String> {
        let raw = self.complete(system, user)?;
        match parse_json::<T>(&raw) {
            Ok(v) => Ok(v),
            Err(first_err) => {
                let retry_system = format!(
                    "{}\n\nYour previous response could not be parsed ({}). Respond with ONLY \
                     valid JSON matching the schema. No markdown fences, no commentary.",
                    system, first_err
                );
                let raw2 = self.complete(&retry_system, user)?;
                parse_json::<T>(&raw2).map_err(|e| {
                    format!("JSON parse failed after retry: {} (first attempt: {})", e, first_err)
                })
            }
        }
    }
}

/// Extract a JSON value from a model response that may be fenced or prefixed.
pub fn parse_json<T: DeserializeOwned>(raw: &str) -> Result<T, String> {
    let cleaned = strip_fences(raw);
    if let Ok(v) = serde_json::from_str::<T>(&cleaned) {
        return Ok(v);
    }
    // Fall back to the outermost brace-delimited span.
    let start = cleaned.find(['{', '[']);
    let end = cleaned.rfind(['}', ']']);
    if let (Some(s), Some(e)) = (start, end) {
        if e > s {
            if let Ok(v) = serde_json::from_str::<T>(&cleaned[s..=e]) {
                return Ok(v);
            }
        }
    }
    Err(format!(
        "no parsable JSON in response (first 200 chars: {})",
        cleaned.chars().take(200).collect::<String>()
    ))
}

fn strip_fences(raw: &str) -> String {
    let t = raw.trim();
    if !t.starts_with("```") {
        return t.to_string();
    }
    let without_open = t.trim_start_matches("```");
    let without_lang = match without_open.find('\n') {
        Some(i) => &without_open[i + 1..],
        None => without_open,
    };
    without_lang.trim_end().trim_end_matches("```").trim().to_string()
}

/// Run `f` over `items` on up to `concurrency` threads, preserving input order.
///
/// `progress` is called once per completed item with (done, total).
pub fn map_concurrent<I, O, F, P>(
    items: Vec<I>,
    concurrency: usize,
    cancel: Option<&AtomicBool>,
    f: F,
    progress: P,
) -> Vec<Option<O>>
where
    I: Send + Sync,
    O: Send,
    F: Fn(usize, &I) -> Option<O> + Sync,
    P: Fn(usize, usize) + Sync,
{
    let total = items.len();
    if total == 0 {
        return Vec::new();
    }
    let workers = concurrency.clamp(1, 32).min(total);

    let results: Mutex<Vec<Option<O>>> = Mutex::new((0..total).map(|_| None).collect());
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);

    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
                    return;
                }
                let idx = next.fetch_add(1, Ordering::SeqCst);
                if idx >= total {
                    return;
                }
                let out = f(idx, &items[idx]);
                if let Ok(mut guard) = results.lock() {
                    guard[idx] = out;
                }
                let n = done.fetch_add(1, Ordering::SeqCst) + 1;
                progress(n, total);
            });
        }
    });

    results.into_inner().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Sample {
        name: String,
        count: u32,
    }

    #[test]
    fn parses_bare_json() {
        let v: Sample = parse_json(r#"{"name":"a","count":2}"#).unwrap();
        assert_eq!(v, Sample { name: "a".into(), count: 2 });
    }

    #[test]
    fn parses_json_wrapped_in_markdown_fences() {
        let raw = "```json\n{\"name\":\"a\",\"count\":2}\n```";
        let v: Sample = parse_json(raw).unwrap();
        assert_eq!(v.count, 2);
    }

    #[test]
    fn parses_json_with_surrounding_commentary() {
        let raw = "Here you go:\n{\"name\":\"a\",\"count\":2}\nHope that helps!";
        let v: Sample = parse_json(raw).unwrap();
        assert_eq!(v.name, "a");
    }

    #[test]
    fn reports_an_error_when_there_is_no_json() {
        let r = parse_json::<Sample>("I cannot help with that.");
        assert!(r.is_err());
    }

    #[test]
    fn map_concurrent_preserves_input_order() {
        let items: Vec<usize> = (0..50).collect();
        let out = map_concurrent(items, 8, None, |_, v| Some(v * 2), |_, _| {});
        let got: Vec<usize> = out.into_iter().map(|o| o.unwrap()).collect();
        assert_eq!(got, (0..50).map(|v| v * 2).collect::<Vec<_>>());
    }

    #[test]
    fn map_concurrent_handles_failures_as_none() {
        let items: Vec<usize> = (0..10).collect();
        let out = map_concurrent(
            items,
            4,
            None,
            |_, v| if v % 2 == 0 { Some(*v) } else { None },
            |_, _| {},
        );
        assert_eq!(out.iter().filter(|o| o.is_some()).count(), 5);
    }

    #[test]
    fn map_concurrent_stops_on_cancel() {
        let cancel = AtomicBool::new(true);
        let items: Vec<usize> = (0..100).collect();
        let out = map_concurrent(items, 4, Some(&cancel), |_, v| Some(*v), |_, _| {});
        assert!(out.iter().all(|o| o.is_none()));
    }

    #[test]
    fn map_concurrent_on_empty_input_returns_empty() {
        let out = map_concurrent(Vec::<usize>::new(), 4, None, |_, v| Some(*v), |_, _| {});
        assert!(out.is_empty());
    }
}
