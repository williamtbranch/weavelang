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
        if tokens.is_empty() { tokens.push(Token::Background(String::new())); }

        Self { tokens, next_word_id_counter: word_id_counter }
    }

    pub fn from_tokens(tokens: Vec<Token>) -> Self {
        let max_id = tokens.iter().filter_map(|t| match t { Token::Word(w) => Some(w.id.0), _ => None }).max().unwrap_or(0);
        Self { tokens, next_word_id_counter: max_id + 1 }
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
                let lemmas = if !token.lemma.is_empty() { vec![token.lemma] } else { vec![] };
                initial_tokens.push(Token::Word(WordData::new(
                    WordId(word_id_counter),
                    token.text,
                    lemmas
                )));
                word_id_counter += 1;

                if !token.whitespace.is_empty() {
                    initial_tokens.push(Token::Background(token.whitespace));
                }
            }
        }

        // 2. LEGACY PIPELINE FUSION LOGIC (Refactored for Borrow Checker)
        let mut fused_tokens = Vec::new();
        let mut i = 0;
        
        while i < initial_tokens.len() {
            let current = initial_tokens[i].clone();
            
            if let Token::Word(mut w_curr) = current {
                let mut advance_by = 1; // Default: just consume this word
                
                if i + 1 < initial_tokens.len() {
                    match &initial_tokens[i+1] {
                        Token::Word(w_next) => {
                            // W + W -> Fuse
                            w_curr.text.push_str(&w_next.text);
                            w_curr.lemmas.extend(w_next.lemmas.clone());
                            advance_by = 2;
                        },
                        Token::Background(b) if b.is_empty() => {
                            // W + Empty B + W -> Fuse
                            if i + 2 < initial_tokens.len() {
                                if let Token::Word(w_next) = &initial_tokens[i+2] {
                                    w_curr.text.push_str(&w_next.text);
                                    w_curr.lemmas.extend(w_next.lemmas.clone());
                                    advance_by = 3;
                                }
                            }
                        },
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
                },
                Token::Word(_) if last_is_word => {
                    final_tokens.push(Token::Background(String::new()));
                    final_tokens.push(t);
                },
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
    
    pub fn tokens(&self) -> &[Token] { &self.tokens }
    
    pub fn modify_word_text(&mut self, id: WordId, new_text: String) -> Result<(), String> {
        if new_text.trim().is_empty() { return Err("Cannot set word text to empty/whitespace.".to_string()); }
        for token in self.tokens.iter_mut() {
            if let Token::Word(data) = token {
                if data.id == id { data.text = new_text; return Ok(()); }
            }
        }
        Err(format!("WordId {:?} not found.", id))
    }
    
    pub fn delete_word(&mut self, id: WordId) -> Result<(), String> {
        let idx = self.tokens.iter().position(|t| match t { Token::Word(data) => data.id == id, _ => false });
        match idx {
            Some(i) => {
                if i + 1 >= self.tokens.len() || i == 0 { return Err("Critical invariant failure".to_string()); }
                let next_bg_text = match &self.tokens[i + 1] { Token::Background(s) => s.clone(), _ => return Err("Invariant check failed".to_string()) };
                self.tokens.remove(i + 1); self.tokens.remove(i); 
                match &mut self.tokens[i - 1] { Token::Background(s) => s.push_str(&next_bg_text), _ => return Err("Invariant check failed".to_string()) }
                Ok(())
            },
            None => Err(format!("WordId {:?} not found.", id)),
        }
    }
    
    pub fn word_count(&self) -> usize { self.tokens.iter().filter(|t| matches!(t, Token::Word(_))).count() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::python_bridge::RawSpacyToken;

    fn raw_w(text: &str, lemma: &str) -> RawSpacyToken {
        RawSpacyToken { text: text.into(), lemma: lemma.into(), pos: "NOUN".into(), is_punct: false, is_space: false, whitespace: "".into() }
    }
    fn raw_b(text: &str) -> RawSpacyToken {
        RawSpacyToken { text: text.into(), lemma: "".into(), pos: "PUNCT".into(), is_punct: true, is_space: false, whitespace: "".into() }
    }
    fn raw_ws(text: &str) -> RawSpacyToken {
        RawSpacyToken { text: text.into(), lemma: "".into(), pos: "SPACE".into(), is_punct: false, is_space: true, whitespace: "".into() }
    }

    #[test]
    fn test_bwb_invariant_simple() {
        let raw = vec![raw_w("Hello", "hello"), raw_ws(" "), raw_w("world", "world")];
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
        match &ts.tokens[2] { Token::Background(s) => assert_eq!(s, "."), _ => panic!(), }
    }

    #[test]
    fn test_em_dash_separation() {
        let raw = vec![raw_w("one", "one"), raw_b("—"), raw_w("not", "not")];
        let ts = TokenStream::from_raw_spacy(raw, "one—not");
        assert_eq!(ts.tokens.len(), 5);
        match &ts.tokens[2] { Token::Background(s) => assert_eq!(s, "—"), _ => panic!(), }
    }

    #[test]
    fn test_possessive_fusion_restored() {
        let raw = vec![raw_w("Frank", "frank"), raw_w("'s", "'s")];
        let ts = TokenStream::from_raw_spacy(raw, "Frank's");
        assert_eq!(ts.tokens.len(), 3);
        if let Token::Word(w) = &ts.tokens[1] {
            assert_eq!(w.text, "Frank's");
            assert!(w.lemmas.contains(&"frank".to_string()));
        } else { panic!(); }
    }

    #[test]
    fn test_hyphenated_word_structure() {
        let raw = vec![raw_w("bad", "bad"), raw_b("-"), raw_w("looking", "look")];
        let ts = TokenStream::from_raw_spacy(raw, "bad-looking");
        assert_eq!(ts.tokens.len(), 5);
        if let Token::Background(b) = &ts.tokens[2] { assert_eq!(b, "-"); }
    }
}