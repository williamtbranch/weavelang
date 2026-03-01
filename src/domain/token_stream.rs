// src/domain/token_stream.rs

use crate::domain::primitives::{WordData, WordId};
use crate::services::python_bridge::RawSpacyToken;
use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Token {
    Background(String),
    Word(WordData),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenStream {
    tokens: Vec<Token>,
    next_word_id_counter: u64,
}

impl TokenStream {
    pub fn new(text: &str) -> Self {
        let word_regex = Regex::new(r"[\w]+").unwrap();
        let mut tokens = Vec::new();
        let mut last_end = 0;
        let mut word_id_counter = 0;

        for mat in word_regex.find_iter(text) {
            let bg_text = &text[last_end..mat.start()];
            tokens.push(Token::Background(bg_text.to_string()));
            let word_text = mat.as_str();
            tokens.push(Token::Word(WordData::new(
                WordId(word_id_counter),
                word_text.to_string(),
                Vec::new(),
            )));
            word_id_counter += 1;
            last_end = mat.end();
        }
        let trailing_bg = &text[last_end..];
        tokens.push(Token::Background(trailing_bg.to_string()));
        if tokens.is_empty() {
            tokens.push(Token::Background(String::new()));
        }

        Self {
            tokens,
            next_word_id_counter: word_id_counter,
        }
    }

    pub fn from_tokens(tokens: Vec<Token>) -> Self {
        let max_id = tokens
            .iter()
            .filter_map(|t| match t {
                Token::Word(w) => Some(w.id.0),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        Self {
            tokens,
            next_word_id_counter: max_id + 1,
        }
    }

    pub fn from_raw_spacy(raw_tokens: Vec<RawSpacyToken>, original_text: &str) -> Self {
        if raw_tokens.is_empty() {
            return Self::new(original_text);
        }

        let mut initial_tokens = Vec::new();
        let mut word_id_counter = 0;

        // 1. Raw Conversion
        for token in raw_tokens {
            if token.is_punct || token.is_space {
                let full_bg = format!("{}{}", token.text, token.whitespace);
                initial_tokens.push(Token::Background(full_bg));
            } else {
                let lemmas = if !token.lemma.is_empty() {
                    vec![token.lemma]
                } else {
                    vec![]
                };
                initial_tokens.push(Token::Word(WordData::new(
                    WordId(word_id_counter),
                    token.text,
                    lemmas,
                )));
                word_id_counter += 1;

                if !token.whitespace.is_empty() {
                    initial_tokens.push(Token::Background(token.whitespace));
                }
            }
        }

        // 2. LEGACY PIPELINE FUSION LOGIC (Refactored for Borrow Checker)
        let fused_tokens = Self::fuse_word_tokens(initial_tokens);

        // 3. Strict B-W-B Invariant Enforcement
        let mut final_tokens = Vec::new();
        for t in fused_tokens {
            if final_tokens.is_empty() {
                final_tokens.push(t);
                continue;
            }

            let last_is_bg = matches!(final_tokens.last(), Some(Token::Background(_)));
            let last_is_word = matches!(final_tokens.last(), Some(Token::Word(_)));

            match t {
                Token::Background(ref b_text) if last_is_bg => {
                    if let Some(Token::Background(last_b)) = final_tokens.last_mut() {
                        last_b.push_str(b_text);
                    }
                }
                Token::Word(_) if last_is_word => {
                    final_tokens.push(Token::Background(String::new()));
                    final_tokens.push(t);
                }
                _ => final_tokens.push(t),
            }
        }

        // 4. Pad Start/End
        if let Some(Token::Word(_)) = final_tokens.first() {
            final_tokens.insert(0, Token::Background(String::new()));
        }
        if let Some(Token::Word(_)) = final_tokens.last() {
            final_tokens.push(Token::Background(String::new()));
        }
        if final_tokens.is_empty() {
            final_tokens.push(Token::Background(String::new()));
        }

        Self {
            tokens: final_tokens,
            next_word_id_counter: word_id_counter,
        }
    }

    pub fn full_text(&self) -> String {
        let mut buffer = String::new();
        for token in &self.tokens {
            match token {
                Token::Background(s) => buffer.push_str(s),
                Token::Word(w) => buffer.push_str(&w.text),
            }
        }
        buffer
    }

    pub fn tokens(&self) -> &[Token] {
        &self.tokens
    }

    pub fn modify_word_text(&mut self, id: WordId, new_text: String) -> Result<(), String> {
        if new_text.trim().is_empty() {
            return Err("Cannot set word text to empty/whitespace.".to_string());
        }
        for token in self.tokens.iter_mut() {
            if let Token::Word(data) = token {
                if data.id == id {
                    data.text = new_text;
                    return Ok(());
                }
            }
        }
        Err(format!("WordId {id:?} not found."))
    }

    pub fn delete_word(&mut self, id: WordId) -> Result<(), String> {
        let idx = self.tokens.iter().position(|t| match t {
            Token::Word(data) => data.id == id,
            _ => false,
        });
        match idx {
            Some(i) => {
                if i + 1 >= self.tokens.len() || i == 0 {
                    return Err("Critical invariant failure".to_string());
                }
                let next_bg_text = match &self.tokens[i + 1] {
                    Token::Background(s) => s.clone(),
                    _ => return Err("Invariant check failed".to_string()),
                };
                self.tokens.remove(i + 1);
                self.tokens.remove(i);
                match &mut self.tokens[i - 1] {
                    Token::Background(s) => s.push_str(&next_bg_text),
                    _ => return Err("Invariant check failed".to_string()),
                }
                Ok(())
            }
            None => Err(format!("WordId {id:?} not found.")),
        }
    }

    pub fn word_count(&self) -> usize {
        self.tokens
            .iter()
            .filter(|t| matches!(t, Token::Word(_)))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::python_bridge::RawSpacyToken;

    fn raw_w(text: &str, lemma: &str) -> RawSpacyToken {
        RawSpacyToken {
            text: text.into(),
            lemma: lemma.into(),
            pos: "NOUN".into(),
            is_punct: false,
            is_space: false,
            whitespace: "".into(),
        }
    }
    fn raw_b(text: &str) -> RawSpacyToken {
        RawSpacyToken {
            text: text.into(),
            lemma: "".into(),
            pos: "PUNCT".into(),
            is_punct: true,
            is_space: false,
            whitespace: "".into(),
        }
    }
    fn raw_ws(text: &str) -> RawSpacyToken {
        RawSpacyToken {
            text: text.into(),
            lemma: "".into(),
            pos: "SPACE".into(),
            is_punct: false,
            is_space: true,
            whitespace: "".into(),
        }
    }

    #[test]
    fn test_bwb_invariant_simple() {
        let raw = vec![
            raw_w("Hello", "hello"),
            raw_ws(" "),
            raw_w("world", "world"),
        ];
        let ts = TokenStream::from_raw_spacy(raw, "Hello world");
        assert_eq!(ts.tokens.len(), 5);
        assert!(matches!(ts.tokens[0], Token::Background(_)));
        assert!(matches!(ts.tokens[1], Token::Word(_)));
    }

    #[test]
    fn test_punctuation_is_background() {
        let raw = vec![raw_w("Hello", "hello"), raw_b(".")];
        let ts = TokenStream::from_raw_spacy(raw, "Hello.");
        assert_eq!(ts.tokens.len(), 3);
        match &ts.tokens[2] {
            Token::Background(s) => assert_eq!(s, "."),
            _ => panic!(),
        }
    }

    #[test]
    fn test_em_dash_separation() {
        let raw = vec![raw_w("one", "one"), raw_b("—"), raw_w("not", "not")];
        let ts = TokenStream::from_raw_spacy(raw, "one—not");
        assert_eq!(ts.tokens.len(), 5);
        match &ts.tokens[2] {
            Token::Background(s) => assert_eq!(s, "—"),
            _ => panic!(),
        }
    }

    #[test]
    fn test_possessive_fusion_restored() {
        let raw = vec![raw_w("Frank", "frank"), raw_w("'s", "'s")];
        let ts = TokenStream::from_raw_spacy(raw, "Frank's");
        assert_eq!(ts.tokens.len(), 3);
        if let Token::Word(w) = &ts.tokens[1] {
            assert_eq!(w.text, "Frank's");
            assert!(w.lemmas.contains(&"frank".to_string()));
        } else {
            panic!();
        }
    }

    #[test]
    fn test_hyphenated_word_structure() {
        let raw = vec![raw_w("bad", "bad"), raw_b("-"), raw_w("looking", "look")];
        let ts = TokenStream::from_raw_spacy(raw, "bad-looking");
        assert_eq!(ts.tokens.len(), 5);
        if let Token::Background(b) = &ts.tokens[2] {
            assert_eq!(b, "-");
        }
    }
}

// ------------------------------------------------------------------
// NEW HELPERS & TESTS
// ------------------------------------------------------------------

impl TokenStream {
    pub(crate) fn fuse_word_tokens(initial_tokens: Vec<Token>) -> Vec<Token> {
        let mut fused_tokens = Vec::new();
        let mut i = 0;

        while i < initial_tokens.len() {
            let current = initial_tokens[i].clone();

            if let Token::Word(mut w_curr) = current {
                let mut advance_by = 1; // Default: just consume this word

                if i + 1 < initial_tokens.len() {
                    match &initial_tokens[i + 1] {
                        Token::Word(w_next) => {
                            // W + W -> Fuse
                            w_curr.text.push_str(&w_next.text);
                            w_curr.lemmas.extend(w_next.lemmas.clone());
                            advance_by = 2;
                        }
                        Token::Background(b) if b.is_empty() => {
                            // W + Empty B + W -> Fuse
                            if i + 2 < initial_tokens.len() {
                                if let Token::Word(w_next) = &initial_tokens[i + 2] {
                                    w_curr.text.push_str(&w_next.text);
                                    w_curr.lemmas.extend(w_next.lemmas.clone());
                                    advance_by = 3;
                                }
                            }
                        }
                        _ => {}
                    }
                }

                // Push exactly once
                fused_tokens.push(Token::Word(w_curr));
                i += advance_by;
            } else {
                fused_tokens.push(current);
                i += 1;
            }
        }
        fused_tokens
    }

    /// Fuses W + non-space-B + W triplets into a single W token, iteratively.
    ///
    /// Unlike `fuse_word_tokens` (which handles W+W and W+emptyB+W for contractions),
    /// this function handles cases like "bad" + "-" + "looking" → "bad-looking"
    /// where a non-space background token connects two words.
    ///
    /// Ported from `helper.py::fuse_tokens`.
    pub(crate) fn fuse_across_background(mut tokens: Vec<Token>) -> Vec<Token> {
        let mut i = 0;
        while i + 2 < tokens.len() {
            let should_fuse = match (&tokens[i], &tokens[i + 1], &tokens[i + 2]) {
                (Token::Word(_), Token::Background(bg), Token::Word(_)) => !bg.contains(' '),
                _ => false,
            };

            if should_fuse {
                let bg_text = match &tokens[i + 1] {
                    Token::Background(b) => b.clone(),
                    _ => unreachable!(),
                };
                let (next_text, next_lemmas) = match &tokens[i + 2] {
                    Token::Word(w) => (w.text.clone(), w.lemmas.clone()),
                    _ => unreachable!(),
                };

                if let Token::Word(w) = &mut tokens[i] {
                    w.text = format!("{}{}{}", w.text, bg_text, next_text);
                    w.lemmas.extend(next_lemmas);
                }

                tokens.remove(i + 2);
                tokens.remove(i + 1);
                // Don't increment — re-evaluate from same position for chaining
            } else {
                i += 1;
            }
        }
        tokens
    }

    /// Creates a token stream from raw SpaCy tokens without any fusion.
    ///
    /// This is the Rust equivalent of Python's `create_v2_token_list`:
    /// it classifies tokens as B/W based on SpaCy attributes, merges adjacent
    /// backgrounds, and enforces BWBW padding — but performs NO contraction
    /// or hyphenation fusion.
    ///
    /// Used by pipeline stages that want a simple tokenization without
    /// the linguistic fusion logic.
    pub fn from_raw_spacy_unfused(raw_tokens: Vec<RawSpacyToken>) -> Self {
        if raw_tokens.is_empty() {
            return Self {
                tokens: vec![Token::Background(String::new())],
                next_word_id_counter: 0,
            };
        }

        let mut initial_tokens = Vec::new();
        let mut word_id_counter = 0;

        // 1. Raw Conversion (same as from_raw_spacy step 1)
        for token in raw_tokens {
            if token.is_punct || token.is_space {
                let full_bg = format!("{}{}", token.text, token.whitespace);
                initial_tokens.push(Token::Background(full_bg));
            } else {
                let lemmas = if !token.lemma.is_empty() {
                    vec![token.lemma]
                } else {
                    vec![]
                };
                initial_tokens.push(Token::Word(WordData::new(
                    WordId(word_id_counter),
                    token.text,
                    lemmas,
                )));
                word_id_counter += 1;

                if !token.whitespace.is_empty() {
                    initial_tokens.push(Token::Background(token.whitespace));
                }
            }
        }

        // 2. NO fusion step — that's the whole point of this function

        // 3. B-W-B invariant enforcement (merge adjacent Bs, pad adjacent Ws)
        let mut final_tokens = Vec::new();
        for t in initial_tokens {
            if final_tokens.is_empty() {
                final_tokens.push(t);
                continue;
            }

            let last_is_bg = matches!(final_tokens.last(), Some(Token::Background(_)));
            let last_is_word = matches!(final_tokens.last(), Some(Token::Word(_)));

            match t {
                Token::Background(ref b_text) if last_is_bg => {
                    if let Some(Token::Background(last_b)) = final_tokens.last_mut() {
                        last_b.push_str(b_text);
                    }
                }
                Token::Word(_) if last_is_word => {
                    final_tokens.push(Token::Background(String::new()));
                    final_tokens.push(t);
                }
                _ => final_tokens.push(t),
            }
        }

        // 4. Pad Start/End
        if let Some(Token::Word(_)) = final_tokens.first() {
            final_tokens.insert(0, Token::Background(String::new()));
        }
        if let Some(Token::Word(_)) = final_tokens.last() {
            final_tokens.push(Token::Background(String::new()));
        }
        if final_tokens.is_empty() {
            final_tokens.push(Token::Background(String::new()));
        }

        Self {
            tokens: final_tokens,
            next_word_id_counter: word_id_counter,
        }
    }
}

#[cfg(test)]
mod fusion_tests {
    use super::*;

    // Helper to create a word token quickly
    fn w(text: &str) -> Token {
        Token::Word(WordData::new(WordId(0), text.to_string(), vec![]))
    }

    // Helper to create a background token quickly
    fn b(text: &str) -> Token {
        Token::Background(text.to_string())
    }

    #[test]
    fn test_pre_fuse_finds_and_fuses_contraction() {
        let corrupted_stream = vec![
            b("“"),
            w("What"),
            w("’s"),
            b(" "),
            w("happened"),
            b("."),
        ];

        let fixed_stream = TokenStream::fuse_word_tokens(corrupted_stream);

        let word_tokens: Vec<String> = fixed_stream
            .iter()
            .filter_map(|t| match t {
                Token::Word(wd) => Some(wd.text.clone()),
                _ => None,
            })
            .collect();
        
        assert_eq!(word_tokens, vec!["What’s", "happened"]);
        
        for i in 0..fixed_stream.len() - 1 {
            let t1 = &fixed_stream[i];
            let t2 = &fixed_stream[i+1];
            let same_type = match (t1, t2) {
                (Token::Word(_), Token::Word(_)) => true,
                (Token::Background(_), Token::Background(_)) => true,
                _ => false,
            };
            assert!(!same_type, "BWBWB invariant violation at index {}", i);
        }
    }

    #[test]
    fn test_pre_fuse_finds_and_fuses_with_empty_b_token() {
        let corrupted_stream = vec![
            w("abc"),
            b(""),
            w("def"),
        ];

        let fixed_stream = TokenStream::fuse_word_tokens(corrupted_stream);

        let word_tokens: Vec<String> = fixed_stream
            .iter()
            .filter_map(|t| match t {
                Token::Word(wd) => Some(wd.text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(word_tokens, vec!["abcdef"]);
    }

    #[test]
    fn test_pre_fuse_does_not_fuse_across_a_space() {
        let original_stream = vec![
            w("abc"),
            b(" "),
            w("def"),
        ];
        let processed_stream = TokenStream::fuse_word_tokens(original_stream.clone());
        assert_eq!(processed_stream, original_stream);
    }

    #[test]
    fn test_pre_fuse_does_not_fuse_across_intervening_characters() {
        let original_stream = vec![
            w("abc"),
            b("k"),
            w("def"),
        ];
        let processed_stream = TokenStream::fuse_word_tokens(original_stream.clone());
        assert_eq!(processed_stream, original_stream);
    }

    #[test]
    fn test_pre_fuse_handles_multiple_fusions_in_one_stream() {
        let corrupted_stream = vec![
            w("It"),
            w("'s"),
            b(" "),
            w("a"),
            b(" "),
            w("don"),
            w("'t"),
            b("-"),
            w("miss"),
            b(" "),
            w("event"),
        ];

        let fixed_stream = TokenStream::fuse_word_tokens(corrupted_stream);

        let word_tokens: Vec<String> = fixed_stream
            .iter()
            .filter_map(|t| match t {
                Token::Word(wd) => Some(wd.text.clone()),
                _ => None,
            })
            .collect();
        
        assert_eq!(word_tokens, vec!["It's", "a", "don't", "miss", "event"]);
    }

    #[test]
    fn test_pre_fuse_returns_original_stream_if_no_fusions_needed() {
        let valid_stream = vec![
            b(""),
            w("A"),
            b(" "),
            w("valid"),
            b(" "),
            w("stream"),
            b("."),
        ];
        let processed_stream = TokenStream::fuse_word_tokens(valid_stream.clone());
        assert_eq!(processed_stream, valid_stream);
    }
}
// ------------------------------------------------------------------
// fuse_across_background tests  (ported from helper.py::fuse_tokens)
// ------------------------------------------------------------------
#[cfg(test)]
mod fuse_across_bg_tests {
    use super::*;
    use crate::domain::primitives::{WordData, WordId};

    fn w(text: &str) -> Token {
        Token::Word(WordData::new(WordId(0), text.to_string(), vec![]))
    }
    fn wl(text: &str, lemmas: Vec<&str>) -> Token {
        Token::Word(WordData::new(
            WordId(0),
            text.to_string(),
            lemmas.into_iter().map(String::from).collect(),
        ))
    }
    fn b(text: &str) -> Token {
        Token::Background(text.to_string())
    }

    #[test]
    fn test_fuse_hyphenated_word() {
        // W + "-" + W → single fused W
        let tokens = vec![w("bad"), b("-"), w("looking")];
        let result = TokenStream::fuse_across_background(tokens);
        assert_eq!(result.len(), 1);
        if let Token::Word(wd) = &result[0] {
            assert_eq!(wd.text, "bad-looking");
        } else {
            panic!("Expected a Word token");
        }
    }

    #[test]
    fn test_no_fuse_across_space() {
        // Space-containing background must NOT fuse
        let tokens = vec![w("hello"), b(" "), w("world")];
        let result = TokenStream::fuse_across_background(tokens);
        let words: Vec<&str> = result
            .iter()
            .filter_map(|t| match t {
                Token::Word(wd) => Some(wd.text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(words, vec!["hello", "world"]);
    }

    #[test]
    fn test_fuse_chain_three_words() {
        // W + "-" + W + "-" + W → single fused W (iterative re-evaluation)
        let tokens = vec![w("a"), b("-"), w("b"), b("-"), w("c")];
        let result = TokenStream::fuse_across_background(tokens);
        assert_eq!(result.len(), 1);
        if let Token::Word(wd) = &result[0] {
            assert_eq!(wd.text, "a-b-c");
        } else {
            panic!("Expected a fused Word token");
        }
    }

    #[test]
    fn test_fuse_lemmas_are_merged() {
        let tokens = vec![wl("bad", vec!["bad"]), b("-"), wl("looking", vec!["look"])];
        let result = TokenStream::fuse_across_background(tokens);
        if let Token::Word(wd) = &result[0] {
            assert_eq!(wd.text, "bad-looking");
            assert!(wd.lemmas.contains(&"bad".to_string()));
            assert!(wd.lemmas.contains(&"look".to_string()));
        } else {
            panic!("Expected a Word token");
        }
    }

    #[test]
    fn test_fuse_empty_input() {
        let result = TokenStream::fuse_across_background(vec![]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_fuse_no_change_when_all_spaces() {
        let tokens = vec![
            b(""),
            w("A"),
            b(" "),
            w("valid"),
            b(" "),
            w("stream"),
            b("."),
        ];
        let result = TokenStream::fuse_across_background(tokens.clone());
        assert_eq!(result, tokens);
    }

    #[test]
    fn test_fuse_mixed_space_and_nonspace() {
        // "don't" + " " + "miss" — space prevents fusion;
        // then "miss" + "-" + "event" — hyphen causes fusion
        let tokens = vec![
            w("don't"),
            b(" "),
            w("miss"),
            b("-"),
            w("event"),
        ];
        let result = TokenStream::fuse_across_background(tokens);
        let words: Vec<&str> = result
            .iter()
            .filter_map(|t| match t {
                Token::Word(wd) => Some(wd.text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(words, vec!["don't", "miss-event"]);
    }
}

// ------------------------------------------------------------------
// from_raw_spacy_unfused tests  (ported from helper.py::create_v2_token_list)
// ------------------------------------------------------------------
#[cfg(test)]
mod unfused_tests {
    use super::*;
    use crate::services::python_bridge::RawSpacyToken;

    fn raw_w(text: &str, lemma: &str, ws: &str) -> RawSpacyToken {
        RawSpacyToken {
            text: text.into(),
            lemma: lemma.into(),
            pos: "NOUN".into(),
            is_punct: false,
            is_space: false,
            whitespace: ws.into(),
        }
    }
    fn raw_punct(text: &str, ws: &str) -> RawSpacyToken {
        RawSpacyToken {
            text: text.into(),
            lemma: "".into(),
            pos: "PUNCT".into(),
            is_punct: true,
            is_space: false,
            whitespace: ws.into(),
        }
    }
    fn raw_space(ws: &str) -> RawSpacyToken {
        RawSpacyToken {
            text: ws.into(),
            lemma: "".into(),
            pos: "SPACE".into(),
            is_punct: false,
            is_space: true,
            whitespace: "".into(),
        }
    }

    #[test]
    fn test_unfused_simple_sentence_with_comma() {
        // Mirrors Python test: "A king had a garden,"
        let raw = vec![
            raw_w("A", "a", " "),
            raw_w("king", "king", " "),
            raw_w("had", "have", " "),
            raw_w("a", "a", " "),
            raw_w("garden", "garden", ""),
            raw_punct(",", ""),
        ];
        let ts = TokenStream::from_raw_spacy_unfused(raw);

        // Word tokens should NOT contain punctuation
        let words: Vec<&str> = ts
            .tokens()
            .iter()
            .filter_map(|t| match t {
                Token::Word(w) => Some(w.text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(words, vec!["A", "king", "had", "a", "garden"]);

        // Last token should be background containing the comma
        let last = ts.tokens().last().unwrap();
        if let Token::Background(b) = last {
            assert!(b.contains(','), "Last background should contain comma");
        } else {
            panic!("Last token should be background");
        }

        // Must start and end with background
        assert!(matches!(ts.tokens().first(), Some(Token::Background(_))));
        assert!(matches!(ts.tokens().last(), Some(Token::Background(_))));
    }

    #[test]
    fn test_unfused_no_fusion_on_possessive() {
        // Unlike from_raw_spacy, possessives should NOT be fused
        let raw = vec![
            raw_w("Frank", "frank", ""),
            raw_w("'s", "'s", " "),
            raw_w("son", "son", ""),
        ];
        let ts = TokenStream::from_raw_spacy_unfused(raw);

        let words: Vec<&str> = ts
            .tokens()
            .iter()
            .filter_map(|t| match t {
                Token::Word(w) => Some(w.text.as_str()),
                _ => None,
            })
            .collect();
        // Words stay SEPARATE — no fusion
        assert_eq!(words, vec!["Frank", "'s", "son"]);
    }

    #[test]
    fn test_unfused_empty_input() {
        let ts = TokenStream::from_raw_spacy_unfused(vec![]);
        assert_eq!(ts.tokens().len(), 1);
        assert!(matches!(ts.tokens()[0], Token::Background(ref s) if s.is_empty()));
    }

    #[test]
    fn test_unfused_bwbw_invariant() {
        let raw = vec![
            raw_w("Hello", "hello", " "),
            raw_w("world", "world", ""),
            raw_punct(".", ""),
        ];
        let ts = TokenStream::from_raw_spacy_unfused(raw);

        // Check BWBW alternation
        for i in 0..ts.tokens().len() - 1 {
            let same = match (&ts.tokens()[i], &ts.tokens()[i + 1]) {
                (Token::Word(_), Token::Word(_)) => true,
                (Token::Background(_), Token::Background(_)) => true,
                _ => false,
            };
            assert!(!same, "BWBW invariant violated at index {i}");
        }

        // Starts with B, ends with B
        assert!(matches!(ts.tokens().first(), Some(Token::Background(_))));
        assert!(matches!(ts.tokens().last(), Some(Token::Background(_))));
    }

    #[test]
    fn test_unfused_adjacent_backgrounds_merged() {
        // Comma + space should merge into one background
        let raw = vec![
            raw_w("hello", "hello", ""),
            raw_punct(",", " "),
            raw_w("world", "world", ""),
        ];
        let ts = TokenStream::from_raw_spacy_unfused(raw);

        let words: Vec<&str> = ts
            .tokens()
            .iter()
            .filter_map(|t| match t {
                Token::Word(w) => Some(w.text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(words, vec!["hello", "world"]);

        // The comma and space should be merged into one B token
        if let Token::Background(b) = &ts.tokens()[2] {
            assert_eq!(b, ", ");
        } else {
            panic!("Expected merged background at index 2");
        }
    }
}