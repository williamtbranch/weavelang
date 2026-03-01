// src/gui/preview.rs

use crate::domain::{sentence::Sentence, tier::Tier, token_stream::Token};
use crate::simulation::{frequency_manager, numerical_types::VLevelRecipe};
use std::collections::HashMap;

pub fn generate_preview_text(sentence: &Sentence, recipe: &VLevelRecipe) -> String {
    if frequency_manager::get_max_rank() == 0 {
        return "Error: Frequency List not loaded.".to_string();
    }

    // Helper: Check if a tier is strictly known
    let is_tier_known = |tier: &Tier, limit: u32| -> bool {
        if limit == u32::MAX {
            return true;
        }
        if limit == 0 {
            return false;
        }

        // 1. Bulk Check (Fastest)
        if !tier.lemmas.is_empty() {
            for lemma in &tier.lemmas {
                if let Some(rank) = frequency_manager::get_rank_for_lemma(lemma) {
                    if rank > limit {
                        return false;
                    }
                } else {
                    return false;
                }
            }
            return true;
        }

        // 2. Fallback: Check Tokens across ALL segments
        let mut total_word_count = 0;
        for segment in &tier.segments {
            for token in segment.stream.tokens() {
                if let Token::Word(w) = token {
                    total_word_count += 1;
                    let mut word_known = false;
                    if !w.lemmas.is_empty()
                        && w.lemmas.iter().all(|l| {
                            frequency_manager::get_rank_for_lemma(l).is_some_and(|r| r <= limit)
                        })
                    {
                        word_known = true;
                    }
                    if !word_known {
                        let norm = w.text.to_lowercase();
                        if frequency_manager::get_rank_for_lemma(&norm).is_some_and(|r| r <= limit)
                        {
                            word_known = true;
                        }
                    }
                    if !word_known {
                        return false;
                    }
                }
            }
        }
        if total_word_count == 0 {
            return true;
        }
        true
    };

    // ... (Logic for Adv, Mod, Bas is unchanged, just calling the helper) ...
    // 1. Advanced Target
    if let Some(adv_tier) = sentence.get_tier("advanced_target") {
        if is_tier_known(adv_tier, recipe.adv) {
            return adv_tier.full_text();
        }
    }

    // 2. Moderate Target
    if let Some(mod_tier) = sentence.get_tier("moderate_target") {
        if is_tier_known(mod_tier, recipe.mod_v) {
            return mod_tier.full_text();
        }
    }

    // 3. Basic Target
    if let Some(bas_tier) = sentence.get_tier("basic_target") {
        if is_tier_known(bas_tier, recipe.bas) {
            return bas_tier.full_text();
        }
    }

    // 4. Basic Base (Forward Diglot)
    if let Some(base_tier) = sentence.get_tier("basic_base") {
        let mut buffer = String::new();
        let mut mapping_lookup = HashMap::new();

        for mapping in sentence.mappings() {
            if mapping.from_tier_id == "basic_base" && mapping.to_tier_id == "basic_target" {
                for entry in &mapping.entries {
                    mapping_lookup.insert(entry.source_word_id, entry);
                }
            }
        }

        // Iterate Segments -> Tokens
        for segment in &base_tier.segments {
            for token in segment.stream.tokens() {
                match token {
                    Token::Background(s) => buffer.push_str(s),
                    Token::Word(w) => {
                        let mut used_spanish = false;
                        if let Some(entry) = mapping_lookup.get(&w.id) {
                            if entry.is_viable {
                                let target_known = entry.target_lemmas.iter().all(|l| {
                                    frequency_manager::get_rank_for_lemma(l)
                                        .is_some_and(|r| r <= recipe.bas)
                                });
                                if target_known || entry.is_proper_noun {
                                    buffer.push_str(&entry.target_text);
                                    used_spanish = true;
                                }
                            }
                        }
                        if !used_spanish {
                            buffer.push_str(&w.text);
                        }
                    }
                }
            }
        }
        return buffer;
    }

    "Preview Unavailable".to_string()
}
