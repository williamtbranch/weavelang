use once_cell::sync::Lazy;
use regex::Regex;

// Ported from raw2stage.py
pub static BRACKETED_ARABIC_CHAPTER_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*\[\s*(\d+)\s*\]\s*$").unwrap());
pub static BRACKETED_ROMAN_CHAPTER_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)^\s*\[\s*([IVXLCDM]+)\s*\]\s*$").unwrap());
pub static EM_DASH_SECTION_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)^\s*—\s*([IVXLCDM]+)\s*—\s*$").unwrap());
pub static CHAPTER_MAIN_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)^\s*(CHAPTER)\s+([IVXLCDM\d]+(?:st|nd|rd|th)?|[A-ZÀ-ÖØ-ÞĀ-ĒĪ-ŌŪ-Ž'-]+)\s*[:.]?\s*(.*)$").unwrap());
pub static SHORT_LINE_NUMERAL_CHAPTER_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)^\s*([IVXLCDM\d]+)\s*[:.]?\s*$").unwrap());
pub static SPECIAL_SECTION_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)^\s*(PREFACE|INTRODUCTION|EPILOGUE|PROLOGUE|CONTENTS|APPENDIX|GLOSSARY|FORWARD|FOREWORD)(?:[:.\s]|$)").unwrap());

pub fn clean_italics_and_underscores(text: &str) -> String {
    let re1 = Regex::new(r"\b_([^_]+)_([.,!?;:])").unwrap();
    let re2 = Regex::new(r"\b_([^_]+)_\b").unwrap();
    
    // We must clone or use Cow because replace_all returns Cow
    let step1 = re1.replace_all(text, "$1$2");
    let step2 = re2.replace_all(&step1, "$1");
    step2.replace('_', " ")
}

/// Applies a series of cleaning and normalization steps to a raw Spanish lemma string,
/// handling all standard accented vowels, the ñ, and the ü with diaeresis.
///
/// Steps:
/// 1. Take only the first word (splitting by space).
/// 2. Lowercase.
/// 3. Replace accented characters with ASCII equivalents.
/// 4. Strip leading/trailing non-alphanumeric characters.
/// 5. Validate that only 'a-z' and '-' remain.
pub fn normalize_spanish_lemma(lemma_str: &str) -> String {
    // 1. Take the first word
    let first_word = lemma_str.split_whitespace().next().unwrap_or("");
    if first_word.is_empty() {
        return String::new();
    }

    // 2. Lowercase
    let mut s = first_word.to_lowercase();

    // 3. Replace accents manually (matches Python logic exactly)
    // Python: .replace('á', 'a')...
    s = s
        .replace('á', "a")
        .replace('é', "e")
        .replace('í', "i")
        .replace('ó', "o")
        .replace('ú', "u")
        .replace('ñ', "n")
        .replace('ü', "u");

    // 4. Strip leading/trailing non-word characters
    // Python: re.sub(r'^[^\w]+|[^\w]+$', '', s)
    // In Rust regex, \w includes alphanumeric + underscore. Python logic implies stripping punctuation.
    let re_strip = Regex::new(r"^[^\w]+|[^\w]+$").unwrap();
    s = re_strip.replace_all(&s, "").to_string();

    if s.is_empty() {
        return String::new();
    }

    // 5. Validation: ensure only a-z and - remain
    // Python: if re.search(r'[^a-z-]', s): return ""
    let re_validate = Regex::new(r"[^a-z-]").unwrap();
    if re_validate.is_match(&s) {
        return String::new();
    }

    s
}

/// Inserts spaces around em-dashes and parentheses so that SpaCy tokenizes
/// them as separate tokens.
///
/// Ported from `helper.py::preprocess_for_spacy`.
pub fn preprocess_for_spacy(text: &str) -> String {
    let re_before = Regex::new(r"(\w)([—()])").unwrap();
    let re_after = Regex::new(r"([—()])(\w)").unwrap();

    let step1 = re_before.replace_all(text, "$1 $2");
    let step2 = re_after.replace_all(&step1, "$1 $2");
    step2.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_basic() {
        assert_eq!(normalize_spanish_lemma("gato"), "gato");
        assert_eq!(normalize_spanish_lemma("Gato"), "gato");
    }

    #[test]
    fn test_normalize_accents() {
        assert_eq!(normalize_spanish_lemma("canción"), "cancion");
        assert_eq!(normalize_spanish_lemma("cigüesa"), "ciguesa");
        assert_eq!(normalize_spanish_lemma("niño"), "nino");
        assert_eq!(normalize_spanish_lemma("él"), "el");
    }

    #[test]
    fn test_normalize_strips_punctuation() {
        assert_eq!(normalize_spanish_lemma("(hola)"), "hola");
        assert_eq!(normalize_spanish_lemma("...adios..."), "adios");
    }

    #[test]
    fn test_normalize_first_word_only() {
        assert_eq!(normalize_spanish_lemma("ir (yo)"), "ir");
        assert_eq!(normalize_spanish_lemma("comer manzanas"), "comer");
    }

    #[test]
    fn test_normalize_rejects_invalid() {
        // "123" contains digits, which match [^a-z-], so it should be rejected.
        assert_eq!(normalize_spanish_lemma("123"), ""); 
        assert_eq!(normalize_spanish_lemma("h4xor"), "");
    }

    #[test]
    fn test_normalize_empty() {
        assert_eq!(normalize_spanish_lemma(""), "");
        assert_eq!(normalize_spanish_lemma("   "), "");
    }

    // --- preprocess_for_spacy tests ---

    #[test]
    fn test_preprocess_em_dash_unspaced() {
        // "word—word" → "word — word"
        assert_eq!(preprocess_for_spacy("one—not"), "one — not");
    }

    #[test]
    fn test_preprocess_parentheses() {
        // "(word)" → "( word )"  — space inserted between \w and ( and between ) and \w
        assert_eq!(preprocess_for_spacy("say(hello)now"), "say ( hello ) now");
    }

    #[test]
    fn test_preprocess_no_change_when_already_spaced() {
        assert_eq!(preprocess_for_spacy("one — not"), "one — not");
        assert_eq!(preprocess_for_spacy("hello world"), "hello world");
    }

    #[test]
    fn test_preprocess_multiple_em_dashes() {
        assert_eq!(
            preprocess_for_spacy("no one—not even his sister—thought"),
            "no one — not even his sister — thought"
        );
    }

    #[test]
    fn test_preprocess_empty_string() {
        assert_eq!(preprocess_for_spacy(""), "");
    }
}
