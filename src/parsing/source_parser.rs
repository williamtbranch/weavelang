use crate::domain::sentence::Sentence;
use crate::domain::tier::Tier;
use crate::domain::segment::Segment;
use crate::domain::token_stream::TokenStream;
use regex::Regex;
use std::error::Error;

pub fn parse_source_file(content: &str) -> Result<Vec<Sentence>, Box<dyn Error>> {
    let mut document = Vec::new();
    // Regex to capture {S1: Text...}
    // Matches start of line, '{', 'S', digits, ':', whitespace, capture text, '}' at end.
    let s_re = Regex::new(r"^\{S(\d+):\s*(.*)\}$").unwrap();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }

        // We currently ignore %%CHAPTER_MARKER%% lines for the Sentence document,
        // as the text content usually follows in a {S#: Chapter X} line anyway.
        
        if let Some(caps) = s_re.captures(trimmed) {
            let s_id_num = caps.get(1).unwrap().as_str();
            let text = caps.get(2).unwrap().as_str();
            
            let s_id = format!("S{}", s_id_num);
            let mut sentence = Sentence::new(s_id);
            
            // Create the Base Tier
            let mut tier = Tier::new("base".to_string());
            
            // Create a single segment with basic tokenization
            // TokenStream::new() uses a regex to split words/punctuation, 
            // giving us a "Valid" starting state without needing Python yet.
            let segment = Segment::from_stream(
                "S1".to_string(),
                TokenStream::new(text),
                vec![]
            );
            
            tier.add_segment(segment);
            sentence.add_tier(tier);
            
            document.push(sentence);
        }
    }

    Ok(document)
}