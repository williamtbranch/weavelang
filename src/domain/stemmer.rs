//! Language-specific stemming for wlemma bucket keys.
//!
//! See `documentation/Wlemma_Migration_Plan.md`. The `Stemmer` trait keeps
//! the rest of the codebase free of any specific stemming library so we can
//! swap algorithms per language without touching consumers.

use rust_stemmers::{Algorithm, Stemmer as SnowballStemmer};

/// Trait for language-specific stemmers used to compute wlemma bucket keys.
///
/// Implementations must be deterministic and side-effect-free. The input is
/// expected to already be lower-cased and trimmed; implementations may apply
/// additional language-specific normalization (e.g. accent folding).
pub trait Stemmer: Send + Sync {
    /// Reduce a normalized token to its bucket key.
    fn stem(&self, word: &str) -> String;

    /// Strip a language-specific enclitic suffix from a surface form, if
    /// any. Returns `Some(base)` when a clitic was removed and the
    /// remainder looks plausibly verb-shaped; `None` otherwise.
    ///
    /// This is a salvage candidate for the wlemma `min` rule: in
    /// languages with enclitic pronouns (Spanish `acércate`, `sentarte`,
    /// `gritándoles`) upstream lemmatizers often regurgitate the
    /// surface or invent a malformed infinitive. Stemming the
    /// clitic-stripped surface gives the wlemma machinery a third
    /// candidate that lands in the correct verb family. Default
    /// implementation returns `None` — languages without enclitics need
    /// no override.
    fn strip_enclitics(&self, _word: &str) -> Option<String> {
        None
    }

    /// Generate radical-change ("stem-changing verb") un-mutated
    /// variants of `word`. For Spanish, this undoes the stressed-stem
    /// diphthongization (`ie → e`, `ue → o`) so that a stressed
    /// imperative-with-clitic stem like `siénta` (from `siéntate`) can
    /// reach the same Snowball bucket as the infinitive `sentar`.
    ///
    /// Callers MUST pre-gate this with `strip_enclitics`: the function
    /// itself does no validation, and applied to raw text it would
    /// happily mangle `puerta`, `tiempo`, `bueno`, etc. Only the
    /// post-strip remainder of an enclitic-attached form is a safe
    /// input.
    ///
    /// Default implementation returns an empty vector — languages
    /// without radical-change conjugation patterns need no override.
    fn unmutate_radical_change(&self, _word: &str) -> Vec<String> {
        Vec::new()
    }
}

/// Snowball-based Spanish stemmer.
///
/// Wraps `rust_stemmers::Stemmer(Algorithm::Spanish)` with an ASCII
/// diacritic-fold pre-pass: `á é í ó ú ñ ü` are folded to plain ASCII
/// before stemming. This is required because the master Spanish
/// frequency list (`assets/frequency_lists/es_master_frequency_list.txt`)
/// was built ASCII-folded — `niño` is stored as `nino`, `gritándoles` as
/// `gritandoles`, etc. Snowball-Spanish strips á/é/í/ó/ú on its own but
/// preserves `ñ`, so without this fold every `ñ`-bearing surface would
/// stem to a bucket key that doesn't exist in the loaded list.
pub struct SpanishSnowball {
    inner: SnowballStemmer,
}

impl SpanishSnowball {
    pub fn new() -> Self {
        Self {
            inner: SnowballStemmer::create(Algorithm::Spanish),
        }
    }

    /// ASCII-fold the Spanish diacritics that appear in the master
    /// frequency list. Cheap char-by-char remap; avoids pulling in a
    /// full Unicode-normalization dependency. Inputs are expected to
    /// already be lower-cased upstream, but uppercase variants are
    /// handled defensively so calling this on a raw surface form is
    /// also safe.
    fn fold_diacritics(word: &str) -> String {
        let mut out = String::with_capacity(word.len());
        for c in word.chars() {
            let folded = match c {
                'á' | 'à' | 'ä' | 'â' => 'a',
                'é' | 'è' | 'ë' | 'ê' => 'e',
                'í' | 'ì' | 'ï' | 'î' => 'i',
                'ó' | 'ò' | 'ö' | 'ô' => 'o',
                'ú' | 'ù' | 'ü' | 'û' => 'u',
                'ñ' => 'n',
                'Á' | 'À' | 'Ä' | 'Â' => 'A',
                'É' | 'È' | 'Ë' | 'Ê' => 'E',
                'Í' | 'Ì' | 'Ï' | 'Î' => 'I',
                'Ó' | 'Ò' | 'Ö' | 'Ô' => 'O',
                'Ú' | 'Ù' | 'Ü' | 'Û' => 'U',
                'Ñ' => 'N',
                other => other,
            };
            out.push(folded);
        }
        out
    }
}

impl Default for SpanishSnowball {
    fn default() -> Self {
        Self::new()
    }
}

impl Stemmer for SpanishSnowball {
    fn stem(&self, word: &str) -> String {
        let folded = Self::fold_diacritics(word);
        self.inner.stem(&folded).into_owned()
    }

    /// Strip a Spanish enclitic-pronoun suffix from `word` if the result
    /// is plausibly a verb form. Returns `None` otherwise.
    ///
    /// Closed list of clitics: `me te se lo la le nos os los las les`
    /// plus the dative+accusative combos (`melo mela telo tela selo
    /// sela …`) and `nos`-prefixed compounds. Tries longest matches
    /// first.
    ///
    /// Gate: to avoid stripping incidental clitic-shaped suffixes from
    /// non-verbs (e.g. `carteles` → `carte`, `papeles` → `pape`), only
    /// strip when the original word either contains an accented vowel
    /// (`á é í ó ú` — typical of imperatives and gerunds with enclitics
    /// like `acércate`, `siéntate`, `gritándoles`) or the stripped
    /// remainder ends in an infinitive suffix `-ar`, `-er`, `-ir`
    /// (`sentarte` → `sentar`, `decirme` → `decir`). The remainder
    /// must also be at least 3 characters.
    fn strip_enclitics(&self, word: &str) -> Option<String> {
        // Closed list, longest first so greedy match wins.
        const CLITICS: &[&str] = &[
            "noslos", "noslas",
            "noslo", "nosla", "oslos", "oslas",
            "melos", "melas", "telos", "telas", "selos", "selas",
            "lelos", "lelas",
            "melo", "mela", "telo", "tela", "selo", "sela",
            "lelo", "lela", "oslo", "osla",
            "nos", "los", "las", "les",
            "me", "te", "se", "lo", "la", "le", "os",
        ];

        let lower = word.to_lowercase();
        let has_accent = lower.chars().any(|c| matches!(c, 'á' | 'é' | 'í' | 'ó' | 'ú'));

        for clitic in CLITICS {
            if let Some(stripped) = lower.strip_suffix(clitic) {
                // Longest match wins. If this match fails the gates,
                // we do NOT fall through to a shorter clitic — falling
                // through would leave a residual clitic on the front
                // of the remainder and pick up the wrong bucket.
                if stripped.chars().count() < 3 {
                    return None;
                }
                let infinitive_tail = stripped.ends_with("ar")
                    || stripped.ends_with("er")
                    || stripped.ends_with("ir");
                if !has_accent && !infinitive_tail {
                    return None;
                }
                return Some(stripped.to_string());
            }
        }
        None
    }

    /// Un-mutate Spanish stressed-stem diphthongization to recover the
    /// infinitive-style stem vowel. Generates up to two candidates by
    /// replacing the FIRST occurrence of `ie` with `e` and the FIRST
    /// occurrence of `ue` with `o` (independently — both candidates are
    /// emitted if both diphthongs appear). The diacritic fold is
    /// applied internally so callers can pass an accented strip
    /// remainder like `siénta` directly.
    ///
    /// Skips word-initial diphthongs (`idx == 0`) to avoid mangling
    /// `iendo`/`iente` style endings or `uebra` if the strip somehow
    /// landed at position 0; the radical-change pattern always operates
    /// on a stem internal vowel after at least one consonant.
    ///
    /// Examples (post strip-enclitics):
    ///   `siénta` (siéntate -te) → fold `sienta` → un-mutate `senta` → snowball `sent`
    ///   `cuénta` (cuéntame -me) → fold `cuenta` → un-mutate `conta` → snowball `cont`
    ///   `duérme` (duérmete -te) → fold `duerme` → un-mutate `dorme` → snowball `dorm`
    ///   `acérca` (acércate -te) → fold `acerca` → no diphthong → empty
    fn unmutate_radical_change(&self, word: &str) -> Vec<String> {
        let folded = Self::fold_diacritics(word);
        let mut out: Vec<String> = Vec::new();

        if let Some(idx) = folded.find("ie") {
            if idx >= 1 {
                let mut variant = String::with_capacity(folded.len() - 1);
                variant.push_str(&folded[..idx]);
                variant.push('e');
                variant.push_str(&folded[idx + 2..]);
                out.push(variant);
            }
        }
        if let Some(idx) = folded.find("ue") {
            if idx >= 1 {
                let mut variant = String::with_capacity(folded.len() - 1);
                variant.push_str(&folded[..idx]);
                variant.push('o');
                variant.push_str(&folded[idx + 2..]);
                out.push(variant);
            }
        }
        out
    }
}

/// Factory: pick a stemmer for a BCP-47-ish language code.
///
/// Returns `None` for languages without a registered stemmer; callers should
/// fall back to identity (treat surface == wlemma) in that case.
pub fn for_language(lang_code: &str) -> Option<Box<dyn Stemmer>> {
    let primary = lang_code
        .split(['-', '_'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match primary.as_str() {
        "es" => Some(Box::new(SpanishSnowball::new())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spanish_buckets_inflections_together() {
        let s = SpanishSnowball::new();
        // Plural / singular noun families must share a stem.
        assert_eq!(s.stem("niños"), s.stem("niño"));
        assert_eq!(s.stem("niños"), s.stem("niña"));
        assert_eq!(s.stem("camioneros"), s.stem("camionero"));
    }

    #[test]
    fn spanish_buckets_verb_inflections_with_lemma() {
        let s = SpanishSnowball::new();
        // Surface and infinitive should land in the same bucket.
        assert_eq!(s.stem("corres"), s.stem("correr"));
        assert_eq!(s.stem("gritándoles"), s.stem("gritar"));
    }

    #[test]
    fn spanish_closed_class_words_are_stable() {
        // Closed-class words have no inflectional family worth merging; we
        // just want their stem to be deterministic so the lookup is stable.
        let s = SpanishSnowball::new();
        assert_eq!(s.stem("los"), s.stem("los"));
        assert_eq!(s.stem("de"), s.stem("de"));
        assert_eq!(s.stem("que"), s.stem("que"));
        // And they must not collapse into each other.
        assert_ne!(s.stem("los"), s.stem("de"));
        assert_ne!(s.stem("de"), s.stem("que"));
    }

    #[test]
    fn factory_returns_spanish_stemmer() {
        let s = for_language("es").expect("es stemmer");
        assert_eq!(s.stem("niños"), s.stem("niño"));
        // Locale variants resolve to the primary tag.
        let s2 = for_language("es-MX").expect("es-MX stemmer");
        assert_eq!(s2.stem("niños"), s2.stem("niño"));
    }

    #[test]
    fn factory_returns_none_for_unknown_language() {
        assert!(for_language("xx").is_none());
        assert!(for_language("").is_none());
    }

    #[test]
    fn ascii_folded_and_diacritic_forms_share_a_bucket() {
        // The master frequency list is ASCII-folded (`niño` → `nino`,
        // `gritándoles` → `gritandoles`). `SpanishSnowball::stem` must
        // collapse both encodings to the same key so in-text surfaces
        // (which carry diacritics) hit the same bucket as the loaded
        // list entries.
        let s = SpanishSnowball::new();
        assert_eq!(s.stem("niño"), s.stem("nino"));
        assert_eq!(s.stem("niños"), s.stem("ninos"));
        assert_eq!(s.stem("gritándoles"), s.stem("gritandoles"));
        assert_eq!(s.stem("año"), s.stem("ano"));
        assert_eq!(s.stem("señor"), s.stem("senor"));
        // And these `ñ`-bearing words still bucket with their inflectional
        // family — that test was already passing pre-fold and must remain.
        assert_eq!(s.stem("niños"), s.stem("niño"));
    }

    #[test]
    fn strip_enclitics_imperative_with_accent() {
        let s = SpanishSnowball::new();
        // `acércate` (imperative + te), `siéntate` (imperative + te):
        // accent on the verb stem unblocks the gate.
        assert_eq!(s.strip_enclitics("acércate").as_deref(), Some("acérca"));
        assert_eq!(s.strip_enclitics("siéntate").as_deref(), Some("siénta"));
        assert_eq!(s.strip_enclitics("Acércate").as_deref(), Some("acérca"));
    }

    #[test]
    fn strip_enclitics_gerund_with_compound_clitic() {
        let s = SpanishSnowball::new();
        // `gritándoles` = gritando + les. Accent on `á` unblocks gate;
        // longest-first match takes `les`, leaving `gritándo`.
        assert_eq!(s.strip_enclitics("gritándoles").as_deref(), Some("gritándo"));
        // `dímelo` = di + me + lo (combo `melo`). Stripped remainder
        // `dí` is only 2 chars, so the gate rejects it (we don't try a
        // shorter clitic afterward in this implementation).
        assert_eq!(s.strip_enclitics("dímelo"), None);
    }

    #[test]
    fn strip_enclitics_infinitive_plus_clitic() {
        let s = SpanishSnowball::new();
        // No accent, but stripped form ends in `-ar`/`-er`/`-ir` →
        // infinitive-tail gate unblocks.
        assert_eq!(s.strip_enclitics("sentarte").as_deref(), Some("sentar"));
        assert_eq!(s.strip_enclitics("lastimarte").as_deref(), Some("lastimar"));
        assert_eq!(s.strip_enclitics("decirme").as_deref(), Some("decir"));
        assert_eq!(s.strip_enclitics("esperarse").as_deref(), Some("esperar"));
        assert_eq!(s.strip_enclitics("despedirse").as_deref(), Some("despedir"));
    }

    #[test]
    fn strip_enclitics_skips_non_verb_lookalikes() {
        let s = SpanishSnowball::new();
        // Plural nouns ending in clitic-shaped suffixes must NOT be
        // stripped: no accent, no infinitive tail.
        assert_eq!(s.strip_enclitics("carteles"), None);
        assert_eq!(s.strip_enclitics("papeles"), None);
        assert_eq!(s.strip_enclitics("hoteles"), None);
        assert_eq!(s.strip_enclitics("camiones"), None);
        // Closed-class words with clitic-shaped tails: same.
        assert_eq!(s.strip_enclitics("los"), None);
        assert_eq!(s.strip_enclitics("nos"), None);
    }

    #[test]
    fn unmutate_radical_change_ie_to_e() {
        let s = SpanishSnowball::new();
        // Post-strip stressed-stem forms: e→ie pattern.
        assert!(s.unmutate_radical_change("siénta").contains(&"senta".to_string()));
        assert!(s.unmutate_radical_change("piénsa").contains(&"pensa".to_string()));
        assert!(s.unmutate_radical_change("ciérra").contains(&"cerra".to_string()));
        assert!(s.unmutate_radical_change("entiénde").contains(&"entende".to_string()));
        // Already-folded input also works.
        assert!(s.unmutate_radical_change("sienta").contains(&"senta".to_string()));
    }

    #[test]
    fn unmutate_radical_change_ue_to_o() {
        let s = SpanishSnowball::new();
        // o→ue pattern.
        assert!(s.unmutate_radical_change("cuénta").contains(&"conta".to_string()));
        assert!(s.unmutate_radical_change("duérme").contains(&"dorme".to_string()));
        assert!(s.unmutate_radical_change("vuélve").contains(&"volve".to_string()));
        assert!(s.unmutate_radical_change("recuérda").contains(&"recorda".to_string()));
    }

    #[test]
    fn unmutate_radical_change_no_diphthong() {
        let s = SpanishSnowball::new();
        // Strip remainders without `ie`/`ue` get nothing.
        assert!(s.unmutate_radical_change("acérca").is_empty());
        assert!(s.unmutate_radical_change("sentar").is_empty());
        assert!(s.unmutate_radical_change("decir").is_empty());
    }

    #[test]
    fn unmutate_radical_change_round_trip_stems_match_infinitive() {
        let s = SpanishSnowball::new();
        // The whole point: snowball-stem of the un-mutated form must
        // equal snowball-stem of the infinitive.
        let cases: &[(&str, &str)] = &[
            ("siénta", "sentar"),   // siéntate -> sienta -> senta == sent_
            ("cuénta", "contar"),   // cuéntame -> cuenta -> conta == cont_
            ("duérme", "dormir"),   // duérmete -> duerme -> dorme == dorm_
            ("piénsa", "pensar"),
            ("vuélve", "volver"),
        ];
        for (stripped, infinitive) in cases {
            let candidates = s.unmutate_radical_change(stripped);
            let infinitive_stem = s.stem(infinitive);
            let any_match = candidates.iter().any(|c| s.stem(c) == infinitive_stem);
            assert!(
                any_match,
                "expected un-mutated `{}` to stem to infinitive `{}` ({}), got candidates {:?} -> stems {:?}",
                stripped,
                infinitive,
                infinitive_stem,
                candidates,
                candidates.iter().map(|c| s.stem(c)).collect::<Vec<_>>()
            );
        }
    }
}
