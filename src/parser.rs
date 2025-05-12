use serde::{Deserialize, Serialize};
use regex::Regex;

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct SegmentData {
    pub id: String,
    pub text: String,
}
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct PhraseAlignment {
    pub segment_id: String,
    pub adv_s_span: String,
    pub sim_e_span: String,
}
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct SegmentLemmas {
    pub segment_id: String,
    pub lemmas: Vec<String>,
}
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct DiglotEntry {
    pub eng_word: String,
    pub spa_lemma: String,
    pub exact_spa_form: String,
    pub viable: bool,
}
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct DiglotSegmentMap {
    pub segment_id: String,
    pub entries: Vec<DiglotEntry>,
}
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ProcessedSentence {
    pub sentence_id: String,
    pub adv_s: String,
    pub sim_s: String,
    pub sim_e: String,
    pub sim_s_segments: Vec<SegmentData>,
    pub phrase_alignments: Vec<PhraseAlignment>,
    pub sim_s_lemmas: Vec<SegmentLemmas>,
    pub adv_s_lemmas: Vec<String>,
    pub diglot_map: Vec<DiglotSegmentMap>,
    pub locked_phrases: Option<Vec<String>>,
}
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ProcessedChapter {
    pub source_file_name: String,
    pub sentences: Vec<ProcessedSentence>,
}
#[derive(Debug, PartialEq, Clone, Copy)]
enum ParsingSection { None, AdvS, SimS, SimE, SimSSegments, PhraseAlign, SimSL, AdvSL, DiglotMap, LockedPhrase }

pub fn parse_llm_text_to_chapter(source_file_name: &str, llm_content: &str) -> Result<ProcessedChapter, String> {
    let mut chapter = ProcessedChapter { source_file_name: source_file_name.to_string(), sentences: Vec::new() };
    let base_sentence_id = source_file_name.replace(".llm.txt", "");
    let sentence_blocks: Vec<&str> = llm_content.split("END_SENTENCE").map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    if sentence_blocks.is_empty() && !llm_content.trim().is_empty() { return Err("No END_SENTENCE markers found, or content is malformed.".to_string()); }
    for (index, block_str) in sentence_blocks.iter().enumerate() {
        if block_str.trim().is_empty() { continue; }
        let mut sentence = ProcessedSentence { sentence_id: format!("{}_{}", base_sentence_id, index), ..Default::default() };
        let mut current_section = ParsingSection::None;
        for line in block_str.lines() {
            let line_trimmed = line.trim();
            if line_trimmed.is_empty() { continue; }
            if line_trimmed.starts_with("AdvS::") { current_section = ParsingSection::AdvS; sentence.adv_s = line_trimmed.trim_start_matches("AdvS::").trim().to_string(); continue; }
            if line_trimmed.starts_with("SimS::") { current_section = ParsingSection::SimS; sentence.sim_s = line_trimmed.trim_start_matches("SimS::").trim().to_string(); continue; }
            if line_trimmed.starts_with("SimE::") { current_section = ParsingSection::SimE; sentence.sim_e = line_trimmed.trim_start_matches("SimE::").trim().to_string(); continue; }
            if line_trimmed.starts_with("AdvSL::") { let cwc = line_trimmed.trim_start_matches("AdvSL::").trim(); let c = if let Some(cs) = cwc.find(" //") { cwc[..cs].trim_end() } else { cwc }; sentence.adv_s_lemmas = c.split_whitespace().map(String::from).collect(); continue; }
            if line_trimmed.starts_with("LOCKED_PHRASE::") { let cwc = line_trimmed.trim_start_matches("LOCKED_PHRASE::").trim(); let c = if let Some(cs) = cwc.find(" //") { cwc[..cs].trim_end() } else { cwc }; sentence.locked_phrases = Some(c.split_whitespace().map(String::from).collect()); continue; }
            if line_trimmed.starts_with("SimS_Segments::") { current_section = ParsingSection::SimSSegments; continue; }
            if line_trimmed.starts_with("PHRASE_ALIGN::") { current_section = ParsingSection::PhraseAlign; continue; }
            if line_trimmed.starts_with("SimSL::") { current_section = ParsingSection::SimSL; continue; }
            if line_trimmed.starts_with("DIGLOT_MAP::") { current_section = ParsingSection::DiglotMap; continue; }
            match current_section {
                ParsingSection::SimSSegments => { let re = Regex::new(r"^(S\d+)\((.*)\)$").unwrap(); if let Some(caps) = re.captures(line_trimmed) { sentence.sim_s_segments.push(SegmentData { id: caps.get(1).unwrap().as_str().to_string(), text: caps.get(2).unwrap().as_str().to_string() }); } }
                ParsingSection::PhraseAlign => { let p: Vec<&str> = line_trimmed.split('~').map(|x| x.trim()).collect(); if p.len() == 3 { sentence.phrase_alignments.push(PhraseAlignment { segment_id: p[0].to_string(), adv_s_span: p[1].to_string(), sim_e_span: p[2].to_string() }); } }
                ParsingSection::SimSL => { let lp: Vec<&str> = line_trimmed.splitn(2, "::").map(|x| x.trim()).collect(); if lp.len() == 2 { let sid = lp[0].to_string(); let lwc = lp[1]; let ls = if let Some(cs) = lwc.find(" //") { lwc[..cs].trim_end() } else { lwc }; sentence.sim_s_lemmas.push(SegmentLemmas { segment_id: sid, lemmas: ls.split_whitespace().map(String::from).collect() }); } }
                ParsingSection::DiglotMap => { let lp: Vec<&str> = line_trimmed.splitn(2, "::").map(|x| x.trim()).collect(); if lp.len() == 2 { let sid = lp[0]; let esc = lp[1]; let es = if let Some(cs) = esc.find(" //") { esc[..cs].trim_end() } else { esc }; let mut map = DiglotSegmentMap { segment_id: sid.to_string(), entries: Vec::new() }; let re = Regex::new(r"^(.*?)->(.*?)\s*\[(.*?)\]\s*\(([YN])\)$").unwrap(); for epwc in es.split('|').map(|e| e.trim()) { if epwc.is_empty() { continue; } let entry_s = epwc; if let Some(caps) = re.captures(entry_s) { let ew = caps.get(1).map_or("", |m| m.as_str().trim()).to_string(); let sl = caps.get(2).map_or("", |m| m.as_str().trim()).to_string(); let esf = caps.get(3).map_or("", |m| m.as_str().trim()).to_string(); let vc = caps.get(4).map_or("N", |m| m.as_str()); if ew.is_empty() { eprintln!("Warning: Parsed empty EngWord in diglot entry: '{}' (original: '{}') for segment {}", entry_s, epwc, sid); continue; } map.entries.push(DiglotEntry { eng_word: ew, spa_lemma: sl, exact_spa_form: esf, viable: vc == "Y" }); } else { eprintln!("Warning: Could not parse diglot entry part: '{}' (original: '{}') for segment {}", entry_s, epwc, sid); } } if !map.entries.is_empty() || es.contains("->") { sentence.diglot_map.push(map); } else if !es.trim().is_empty() && sid.starts_with('S') { sentence.diglot_map.push(map); } } else if !line_trimmed.is_empty() && line_trimmed.starts_with('S') && line_trimmed.contains("::") { let sid = line_trimmed.split("::").next().unwrap_or("").trim(); if !sid.is_empty() { sentence.diglot_map.push(DiglotSegmentMap { segment_id: sid.to_string(), entries: Vec::new() }); } } }
                _ => {}
            }
        }
        chapter.sentences.push(sentence);
    }
    Ok(chapter)
}
