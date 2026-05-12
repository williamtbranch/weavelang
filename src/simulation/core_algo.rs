// src/simulation/core_algo.rs
use super::frontier::FrontierEngine;
use super::numerical_types::{NumericalLearnerProfile, NumericalProcessedSentence, VLevelRecipe};
use crate::simulation::dictionary::GlobalLemmaDictionary;
use crate::simulation::frequency_manager;
use crate::types::json_types::JsonTokenType;
use std::collections::{HashMap, HashSet};

fn is_lemma_known_for_tier(
    lemma_id: u32,
    tier_v_level: u32,
    dictionary: &GlobalLemmaDictionary,
    pn_lemma_ids: &[u32],
    frontier_known_lemmas: Option<&HashSet<u32>>,
) -> bool {
    if tier_v_level == u32::MAX {
        return true;
    }
    if pn_lemma_ids.contains(&lemma_id) {
        return true;
    }
    if let Some(frontier_known) = frontier_known_lemmas {
        if frontier_known.contains(&lemma_id) {
            return true;
        }
    }

    dictionary
        .get_str(lemma_id)
        .and_then(|lemma_str| frequency_manager::rank_of_lemma_string(lemma_str))
        .unwrap_or(u32::MAX)
        <= tier_v_level
}

fn are_lemmas_known(
    lemma_ids: &[u32],
    tier_v_level: u32,
    dictionary: &GlobalLemmaDictionary,
    pn_lemma_ids: &[u32],
    frontier_known_lemmas: Option<&HashSet<u32>>,
) -> bool {
    if lemma_ids.is_empty() {
        return true;
    }
    if tier_v_level == u32::MAX {
        return true;
    }
    lemma_ids.iter().all(|&id| {
        is_lemma_known_for_tier(
            id,
            tier_v_level,
            dictionary,
            pn_lemma_ids,
            frontier_known_lemmas,
        )
    })
}

fn collect_unknown_lemma_token_weights_for_sentence(
    n_sentence: &NumericalProcessedSentence,
    dictionary: &GlobalLemmaDictionary,
    v_levels: &VLevelRecipe,
    pn_ids: &[u32],
) -> HashMap<u32, usize> {
    let mut weights: HashMap<u32, usize> = HashMap::new();

    for (_, lemmas, _, _, spa_wc) in &n_sentence.basic_inverse_diglot_map_numerical {
        let token_weight = (*spa_wc).max(1);
        for &lemma_id in lemmas {
            if !is_lemma_known_for_tier(lemma_id, v_levels.bas, dictionary, pn_ids, None) {
                *weights.entry(lemma_id).or_insert(0) += token_weight;
            }
        }
    }

    for seg_map in &n_sentence.basic_diglot_map_numerical {
        for entry in &seg_map.entries {
            let token_weight = entry.eng_word_count.max(1);
            for &lemma_id in &entry.spa_lemma_ids {
                if !is_lemma_known_for_tier(lemma_id, v_levels.bas, dictionary, pn_ids, None) {
                    *weights.entry(lemma_id).or_insert(0) += token_weight;
                }
            }
        }
    }

    if weights.is_empty() {
        for &lemma_id in &n_sentence.basic_target_lemma_ids {
            if !is_lemma_known_for_tier(lemma_id, v_levels.bas, dictionary, pn_ids, None) {
                *weights.entry(lemma_id).or_insert(0) += 1;
            }
        }
    }

    weights
}

fn build_sentence_frontier_set(
    n_sentence: &NumericalProcessedSentence,
    dictionary: &GlobalLemmaDictionary,
    v_levels: &VLevelRecipe,
    pn_ids: &[u32],
    frontier_engine: Option<&mut FrontierEngine>,
) -> HashSet<u32> {
    let Some(engine) = frontier_engine else {
        return HashSet::new();
    };

    let unknown_weights =
        collect_unknown_lemma_token_weights_for_sentence(n_sentence, dictionary, v_levels, pn_ids);

    engine.pick_sentence_frontier_lemmas(&unknown_weights)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum OutputLevel {
    #[default]
    AdvancedWeave,
    BasicTarget,
    InverseDiglot,
    BasicBaseDiglot,
}

#[derive(Debug, Clone)]
pub enum L0SegmentChoice {
    Adv(String),
    Mod(String),
}

#[derive(Debug, Clone)]
pub struct ChosenLevelOutput {
    pub level: OutputLevel,
    pub lemma_ids: Vec<u32>,
    pub l0_segment_choices: Option<Vec<L0SegmentChoice>>,
    pub l1_final_text: Option<String>,
    pub spanish_word_count: usize,
    pub english_word_count: usize,
}

fn try_build_advanced_weave(
    n_sentence: &NumericalProcessedSentence,
    dictionary: &GlobalLemmaDictionary,
    v_levels: &VLevelRecipe,
    pn_ids: &[u32],
    frontier_known_lemmas: Option<&HashSet<u32>>,
) -> Option<ChosenLevelOutput> {
    if n_sentence.adv_segment_bundles_numerical.is_empty() {
        return None;
    }
    let mut l0_candidate_choices: Vec<L0SegmentChoice> = Vec::new();
    let mut l0_collected_lemma_ids: Vec<u32> = Vec::new();
    let mut spanish_words = 0;
    for bundle in &n_sentence.adv_segment_bundles_numerical {
        if are_lemmas_known(
            &bundle.adv_lemma_ids,
            v_levels.adv,
            dictionary,
            pn_ids,
            frontier_known_lemmas,
        ) {
            l0_candidate_choices.push(L0SegmentChoice::Adv(bundle.adv_text_original.clone()));
            l0_collected_lemma_ids.extend(&bundle.adv_lemma_ids);
            spanish_words += bundle.adv_text_original.split_whitespace().count();
        } else if are_lemmas_known(
            &bundle.mod_lemma_ids,
            v_levels.mod_v,
            dictionary,
            pn_ids,
            frontier_known_lemmas,
        ) {
            l0_candidate_choices.push(L0SegmentChoice::Mod(bundle.mod_text_original.clone()));
            l0_collected_lemma_ids.extend(&bundle.mod_lemma_ids);
            spanish_words += bundle.mod_text_original.split_whitespace().count();
        } else {
            return None;
        }
    }
    Some(ChosenLevelOutput {
        level: OutputLevel::AdvancedWeave,
        lemma_ids: l0_collected_lemma_ids,
        l0_segment_choices: Some(l0_candidate_choices),
        l1_final_text: None,
        spanish_word_count: spanish_words,
        english_word_count: 0,
    })
}

pub fn determine_and_annotate_sentence_expression(
    n_sentence: &mut NumericalProcessedSentence,
    profile: &NumericalLearnerProfile,
    dictionary: &GlobalLemmaDictionary,
    v_levels: &VLevelRecipe,
    inverse_diglot_threshold: f32,
) -> ChosenLevelOutput {
    determine_and_annotate_sentence_expression_with_frontier(
        n_sentence,
        profile,
        dictionary,
        v_levels,
        inverse_diglot_threshold,
        None,
    )
}

pub fn determine_and_annotate_sentence_expression_with_frontier(
    n_sentence: &mut NumericalProcessedSentence,
    _profile: &NumericalLearnerProfile,
    dictionary: &GlobalLemmaDictionary,
    v_levels: &VLevelRecipe,
    _inverse_diglot_threshold: f32,
    frontier_engine: Option<&mut FrontierEngine>,
) -> ChosenLevelOutput {
    let pn_ids = &n_sentence.proper_noun_lemma_ids;
    let frontier_known_lemmas =
        build_sentence_frontier_set(n_sentence, dictionary, v_levels, pn_ids, frontier_engine);

    // --- 1. Try to build the L0 Weave ---
    if let Some(l0_output) = try_build_advanced_weave(
        n_sentence,
        dictionary,
        v_levels,
        pn_ids,
        Some(&frontier_known_lemmas),
    ) {
        return l0_output;
    }

    // --- 2. If L0 fails, try the full L1 BasicTarget tier ---
    if are_lemmas_known(
        &n_sentence.basic_target_lemma_ids,
        v_levels.bas,
        dictionary,
        pn_ids,
        Some(&frontier_known_lemmas),
    ) {
        let full_text = n_sentence
            .basic_target_tier_tokenized
            .iter()
            .map(|seg| seg.text.clone())
            .collect::<String>();

        return ChosenLevelOutput {
            level: OutputLevel::BasicTarget,
            lemma_ids: n_sentence.basic_target_lemma_ids.clone(),
            l0_segment_choices: None,
            l1_final_text: Some(full_text.clone()),
            spanish_word_count: full_text.split_whitespace().count(),
            english_word_count: 0,
        };
    }

    // --- 3. If BasicTarget fails, try the L1 InverseDiglot Weave ---
    let mut known_word_instances = 0;
    let mut can_render_inverse_diglot = true;
    let inv_diglot_map = &n_sentence.basic_inverse_diglot_map_numerical;

    if inv_diglot_map.is_empty() {
        can_render_inverse_diglot = false;
    }

    let total_word_instances: usize = inv_diglot_map.iter().map(|(_, _, _, _, spa_wc)| spa_wc).sum();

    if can_render_inverse_diglot {
        for (_, lemmas, sub, _, _spa_wc) in inv_diglot_map.iter() {
            if are_lemmas_known(
                lemmas,
                v_levels.bas,
                dictionary,
                pn_ids,
                Some(&frontier_known_lemmas),
            ) {
                let corresponding_entry = inv_diglot_map.iter().find(|(_, l, _, _, _)| l == lemmas);
                if let Some((_, _, _, _, spa_wc_val)) = corresponding_entry {
                    known_word_instances += spa_wc_val;
                }
            } else if *sub == "NO_SUB" {
                can_render_inverse_diglot = false;
                break;
            }
        }
    }

    if can_render_inverse_diglot {
        let known_ratio = if total_word_instances > 0 {
            known_word_instances as f32 / total_word_instances as f32
        } else {
            1.0
        };

        if known_ratio >= 0.5 {
            let mut final_parts = Vec::new();
            let mut collected_lemmas = Vec::new();
            let mut spanish_words = 0;
            let mut english_words = 0;
            let target_tokens = n_sentence
                .basic_target_tier_tokenized
                .first()
                .map_or(Vec::new(), |seg| seg.tokenized_text.clone());

            let mut map_idx = 0;
            for token in &target_tokens {
                if token.token_type == JsonTokenType::Background {
                    final_parts.push(token.value.clone());
                    continue;
                }
                if let Some((_, lemmas, sub, eng_wc, spa_wc)) = inv_diglot_map.get(map_idx) {
                    if are_lemmas_known(
                        lemmas,
                        v_levels.bas,
                        dictionary,
                        pn_ids,
                        Some(&frontier_known_lemmas),
                    ) {
                        final_parts.push(token.value.clone());
                        collected_lemmas.extend(lemmas.iter().cloned());
                        spanish_words += spa_wc;
                    } else {
                        final_parts.push(sub.to_string());
                        english_words += eng_wc;
                    }
                } else {
                    final_parts.push(token.value.clone());
                    spanish_words += 1;
                }
                map_idx += 1;
            }

            return ChosenLevelOutput {
                level: OutputLevel::InverseDiglot,
                lemma_ids: collected_lemmas,
                l0_segment_choices: None,
                l1_final_text: Some(final_parts.concat()),
                spanish_word_count: spanish_words,
                english_word_count: english_words,
            };
        }
    }

    // --- 4. If all else fails, fall back to the L1 BasicBaseDiglot ---
    let mut l1_collected_lemma_ids: Vec<u32> = Vec::new();
    let mut spanish_words = 0;
    let mut english_words = 0;
    let mut final_parts: Vec<String> = Vec::new();
    let diglot_lookup: HashMap<usize, _> = n_sentence
        .basic_diglot_map_numerical
        .iter()
        .flat_map(|seg_map| &seg_map.entries)
        .map(|entry| (entry.base_word_di, entry))
        .collect();
    let base_tokens = n_sentence
        .basic_base_tier_tokenized
        .first()
        .map_or(Vec::new(), |seg| seg.tokenized_text.clone());

    for token in &base_tokens {
        if token.token_type == JsonTokenType::Background {
            final_parts.push(token.value.clone());
            continue;
        }
        let di = token.diglot_index.unwrap_or(usize::MAX);
        if let Some(entry) = diglot_lookup.get(&di) {
            if entry.viable
                && entry.exact_spa_form_original != "NO_SUB"
                && (entry.is_base_token_pn
                    || are_lemmas_known(
                        &entry.spa_lemma_ids,
                        v_levels.bas,
                        dictionary,
                        pn_ids,
                        Some(&frontier_known_lemmas),
                    ))
            {
                final_parts.push(entry.exact_spa_form_original.clone());
                l1_collected_lemma_ids.extend(&entry.spa_lemma_ids);
                spanish_words += entry.eng_word_count;
            } else {
                final_parts.push(entry.eng_word_original.clone());
                english_words += entry.eng_word_count;
            }
        } else {
            final_parts.push(token.value.clone());
            english_words += token.value.split_whitespace().count();
        }
    }

    ChosenLevelOutput {
        level: OutputLevel::BasicBaseDiglot,
        lemma_ids: l1_collected_lemma_ids,
        l0_segment_choices: None,
        l1_final_text: Some(final_parts.concat()),
        spanish_word_count: spanish_words,
        english_word_count: english_words,
    }
}
