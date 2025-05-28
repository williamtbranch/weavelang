//*** START FILE: src/parsing/llm_parser.rs ***//
use crate::types::llm_data::*; // Use the structs from the new types module
use regex::Regex;

// This enum stays local to the parser's logic
#[derive(Debug, PartialEq, Clone, Copy)]
enum ParsingSection { None, AdvS, SimS, SimE, SimSSegments, PhraseAlign, SimSL, AdvSL, DiglotMap, LockedPhrase }

pub fn parse_llm_text_to_chapter(source_file_name: &str, llm_content: &str) -> Result<ProcessedChapter, String> {
    let mut chapter = ProcessedChapter { source_file_name: source_file_name.to_string(), sentences: Vec::new() };
    let base_sentence_id = source_file_name.replace(".llm.txt", "");
    
    let sentence_blocks: Vec<&str> = llm_content
        .split("END_SENTENCE")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    if sentence_blocks.is_empty() && !llm_content.trim().is_empty() { 
        return Err("No processable blocks found (missing END_SENTENCE markers or empty content between them).".to_string());
    }

    for (index, block_str) in sentence_blocks.iter().enumerate() {
        if block_str.starts_with("CHAPTER_MARKER_DIRECT::") || block_str.starts_with("//") {
            continue;
        }

        let mut sentence = ProcessedSentence { sentence_id: format!("{}_{}", base_sentence_id, index + 1), ..Default::default() };
        let mut current_section = ParsingSection::None;
        
        for line in block_str.lines() {
            let line_trimmed = line.trim();
            if line_trimmed.is_empty() { continue; }

            let mut is_marker_line = true; 
            match line_trimmed {
                s if s.starts_with("AdvS::") => { current_section = ParsingSection::AdvS; sentence.adv_s = s.trim_start_matches("AdvS::").trim().to_string(); }
                s if s.starts_with("SimS::") => { current_section = ParsingSection::SimS; sentence.sim_s = s.trim_start_matches("SimS::").trim().to_string(); }
                s if s.starts_with("SimE::") => { current_section = ParsingSection::SimE; sentence.sim_e = s.trim_start_matches("SimE::").trim().to_string(); }
                s if s.starts_with("SimS_Segments::") => { current_section = ParsingSection::SimSSegments; }
                s if s.starts_with("PHRASE_ALIGN::") => { current_section = ParsingSection::PhraseAlign; }
                s if s.starts_with("SimSL::") => { current_section = ParsingSection::SimSL; }
                s if s.starts_with("AdvSL::") => { current_section = ParsingSection::AdvSL; 
                    let content_without_marker = s.trim_start_matches("AdvSL::").trim();
                    let lemmas_str_cleaned = if let Some(comment_start) = content_without_marker.find(" //") {
                       content_without_marker[..comment_start].trim_end()
                   } else {
                       content_without_marker
                   };
                   sentence.adv_s_lemmas.extend(lemmas_str_cleaned.split_whitespace().map(String::from));
                }
                s if s.starts_with("DIGLOT_MAP::") => { current_section = ParsingSection::DiglotMap; }
                s if s.starts_with("LOCKED_PHRASE::") => { current_section = ParsingSection::LockedPhrase; 
                    let content_without_marker = s.trim_start_matches("LOCKED_PHRASE::").trim();
                    let ids_str_cleaned = if let Some(comment_start) = content_without_marker.find(" //") {
                        content_without_marker[..comment_start].trim_end()
                    } else {
                        content_without_marker
                    };
                    if !ids_str_cleaned.is_empty() {
                        sentence.locked_phrases = Some(ids_str_cleaned.split_whitespace().map(String::from).collect());
                    }
                }
                _ => { is_marker_line = false; } 
            }

            if is_marker_line { 
                continue;
            }

            match current_section {
                ParsingSection::AdvS => sentence.adv_s.push_str(&format!(" {}", line_trimmed)),
                ParsingSection::SimS => sentence.sim_s.push_str(&format!(" {}", line_trimmed)),
                ParsingSection::SimE => sentence.sim_e.push_str(&format!(" {}", line_trimmed)),
                ParsingSection::SimSSegments => {
                    let re = Regex::new(r"^(S\d+)\((.*?)\)$").unwrap();
                    if let Some(caps) = re.captures(line_trimmed) {
                        sentence.sim_s_segments.push(SegmentData {
                            id: caps.get(1).map_or_else(String::new, |m| m.as_str().to_string()),
                            text: caps.get(2).map_or_else(String::new, |m| m.as_str().trim().to_string()),
                        });
                    } else if !line_trimmed.is_empty() {
                        eprintln!("Warning: Malformed SimS_Segments line: '{}' in block for ID {}", line_trimmed, sentence.sentence_id);
                    }
                }
                ParsingSection::PhraseAlign => {
                    let parts: Vec<&str> = line_trimmed.split('~').map(|x| x.trim()).collect();
                    if parts.len() == 3 {
                        sentence.phrase_alignments.push(PhraseAlignment {
                            segment_id: parts[0].to_string(),
                            adv_s_span: parts[1].to_string(),
                            sim_e_span: parts[2].to_string(),
                        });
                    } else if !line_trimmed.is_empty() {
                         eprintln!("Warning: Malformed PHRASE_ALIGN line: '{}' in block for ID {}", line_trimmed, sentence.sentence_id);
                    }
                }
                ParsingSection::SimSL => {
                    let parts: Vec<&str> = line_trimmed.splitn(2, "::").map(|x| x.trim()).collect();
                    if parts.len() == 2 {
                        let segment_id_str = parts[0];
                        let lemmas_str_raw = parts[1];
                        let lemmas_str_cleaned = if let Some(comment_start) = lemmas_str_raw.find(" //") {
                            lemmas_str_raw[..comment_start].trim_end()
                        } else {
                            lemmas_str_raw
                        };
                        sentence.sim_s_lemmas.push(SegmentLemmas {
                            segment_id: segment_id_str.to_string(),
                            lemmas: lemmas_str_cleaned.split_whitespace().map(String::from).collect(),
                        });
                    } else if !line_trimmed.is_empty() && line_trimmed.starts_with('S') {
                         eprintln!("Warning: Malformed SimSL line: '{}' in block for ID {}", line_trimmed, sentence.sentence_id);
                    }
                }
                ParsingSection::AdvSL => {
                    if !line_trimmed.is_empty() {
                        eprintln!("Warning: Unexpected content line '{}' under AdvSL section for ID {}. AdvSL should be single line.", line_trimmed, sentence.sentence_id);
                    }
                }
                ParsingSection::DiglotMap => {
                    let parts: Vec<&str> = line_trimmed.splitn(2, "::").map(|x| x.trim()).collect();
                    if parts.len() == 2 {
                        let segment_id_str = parts[0];
                        let entries_str_raw = parts[1];
                        let entries_str_cleaned = if let Some(comment_start) = entries_str_raw.find(" //") {
                            entries_str_raw[..comment_start].trim_end()
                        } else {
                            entries_str_raw
                        };

                        let mut current_segment_map = DiglotSegmentMap { segment_id: segment_id_str.to_string(), entries: Vec::new() };
                        let entry_re = Regex::new(r"^(.*?)->(.*?)\((.*?)\)\s*\(([YNyn])\)$").unwrap();

                        for entry_part_str in entries_str_cleaned.split('|').map(|e| e.trim()) {
                            if entry_part_str.is_empty() { continue; }
                            if let Some(caps) = entry_re.captures(entry_part_str) {
                                let eng_word = caps.get(1).map_or("", |m| m.as_str().trim()).to_string();
                                let spa_lemma = caps.get(2).map_or("", |m| m.as_str().trim()).to_string();
                                let exact_spa_form = caps.get(3).map_or("", |m| m.as_str().trim()).to_string();
                                let viability_char_str = caps.get(4).map_or("N", |m| m.as_str());
                                
                                if eng_word.is_empty() && spa_lemma.is_empty() && exact_spa_form.is_empty() {
                                     eprintln!("Warning: Parsed completely empty diglot entry (Eng, Spa, Form all empty) for segment {} from part '{}'. Skipping.", segment_id_str, entry_part_str);
                                     continue;
                                }
                                current_segment_map.entries.push(DiglotEntry {
                                    eng_word, spa_lemma, exact_spa_form,
                                    viable: viability_char_str.eq_ignore_ascii_case("Y"),
                                });
                            } else {
                                eprintln!("Warning: Could not parse diglot entry part: '{}' for segment {} in block ID {}", entry_part_str, segment_id_str, sentence.sentence_id);
                            }
                        }
                        sentence.diglot_map.push(current_segment_map);
                    } else if !line_trimmed.is_empty() && line_trimmed.starts_with('S') {
                         eprintln!("Warning: Malformed DIGLOT_MAP S-ID line: '{}' in block for ID {}", line_trimmed, sentence.sentence_id);
                    }
                }
                ParsingSection::LockedPhrase => {
                    if !line_trimmed.is_empty() {
                         eprintln!("Warning: Unexpected content line '{}' under LockedPhrase section for ID {}. LockedPhrase should be single line.", line_trimmed, sentence.sentence_id);
                    }
                }
                ParsingSection::None => {
                     eprintln!("Warning: Content found ('{}') before any section marker in block for ID {}", line_trimmed, sentence.sentence_id);
                }
            }
        }
        if sentence.adv_s.is_empty() && sentence.sim_s.is_empty() && sentence.sim_e.is_empty() && sentence.sim_s_segments.is_empty() {
            eprintln!("Warning: Sentence ID {} appears to be mostly empty or malformed after parsing. Key fields are empty.", sentence.sentence_id);
        }
        chapter.sentences.push(sentence);
    }
    Ok(chapter)
}
//*** END FILE: src/parsing/llm_parser.rs ***//