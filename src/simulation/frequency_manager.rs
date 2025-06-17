// In src/simulation/frequency_manager.rs

use std::collections::HashMap;
use once_cell::sync::Lazy; // Add `once_cell` to Cargo.toml

// Lazy static map of lemma -> corpus frequency (in millions for simplicity)
static HIGH_FREQ_WORDS: Lazy<HashMap<&'static str, f32>> = Lazy::new(|| {
    let mut m = HashMap::new();
    m.insert("de", 9.999518);
    m.insert("la", 6.277560);
    m.insert("que", 4.681839);
    m.insert("el", 4.569652);
    m.insert("en", 4.234281);
    m.insert("y", 4.180279);
    m.insert("a", 3.260939);
    m.insert("los", 2.618657);
    m.insert("se", 2.022514);
    m.insert("del", 1.857225); // from the
    m.insert("las", 1.686741);
    m.insert("un", 1.659827);
    m.insert("por", 1.561904);
    m.insert("con", 1.481607);
    m.insert("no", 1.465503);
    m.insert("una", 1.347603);
    m.insert("su", 1.103617);
    // Note: "es" is a verb form. The lemma is "ser".
    // We should probably map the lemma.
    m.insert("ser", 1.019669); // Mapping the lemma for "es"
    m
});

const DEFAULT_EXPOSURE_THRESHOLD: u32 = 20;

/// Calculates the required exposure threshold for a given lemma string.
/// It applies a special formula for high-frequency words and returns a default otherwise.
pub fn get_exposure_threshold_for_lemma(lemma: &str) -> u32 {
    if let Some(frequency_in_millions) = HIGH_FREQ_WORDS.get(lemma) {
        // Your formula: (Freq / 1M) * (2/3) * 20
        // Since we stored Freq/1M directly, it's simpler:
        let new_threshold = frequency_in_millions * (2.0 / 3.0) * (DEFAULT_EXPOSURE_THRESHOLD as f32);
        
        // Clamp the value to a reasonable range, e.g., at least 1.
        // Round to nearest u32.
        (new_threshold.round() as u32).max(1)
    } else {
        DEFAULT_EXPOSURE_THRESHOLD
    }
}