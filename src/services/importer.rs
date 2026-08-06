use crate::domain::normalization::{
    clean_italics_and_underscores, BRACKETED_ARABIC_CHAPTER_REGEX, BRACKETED_ROMAN_CHAPTER_REGEX,
    CHAPTER_MAIN_REGEX, EM_DASH_SECTION_REGEX, SHORT_LINE_NUMERAL_CHAPTER_REGEX,
    SPECIAL_SECTION_REGEX,
};
use crate::domain::token_stream::{Token, TokenStream};
use crate::services::python_bridge::PythonBridge;
use crate::services::gutenberg_cleaner::GutenbergCleaner;
use crate::types::json_types::{
    JsonBookMetaV2, JsonChapter, JsonChapterMarkerBlock, JsonContentBlock, JsonMappingsV2,
    JsonSegmentV2, JsonSentenceBlock, JsonTierV2, JsonTokenType, JsonTokenV2,
};
use std::collections::HashMap;

/// Testable helper: import from already-cleaned book text using provided seg/token handlers.
pub(crate) fn import_from_cleaned_with_handlers<FSeg, FTok>(
    cleaned_book_text: &str,
    book_name: &str,
    mut seg_fn: FSeg,
    mut tok_fn: FTok,
) -> Result<JsonChapter, String>
where
    FSeg: FnMut(&str) -> Result<Vec<String>, String>,
    FTok: FnMut(&str) -> Result<Vec<crate::services::python_bridge::RawSpacyToken>, String>,
{
    let mut content_blocks: Vec<JsonContentBlock> = Vec::new();
    let mut s_counter = 1;

    let paragraphs: Vec<&str> = cleaned_book_text.split("\n\n").collect();

    for raw_paragraph in paragraphs {
        let cleaned_text = crate::domain::normalization::clean_italics_and_underscores(raw_paragraph.trim());
        if cleaned_text.is_empty() { continue; }

        if let Some(title) = BookImporter::detect_chapter_heading(&cleaned_text) {
            content_blocks.push(JsonContentBlock::ChapterMarker(JsonChapterMarkerBlock { text: title.clone() }));
            // Also emit the heading as a sentence (matches Python pipeline behavior).
            let s_id = format!("S{}", s_counter);
            s_counter += 1;
            let raw_tokens = tok_fn(&title)?;
            let token_stream = TokenStream::from_raw_spacy(raw_tokens, &title);
            let json_tokens = BookImporter::convert_to_json_tokens(&token_stream);
            let segment = JsonSegmentV2 {
                seg_id: s_id.clone(),
                text: title.clone(),
                tokenized_text: json_tokens,
                lemmas: vec![],
            };
            let tier = JsonTierV2 {
                tier_id: "base".to_string(),
                full_text: title.clone(),
                lemmas: vec![],
                segments: vec![segment],
            };
            content_blocks.push(JsonContentBlock::Sentence(JsonSentenceBlock {
                s_id,
                tiers: vec![tier],
                mappings: JsonMappingsV2::default(),                proper_noun_lemmas: vec![],            }));
            continue;
        }

        let sentences = seg_fn(&cleaned_text)?;

        for sent_text in sentences {
            let s_id = format!("S{}", s_counter);
            s_counter += 1;

            let raw_tokens = tok_fn(&sent_text)?;
            let token_stream = TokenStream::from_raw_spacy(raw_tokens, &sent_text);
            let json_tokens = BookImporter::convert_to_json_tokens(&token_stream);

            let segment = JsonSegmentV2 {
                seg_id: s_id.clone(),
                text: sent_text.clone(),
                tokenized_text: json_tokens,
                lemmas: vec![],
            };

            let tier = JsonTierV2 {
                tier_id: "base".to_string(),
                full_text: sent_text.clone(),
                lemmas: vec![],
                segments: vec![segment],
            };

            content_blocks.push(JsonContentBlock::Sentence(JsonSentenceBlock {
                s_id,
                tiers: vec![tier],
                mappings: JsonMappingsV2::default(),                proper_noun_lemmas: vec![],            }));
        }
    }

    Ok(JsonChapter {
        book_meta: JsonBookMetaV2 {
            book_name: book_name.to_string(),
            schema_version: "2.0".to_string(),
            base_language: "en".to_string(),
            target_language: "es".to_string(),
        },
        content_blocks,
        u_level_maps: HashMap::new(),
    })
}

/// Merge segmentation fragments that contain no alphanumeric content (e.g. a
/// lone closing guillemet `»`, a stray quote, or a bare bracket) into the
/// preceding sentence.
///
/// Some sentence segmenters (notably Spanish/stanza) occasionally split a
/// trailing closing quotation mark onto its own "sentence". Such a fragment is
/// never a real sentence, so we re-attach it directly to the end of the
/// previous sentence to avoid hanging quotes after import.
pub(crate) fn merge_hanging_punctuation(sentences: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(sentences.len());
    for sent in sentences {
        let trimmed = sent.trim();
        let is_punct_only =
            !trimmed.is_empty() && !trimmed.chars().any(|c| c.is_alphanumeric());
        if is_punct_only {
            if let Some(prev) = out.last_mut() {
                // Attach the closer directly to the previous sentence, with no
                // intervening space (e.g. `palabra.» `).
                while prev.ends_with(char::is_whitespace) {
                    prev.pop();
                }
                prev.push_str(trimmed);
                continue;
            }
        }
        out.push(sent);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::python_bridge::RawSpacyToken;

    #[test]
    fn mock_gutenberg_import_produces_base_tokens() {
        let sample = "Chapter I\n\nThis is the first sentence.\n\nThis is the second sentence.";

        let seg = |txt: &str| -> Result<Vec<String>, String> { Ok(vec![txt.to_string()]) };

        let tok = |txt: &str| -> Result<Vec<RawSpacyToken>, String> {
            let parts: Vec<&str> = txt.split_whitespace().collect();
            let tokens = parts
                .into_iter()
                .map(|p| RawSpacyToken { text: p.to_string(), lemma: p.to_string(), pos: "NOUN".to_string(), is_punct: false, is_space: false, whitespace: " ".to_string() })
                .collect();
            Ok(tokens)
        };

        let chapter = crate::services::importer::import_from_cleaned_with_handlers(sample, "testbook", seg, tok).expect("import failed");

        // Ensure at least one sentence block produced
        let sentences: Vec<_> = chapter
            .content_blocks
            .into_iter()
            .filter_map(|b| match b { JsonContentBlock::Sentence(s) => Some(s), _ => None })
            .collect();

        assert!(!sentences.is_empty(), "No sentences produced");

        // Check first sentence has a tier with id "base" and non-empty tokenized_text
        let first = &sentences[0];
        assert!(!first.tiers.is_empty(), "No tiers on sentence");
        let base_tier = &first.tiers[0];
        assert_eq!(base_tier.tier_id, "base");
        assert!(!base_tier.segments[0].tokenized_text.is_empty(), "Token stream empty for base tier");
    }

    #[test]
    fn hanging_closing_guillemet_merges_into_previous_sentence() {
        // Simulates the Spanish/stanza segmenter splitting a closing `»` off
        // onto its own line. It must be re-attached to the previous sentence.
        let input = vec![
            "«Nadie lo vio morir.".to_string(),
            "»".to_string(),
            "Era de noche.".to_string(),
        ];
        let merged = super::merge_hanging_punctuation(input);
        assert_eq!(
            merged,
            vec![
                "«Nadie lo vio morir.»".to_string(),
                "Era de noche.".to_string(),
            ]
        );
    }

    #[test]
    fn merge_leaves_normal_sentences_untouched() {
        let input = vec![
            "Hola mundo.".to_string(),
            "¿Cómo estás?".to_string(),
        ];
        let merged = super::merge_hanging_punctuation(input.clone());
        assert_eq!(merged, input);
    }

    #[test]
    fn merge_keeps_leading_punctuation_fragment_when_no_previous() {
        // A punctuation-only fragment with nothing before it has nowhere to
        // merge; keep it rather than dropping content.
        let input = vec!["»".to_string(), "Texto.".to_string()];
        let merged = super::merge_hanging_punctuation(input.clone());
        assert_eq!(merged, input);
    }
}

pub struct BookImporter;

impl BookImporter {
    pub fn import_from_text(
        raw_text: &str,
        book_name: &str,
        bridge: &mut PythonBridge,
    ) -> Result<JsonChapter, String> {
        // delegated to helper

        // 1. Clean Gutenberg Text (Handles hard wraps, headers, illustrations)
        let cleaned_book_text = GutenbergCleaner::clean_text(raw_text);

        // 2. Initial Paragraph Splitting (now we can trust newlines more)
        let mut content_blocks: Vec<JsonContentBlock> = Vec::new();
        let mut s_counter = 1;
        let paragraphs: Vec<&str> = cleaned_book_text.split("\n\n").collect();

        for raw_paragraph in paragraphs {
            let cleaned_text = clean_italics_and_underscores(raw_paragraph.trim());
            if cleaned_text.is_empty() {
                continue;
            }

            // 3. Chapter Detection
            if let Some(title) = Self::detect_chapter_heading(&cleaned_text) {
                content_blocks.push(JsonContentBlock::ChapterMarker(JsonChapterMarkerBlock { text: title.clone() }));
                // Also emit the heading as a sentence (matches Python pipeline behavior).
                let s_id = format!("S{}", s_counter);
                s_counter += 1;
                let raw_tokens = bridge.tokenize(&title, "en")?;
                let token_stream = TokenStream::from_raw_spacy(raw_tokens, &title);
                let json_tokens = Self::convert_to_json_tokens(&token_stream);
                let segment = JsonSegmentV2 {
                    seg_id: s_id.clone(),
                    text: title.clone(),
                    tokenized_text: json_tokens,
                    lemmas: vec![],
                };
                let tier = JsonTierV2 {
                    tier_id: "base".to_string(),
                    full_text: title.clone(),
                    lemmas: vec![],
                    segments: vec![segment],
                };
                content_blocks.push(JsonContentBlock::Sentence(JsonSentenceBlock {
                    s_id,
                    tiers: vec![tier],
                    mappings: JsonMappingsV2::default(),                    proper_noun_lemmas: vec![],                }));
                continue;
            }

            // 4. Segmentation
            let sentences = bridge.segment(&cleaned_text, "en")?;

            for sent_text in sentences {
                let s_id = format!("S{}", s_counter);
                s_counter += 1;

                // 5. Tokenization & Struct Construction
                let raw_tokens = bridge.tokenize(&sent_text, "en")?;
                let token_stream = TokenStream::from_raw_spacy(raw_tokens, &sent_text);
                let json_tokens = Self::convert_to_json_tokens(&token_stream);

                let segment = JsonSegmentV2 {
                    seg_id: s_id.clone(),
                    text: sent_text.clone(),
                    tokenized_text: json_tokens,
                    lemmas: vec![],
                };

                let tier = JsonTierV2 {
                    tier_id: "base".to_string(),
                    full_text: sent_text.clone(),
                    lemmas: vec![],
                    segments: vec![segment],
                };

                content_blocks.push(JsonContentBlock::Sentence(JsonSentenceBlock {
                    s_id,
                    tiers: vec![tier],
                    mappings: JsonMappingsV2::default(),
                    proper_noun_lemmas: vec![],
                }));
            }
        }

        Ok(JsonChapter {
            book_meta: JsonBookMetaV2 {
                book_name: book_name.to_string(),
                schema_version: "2.0".to_string(),
                base_language: "en".to_string(),
                target_language: "es".to_string(),
            },
            content_blocks,
            u_level_maps: HashMap::new(),
        })
    }

    fn detect_chapter_heading(text: &str) -> Option<String> {
        // Ported logic from raw2stage.py loop
        if let Some(caps) = BRACKETED_ARABIC_CHAPTER_REGEX.captures(text) {
            return Some(format!("Chapter {}", caps.get(1).unwrap().as_str()));
        }
        if let Some(caps) = BRACKETED_ROMAN_CHAPTER_REGEX.captures(text) {
            return Some(format!("Chapter {}", caps.get(1).unwrap().as_str().to_uppercase()));
        }
        if let Some(caps) = EM_DASH_SECTION_REGEX.captures(text) {
            return Some(format!("Chapter {}", caps.get(1).unwrap().as_str().to_uppercase()));
        }
        if let Some(caps) = CHAPTER_MAIN_REGEX.captures(text) {
            let potential_num = caps.get(2).unwrap().as_str().to_uppercase();
            let rest = caps.get(3).map_or("", |m| m.as_str());
            return Some(format!("Chapter {}: {}", potential_num, rest.trim_matches(|c| c == '.' || c == ' ')).trim().to_string());
        }
        if let Some(caps) = SPECIAL_SECTION_REGEX.captures(text) {
             return Some(caps.get(1).unwrap().as_str().trim().to_uppercase()); // Changed Title Case to Upper to match logic somewhat or just keep clean
        }
        if text.len() < 30 {
             if let Some(caps) = SHORT_LINE_NUMERAL_CHAPTER_REGEX.captures(text) {
                 return Some(format!("Chapter {}", caps.get(1).unwrap().as_str().to_uppercase()));
             }
        }
        None
    }

    fn convert_to_json_tokens(stream: &TokenStream) -> Vec<JsonTokenV2> {
        stream.tokens().iter().map(|t| match t {
            Token::Background(s) => JsonTokenV2 {
                token_type: JsonTokenType::Background,
                value: s.clone(),
                ..Default::default()
            },
            Token::Word(w) => JsonTokenV2 {
                token_type: JsonTokenType::Word,
                value: w.text.clone(),
                lemmas: w.lemmas.clone(),
                ..Default::default()
            },
        }).collect()
    }
    

    /// Testable helper: import from already-cleaned book text using provided seg/token handlers.
    #[allow(dead_code)]
    pub(crate) fn import_from_cleaned_with_handlers<FSeg, FTok>(
        cleaned_book_text: &str,
        book_name: &str,
        seg_fn: FSeg,
        tok_fn: FTok,
) -> Result<JsonChapter, String>
where
    FSeg: Fn(&str) -> Result<Vec<String>, String>,
    FTok: Fn(&str) -> Result<Vec<crate::services::python_bridge::RawSpacyToken>, String>,
{
    let mut content_blocks = Vec::new();
    let mut s_counter = 1;

    let paragraphs: Vec<&str> = cleaned_book_text.split("\n\n").collect();

    for raw_paragraph in paragraphs {
        let cleaned_text = crate::domain::normalization::clean_italics_and_underscores(raw_paragraph.trim());
        if cleaned_text.is_empty() { continue; }

        if let Some(title) = BookImporter::detect_chapter_heading(&cleaned_text) {
            content_blocks.push(JsonContentBlock::ChapterMarker(JsonChapterMarkerBlock { text: title.clone() }));
            // Also emit the heading as a sentence (matches Python pipeline behavior).
            let s_id = format!("S{}", s_counter);
            s_counter += 1;
            let raw_tokens = tok_fn(&title)?;
            let token_stream = TokenStream::from_raw_spacy(raw_tokens, &title);
            let json_tokens = BookImporter::convert_to_json_tokens(&token_stream);
            let segment = JsonSegmentV2 {
                seg_id: s_id.clone(),
                text: title.clone(),
                tokenized_text: json_tokens,
                lemmas: vec![],
            };
            let tier = JsonTierV2 {
                tier_id: "base".to_string(),
                full_text: title.clone(),
                lemmas: vec![],
                segments: vec![segment],
            };
            content_blocks.push(JsonContentBlock::Sentence(JsonSentenceBlock {
                s_id,
                tiers: vec![tier],
                mappings: JsonMappingsV2::default(),                proper_noun_lemmas: vec![],            }));
            continue;
        }

        let sentences = seg_fn(&cleaned_text)?;

        for sent_text in sentences {
            let s_id = format!("S{}", s_counter);
            s_counter += 1;

            let raw_tokens = tok_fn(&sent_text)?;
            let token_stream = TokenStream::from_raw_spacy(raw_tokens, &sent_text);
            let json_tokens = BookImporter::convert_to_json_tokens(&token_stream);

            let segment = JsonSegmentV2 {
                seg_id: s_id.clone(),
                text: sent_text.clone(),
                tokenized_text: json_tokens,
                lemmas: vec![],
            };

            let tier = JsonTierV2 {
                tier_id: "base".to_string(),
                full_text: sent_text.clone(),
                lemmas: vec![],
                segments: vec![segment],
            };

            content_blocks.push(JsonContentBlock::Sentence(JsonSentenceBlock {
                s_id,
                tiers: vec![tier],
                mappings: JsonMappingsV2::default(),                proper_noun_lemmas: vec![],            }));
        }
    }

    Ok(JsonChapter {
        book_meta: JsonBookMetaV2 {
            book_name: book_name.to_string(),
            schema_version: "2.0".to_string(),
            base_language: "en".to_string(),
            target_language: "es".to_string(),
        },
        content_blocks,
        u_level_maps: HashMap::new(),
    })
}

    /// Variant that works with the `BridgeService` wrapper (used from the GUI path).
    pub fn import_from_text_with_service(
        raw_text: &str,
        book_name: &str,
        bridge_service: &crate::services::python_bridge::BridgeService,
        lang_code: &str,
    ) -> Result<JsonChapter, String> {
        // We'll call the public `segment` and `tokenize` methods on the BridgeService,
        // which internally lock the Python bridge process.
        // delegated to helper

        // 1. Clean Gutenberg Text (Handles hard wraps, headers, illustrations)
        let cleaned_book_text = crate::services::gutenberg_cleaner::GutenbergCleaner::clean_text(raw_text);

        // Delegate to the handler-based helper so tests can exercise importer behavior
        let lang_owned = lang_code.to_string();
        let lang_seg = lang_owned.clone();
        return crate::services::importer::import_from_cleaned_with_handlers(
            &cleaned_book_text,
            book_name,
            move |txt| {
                bridge_service
                    .segment(txt, &lang_seg)
                    .map(merge_hanging_punctuation)
            },
            move |txt| bridge_service.tokenize(txt, &lang_owned),
        );
    }

}

