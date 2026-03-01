use once_cell::sync::Lazy;
use regex::Regex;

// Regex constants ported from gutenberg_cleaner.py
static SINGLE_LINE_BRACKET_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\[(?:Illustration|Copyright|Etext|Project Gutenberg|PG|etext)[^\]\n]*?\]")
        .unwrap()
});

static CHAPTER_HEADING_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(?:CHAPTER\s+[IVXLCDM\d]+(?:\]|\.)?|PREFACE\.?|CONTENTS\.?|LIST OF ILLUSTRATIONS\.?|EPILOGUE\.?|PROLOGUE\.?|[A-Z][A-Z\s]{3,}[A-Z]\.?)$")
        .unwrap()
});

static START_BOOK_MARKER_PREFIX: &str = "*** START OF THE PROJECT GUTENBERG EBOOK";
static END_BOOK_MARKER_PREFIX: &str = "*** END OF THE PROJECT GUTENBERG EBOOK";

pub struct GutenbergCleaner;

impl GutenbergCleaner {
    pub fn clean_text(raw_content: &str) -> String {
        // Normalize newlines: convert CRLF and CR to LF so downstream logic
        // (paragraph splitting and stanza segmentation) sees consistent newlines.
        let normalized = raw_content.replace("\r\n", "\n").replace('\r', "\n");
        let lines: Vec<&str> = normalized.lines().collect();
        let mut book_lines = Vec::new();

        // 1. Locate Book Content
        let mut in_book_content = false;
        let mut found_start = false;

        for line in &lines {
            let trimmed = line.trim();
            if trimmed.starts_with(START_BOOK_MARKER_PREFIX) {
                in_book_content = true;
                found_start = true;
                continue;
            }
            if trimmed.starts_with(END_BOOK_MARKER_PREFIX) {
                in_book_content = false;
                break;
            }
            if in_book_content {
                book_lines.push(*line);
            }
        }

        if !found_start {
            // Fallback: use all lines if no markers found
            book_lines = lines;
        }

        // 2. Remove Multiline Illustrations
        let lines_no_illustrations = Self::remove_multiline_illustration_blocks(&book_lines);

        // 3. Process Paragraphs (handle hard wrapping)
        Self::process_lines_for_paragraphs(lines_no_illustrations)
    }

    fn remove_multiline_illustration_blocks<'a>(lines: &[&'a str]) -> Vec<&'a str> {
        let mut output_lines = Vec::new();
        let mut in_illustration_block = false;
        let block_start_chars = ["[illustration", "[Illustration:"]; 

        for line in lines {
            let trimmed = line.trim();
            let lower_trimmed = trimmed.to_lowercase();
            
            let is_block_start = block_start_chars.iter().any(|prefix| lower_trimmed.starts_with(&prefix.to_lowercase()));

            if in_illustration_block {
                if trimmed.ends_with(']') {
                    in_illustration_block = false;
                }
                output_lines.push(""); // Keep as blank line to preserve paragraph breaks
            } else if is_block_start {
                if !trimmed.ends_with(']') {
                    in_illustration_block = true;
                }
                output_lines.push(""); // Keep as blank line
            } else {
                output_lines.push(*line);
            }
        }
        output_lines
    }

    fn process_lines_for_paragraphs(lines: Vec<&str>) -> String {
        let mut output_paragraphs = Vec::new();
        let mut current_paragraph_lines: Vec<String> = Vec::new();

        for line_raw in lines {
            // Collapse stray carriage returns that may still be embedded.
            // (Most are normalized above, but guard defensively.)
            let mut line = line_raw.replace('\r', "\n");
            // Clean line of any remaining single-line bracket markers
            let line_cleaned_cow = SINGLE_LINE_BRACKET_REGEX.replace_all(&line, "");
            // Replace internal newlines in a physical line with spaces so a
            // single logical paragraph isn't artificially split by CRs.
            let line_cleaned_owned = line_cleaned_cow.replace('\n', " ");
            let line_cleaned = line_cleaned_owned.trim();
            let is_actually_blank = line.trim().is_empty();
            let became_blank = line_cleaned.is_empty() && !is_actually_blank;

            // Handle Chapter Headings or Paragraph boundaries
            let is_chapter = !line_cleaned.is_empty() && CHAPTER_HEADING_REGEX.is_match(line_cleaned);
            
            if is_actually_blank || became_blank || is_chapter {
                if !current_paragraph_lines.is_empty() {
                    output_paragraphs.push(current_paragraph_lines.join(" "));
                    current_paragraph_lines.clear();
                }
                
                if is_chapter {
                    output_paragraphs.push(line_cleaned.to_string());
                }
                continue;
            }

            // Content line - add to current paragraph buffer
            if let Some(last_line) = current_paragraph_lines.last_mut() {
                // Check if the last line ends with a hyphen
                if last_line.ends_with('-') {
                    *last_line = last_line.trim_end_matches('-').to_string();
                    last_line.push_str(line_cleaned);
                } else {
                    last_line.push(' ');
                    last_line.push_str(line_cleaned);
                }
            } else {
                current_paragraph_lines.push(line_cleaned.to_string());
            }
        }

        // Push any remaining buffered lines
        if !current_paragraph_lines.is_empty() {
            output_paragraphs.push(current_paragraph_lines.join(" "));
        }

        output_paragraphs.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_hard_wrapped_sentences() {
        let raw = "My father’s family name being Pirrip, and my Christian name Philip, my\ninfant tongue could make of both names nothing longer or more explicit\nthan Pip.";
        let cleaned = GutenbergCleaner::clean_text(raw);
        assert_eq!(cleaned, "My father’s family name being Pirrip, and my Christian name Philip, my infant tongue could make of both names nothing longer or more explicit than Pip.");
    }

    #[test]
    fn test_clean_paragraph_breaks() {
        let raw = "Paragraph one.\n\nParagraph two.";
        let cleaned = GutenbergCleaner::clean_text(raw);
        assert_eq!(cleaned, "Paragraph one.\nParagraph two.");
    }

    #[test]
    fn test_clean_illustration_block_user_example() {
        let raw = "Mr. Bennet made no answer.\n\n[Illustration:\n\n“He came down to see the place”\n\n[_Copyright 1894 by George Allen._]]\n\nThis was invitation enough.";
        let cleaned = GutenbergCleaner::clean_text(raw);
        let expected = "Mr. Bennet made no answer.\nThis was invitation enough.";
        assert_eq!(cleaned, expected);
    }

    #[test]
    fn test_clean_single_line_illustration() {
        let raw = "Text before.\n[Illustration: Some pic]\nText after.";
        let cleaned = GutenbergCleaner::clean_text(raw);
        assert_eq!(cleaned, "Text before.\nText after.");
    }

    #[test]
    fn test_normalize_carriage_return_within_quote() {
        let raw = "\u{201c}But it is,\u{201d} returned she; \u{201c}for Mrs.\r\nLong has just been here, and she told me all about it.\u{201d}";
        let cleaned = GutenbergCleaner::clean_text(raw);
        // Should collapse the CRLF into a space so the sentence stays together
        assert!(cleaned.contains("Mrs. Long has just been here"));
    }

    #[test]
    fn test_preserve_two_quoted_sentences_on_separate_lines() {
        let raw = "\u{201c}What is his name?\u{201d}\n\u{201c}Bingley.\u{201d}";
        let cleaned = GutenbergCleaner::clean_text(raw);
        // Lines should be joined with a space; segmentation later will split into two sentences.
        assert_eq!(cleaned, "\u{201c}What is his name?\u{201d} \u{201c}Bingley.\u{201d}");
    }

    #[test]
    fn test_preserve_inline_brackets() {
        // Brackets inside a sentence should be preserved
        let raw = "He said [quietly] that it was time.";
        let cleaned = GutenbergCleaner::clean_text(raw);
        assert_eq!(cleaned, "He said [quietly] that it was time.");
    }

    #[test]
    fn test_preserve_brackets_not_at_start() {
        // Brackets appearing later in the line should be preserved
        let raw = "See the note [Note 1] below.";
        let cleaned = GutenbergCleaner::clean_text(raw);
        assert_eq!(cleaned, "See the note [Note 1] below.");
    }

    #[test]
    fn test_start_end_markers() {
        let raw = "Junk before.\n*** START OF THE PROJECT GUTENBERG EBOOK GREAT EXPECTATIONS ***\nReal content.\n*** END OF THE PROJECT GUTENBERG EBOOK GREAT EXPECTATIONS ***\nJunk after.";
        let cleaned = GutenbergCleaner::clean_text(raw);
        assert_eq!(cleaned, "Real content.");
    }

    #[test]
    fn test_headers_and_footers_stripped_default() {
        // If start/end markers are missing, we default to keeping content, 
        // but explicit regexes might still strip known junk if we added them.
        // Currently we only strip illustrations/metadata brackets.
        let raw = "Content.";
        let cleaned = GutenbergCleaner::clean_text(raw);
        assert_eq!(cleaned, "Content.");
    }
}
