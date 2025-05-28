//*** START FILE: src/types/llm_data.rs ***//
use serde::{Deserialize, Serialize};

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
//*** END FILE: src/types/llm_data.rs ***//