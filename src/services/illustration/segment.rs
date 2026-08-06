// src/services/illustration/segment.rs
//
// Text loading and segmentation. Ported from generate_illustration_prompts.py
// so the whole pipeline can run in-process.
//
// Illustration density stays a per-book setting (`sentences_per_illustration`).
// `segment_by = "duration"` exists because create_video.py derives per-image
// screen time from each image's sentence range: equal paragraph counts are not
// equal audio time, so long descriptive prose otherwise sits on screen far
// longer than short dialogue lines.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentBy {
    Sentences,
    Duration,
}

impl SegmentBy {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "duration" | "time" | "audio" => SegmentBy::Duration,
            _ => SegmentBy::Sentences,
        }
    }
}

/// A contiguous run of paragraphs that becomes one illustration.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    /// 0-based, inclusive.
    pub start: usize,
    /// 0-based, exclusive.
    pub end: usize,
    pub text: String,
}

/// Locate the UL0 (pure base-language) text file for a chapter.
///
/// Mirrors the Python fallback chain, including its avoidance of " - Copy" and
/// backup files, which exist in real book directories and silently poisoned
/// earlier runs.
pub fn find_ul0(tts_dir: &Path, book_name: &str, chapter_name: &str) -> Result<PathBuf, String> {
    let exact = tts_dir.join(format!("{}_{}_UL0.txt", book_name, chapter_name));
    if exact.exists() {
        return Ok(exact);
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = fs::read_dir(tts_dir) {
        for e in entries.flatten() {
            let p = e.path();
            let Some(name) = p.file_name().and_then(|n| n.to_str()) else { continue };
            if !name.to_ascii_uppercase().contains("_UL") || !name.to_ascii_lowercase().ends_with(".txt") {
                continue;
            }
            candidates.push(p);
        }
    }
    candidates.sort();

    let chapter_match: Vec<&PathBuf> = candidates
        .iter()
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&format!("{}_{}_UL", book_name, chapter_name)))
        })
        .collect();
    let pool: Vec<&PathBuf> = if chapter_match.is_empty() {
        candidates.iter().collect()
    } else {
        chapter_match
    };

    let is_copy_like = |p: &Path| -> bool {
        let n = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        n.contains(" - copy") || n.contains(" copy") || n.contains("(copy") || n.contains("backup")
    };

    let ul0: Vec<&&PathBuf> = pool
        .iter()
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.to_ascii_uppercase().ends_with("_UL0.TXT"))
        })
        .collect();

    let pick = ul0
        .iter()
        .find(|p| !is_copy_like(p))
        .map(|p| (**p).clone())
        .or_else(|| ul0.first().map(|p| (**p).clone()))
        .or_else(|| pool.iter().find(|p| !is_copy_like(p)).map(|p| (*p).clone()))
        .or_else(|| pool.first().map(|p| (*p).clone()));

    pick.ok_or_else(|| {
        format!(
            "No UL file found in {}. Expected {}_{}_UL0.txt",
            tts_dir.display(),
            book_name,
            chapter_name
        )
    })
}

/// Split weave text into paragraphs (one sentence per paragraph in this format).
pub fn split_paragraphs(text: &str) -> Vec<String> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");

    let mut paragraphs: Vec<String> = Vec::new();
    let mut current = String::new();
    for line in normalized.split('\n') {
        if line.trim().is_empty() {
            if !current.trim().is_empty() {
                paragraphs.push(current.trim().to_string());
            }
            current.clear();
        } else {
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
        }
    }
    if !current.trim().is_empty() {
        paragraphs.push(current.trim().to_string());
    }

    // Hand-edited files sometimes have their blank lines collapsed.
    if paragraphs.len() <= 1 {
        let lines: Vec<String> = normalized
            .split('\n')
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        if lines.len() > paragraphs.len() {
            return lines;
        }
    }
    paragraphs
}

/// `max(ceil(count / per), minimum)`, then optionally capped.
pub fn illustration_count(
    paragraph_count: usize,
    sentences_per: usize,
    minimum: usize,
    cap: usize,
) -> usize {
    if paragraph_count == 0 {
        return 0;
    }
    let per = sentences_per.max(1);
    let mut n = ((paragraph_count + per - 1) / per).max(minimum);
    if cap > 0 {
        n = n.min(cap);
    }
    n.min(paragraph_count)
}

/// Partition paragraphs into `count` segments.
///
/// `Sentences` divides by paragraph count (the historical behaviour, preserved
/// so already-tuned books keep their image counts). `Duration` divides by
/// character count as a proxy for narration time, which distributes screen time
/// evenly instead of giving long prose blocks disproportionate dwell.
pub fn segment(paragraphs: &[String], count: usize, mode: SegmentBy) -> Vec<Segment> {
    if count == 0 || paragraphs.is_empty() {
        return Vec::new();
    }
    let count = count.min(paragraphs.len());

    let bounds: Vec<(usize, usize)> = match mode {
        SegmentBy::Sentences => {
            let n = paragraphs.len();
            (0..count)
                .map(|i| ((i * n) / count, ((i + 1) * n) / count))
                .collect()
        }
        SegmentBy::Duration => {
            let weights: Vec<usize> = paragraphs.iter().map(|p| p.chars().count().max(1)).collect();
            let total: usize = weights.iter().sum();
            let mut bounds = Vec::with_capacity(count);
            let mut start = 0usize;
            let mut acc = 0usize;
            for i in 0..count {
                // Leave at least one paragraph for each remaining segment.
                let remaining_segments = count - i - 1;
                let max_end = paragraphs.len() - remaining_segments;
                let target = (total * (i + 1)) / count;
                let mut end = start;
                while end < max_end && (acc < target || end == start) {
                    acc += weights[end];
                    end += 1;
                }
                bounds.push((start, end.max(start + 1).min(paragraphs.len())));
                start = bounds[i].1;
            }
            // Guarantee full coverage regardless of rounding.
            if let Some(last) = bounds.last_mut() {
                last.1 = paragraphs.len();
            }
            bounds
        }
    };

    bounds
        .into_iter()
        .filter(|(s, e)| e > s)
        .map(|(s, e)| Segment {
            start: s,
            end: e,
            text: paragraphs[s..e].join("\n\n"),
        })
        .collect()
}

/// Surrounding narrative context, so the planner understands a scene it is only
/// shown a slice of.
pub fn build_context(paragraphs: &[String], seg: &Segment, radius: usize) -> String {
    let before_start = seg.start.saturating_sub(radius);
    let after_end = (seg.end + radius).min(paragraphs.len());

    let mut parts = Vec::new();
    if before_start < seg.start {
        parts.push(format!(
            "[PRECEDING CONTEXT]\n{}",
            paragraphs[before_start..seg.start].join("\n")
        ));
    }
    if seg.end < after_end {
        parts.push(format!(
            "[FOLLOWING CONTEXT]\n{}",
            paragraphs[seg.end..after_end].join("\n")
        ));
    }
    parts.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paras(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("Paragraph {}.", i)).collect()
    }

    #[test]
    fn splits_on_blank_lines() {
        let text = "One.\n\nTwo.\n\n\nThree.\n";
        assert_eq!(split_paragraphs(text), vec!["One.", "Two.", "Three."]);
    }

    #[test]
    fn handles_crlf_line_endings() {
        let text = "One.\r\n\r\nTwo.\r\n";
        assert_eq!(split_paragraphs(text), vec!["One.", "Two."]);
    }

    #[test]
    fn falls_back_to_one_line_per_paragraph_when_blank_lines_are_collapsed() {
        let text = "One.\nTwo.\nThree.";
        assert_eq!(split_paragraphs(text), vec!["One.", "Two.", "Three."]);
    }

    #[test]
    fn illustration_count_respects_minimum_and_cap() {
        assert_eq!(illustration_count(100, 50, 3, 0), 3);
        assert_eq!(illustration_count(500, 50, 3, 0), 10);
        assert_eq!(illustration_count(500, 50, 3, 5), 5);
        assert_eq!(illustration_count(2, 50, 3, 0), 2, "cannot exceed paragraph count");
        assert_eq!(illustration_count(0, 50, 3, 0), 0);
    }

    #[test]
    fn sentence_segmentation_covers_every_paragraph_exactly_once() {
        let p = paras(97);
        let segs = segment(&p, 7, SegmentBy::Sentences);
        assert_eq!(segs.len(), 7);
        assert_eq!(segs[0].start, 0);
        assert_eq!(segs.last().unwrap().end, 97);
        for w in segs.windows(2) {
            assert_eq!(w[0].end, w[1].start, "no gaps or overlaps");
        }
    }

    #[test]
    fn duration_segmentation_covers_every_paragraph_exactly_once() {
        let mut p = paras(50);
        // A few very long paragraphs, as in a digression.
        p[10] = "x".repeat(4000);
        p[11] = "y".repeat(4000);
        let segs = segment(&p, 6, SegmentBy::Duration);
        assert_eq!(segs.len(), 6);
        assert_eq!(segs[0].start, 0);
        assert_eq!(segs.last().unwrap().end, 50);
        for w in segs.windows(2) {
            assert_eq!(w[0].end, w[1].start);
        }
    }

    #[test]
    fn duration_segmentation_gives_long_prose_more_images_than_sentence_mode() {
        let mut p = paras(40);
        for i in 0..10 {
            p[i] = "x".repeat(3000);
        }
        let by_duration = segment(&p, 4, SegmentBy::Duration);
        let by_sentence = segment(&p, 4, SegmentBy::Sentences);
        // The heavy block occupies paragraphs 0..10. Duration mode should not
        // swallow all of it into a single segment the way even division does.
        let dur_first = by_duration[0].end;
        let sent_first = by_sentence[0].end;
        assert!(
            dur_first < sent_first,
            "duration mode should cut the heavy block earlier ({} vs {})",
            dur_first,
            sent_first
        );
    }

    #[test]
    fn segment_count_is_clamped_to_available_paragraphs() {
        let p = paras(3);
        assert_eq!(segment(&p, 10, SegmentBy::Sentences).len(), 3);
        assert_eq!(segment(&p, 10, SegmentBy::Duration).len(), 3);
    }

    #[test]
    fn context_window_excludes_the_segment_itself() {
        let p = paras(100);
        let seg = Segment { start: 40, end: 50, text: String::new() };
        let ctx = build_context(&p, &seg, 5);
        assert!(ctx.contains("Paragraph 35."));
        assert!(ctx.contains("Paragraph 54."));
        assert!(!ctx.contains("Paragraph 45."));
    }

    #[test]
    fn context_window_clamps_at_document_edges() {
        let p = paras(10);
        let seg = Segment { start: 0, end: 3, text: String::new() };
        let ctx = build_context(&p, &seg, 25);
        assert!(!ctx.contains("[PRECEDING CONTEXT]"));
        assert!(ctx.contains("[FOLLOWING CONTEXT]"));
    }
}
