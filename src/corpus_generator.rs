// src/corpus_generator.rs
use crate::config::Config;
use crate::profile::LemmaState;
use crate::profile_io::{load_profile_snapshot, save_profile_snapshot};
// --- START FIX: Import PriceAndCost directly ---
use crate::simulation::{
    core_algo::{self, ChosenLevelOutput, L0SegmentChoice, L1PartChoice, OutputLevel},
    dictionary::GlobalLemmaDictionary,
    numerical_types::PriceAndCost, // This line brings it into scope
    numerical_types::NumericalLearnerProfile,
    preprocessor, text_generator,
};
// --- END FIX ---
use crate::{parsing::json_parser, types::json_types::JsonContentBlock};
use std::collections::HashMap;
use std::error::Error;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum SegmentType {
    AdvancedSpanish,
    ModerateSpanish,
    SimpleSpanish,
    EnglishDiglot,
    English,
}

#[derive(Debug)]
struct BlockInfo {
    start_index: usize,
    end_index: usize,
}

fn log_analysis_to_file(
    log_file_path: &Path,
    book_instance_unique_id: &str,
    level_stats: &HashMap<OutputLevel, usize>,
    segment_stats: &HashMap<SegmentType, usize>,
    total_sentences: usize,
    profile_after_book: &NumericalLearnerProfile,
) -> Result<(), std::io::Error> {
    let mut file = OpenOptions::new().create(true).append(true).open(log_file_path)?;
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    writeln!(file, "--- Analysis for Book Instance: {} (at {}) ---", book_instance_unique_id, timestamp)?;
    let total_sentences_float = total_sentences as f32;
    if total_sentences_float > 0.0 {
        writeln!(file, "  Sentence Level Distribution:")?;
        let get_level_count = |level: OutputLevel| -> usize { *level_stats.get(&level).unwrap_or(&0) };
        let l0_count = get_level_count(OutputLevel::AdvancedWeave);
        let l1_count = get_level_count(OutputLevel::SimpleHybrid);
        writeln!(file, "    L0 Advanced Weave: {:>5} sentences ({:>6.2}%)", l0_count, (l0_count as f32 / total_sentences_float) * 100.0)?;
        writeln!(file, "    L1 Simple Hybrid:  {:>5} sentences ({:>6.2}%)", l1_count, (l1_count as f32 / total_sentences_float) * 100.0)?;
    } else { writeln!(file, "  No sentences processed for this instance.")?; }
    let total_segments = segment_stats.values().sum::<usize>();
    if total_segments > 0 {
        writeln!(file, "  Segment Type Distribution (Total: {} segments):", total_segments)?;
        let get_segment_count = |seg_type: SegmentType| -> usize { *segment_stats.get(&seg_type).unwrap_or(&0) };
        let as_count = get_segment_count(SegmentType::AdvancedSpanish);
        let ms_count = get_segment_count(SegmentType::ModerateSpanish);
        let ss_count = get_segment_count(SegmentType::SimpleSpanish);
        let ed_count = get_segment_count(SegmentType::EnglishDiglot);
        let en_count = get_segment_count(SegmentType::English);
        let total_segments_float = total_segments as f32;
        writeln!(file, "    AS (Advanced Spanish): {:>5} segments ({:>6.2}%)", as_count, (as_count as f32 / total_segments_float) * 100.0)?;
        writeln!(file, "    MS (Moderate Spanish): {:>5} segments ({:>6.2}%)", ms_count, (ms_count as f32 / total_segments_float) * 100.0)?;
        writeln!(file, "    SS (Simple Spanish):   {:>5} segments ({:>6.2}%)", ss_count, (ss_count as f32 / total_segments_float) * 100.0)?;
        writeln!(file, "    ED (English Diglot):   {:>5} segments ({:>6.2}%)", ed_count, (ed_count as f32 / total_segments_float) * 100.0)?;
        writeln!(file, "    EN (English):          {:>5} segments ({:>6.2}%)", en_count, (en_count as f32 / total_segments_float) * 100.0)?;
    }
    writeln!(file, "  Profile State at End:")?;
    writeln!(file, "    Known Lemmas: {}", profile_after_book.count_known())?;
    writeln!(file, "    Active Lemmas: {}", profile_after_book.count_active_only())?;
    writeln!(file, "    Total K/A:     {}", profile_after_book.count_total_known_or_active())?;
    writeln!(file, "---------------------------------------------------------\n")?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct GenerationArgs {
    pub sequence_path: PathBuf,
    pub input_json_dir: PathBuf,
    pub tts_output_dir: PathBuf,
    pub profiles_dir: PathBuf,
    pub start_profile_path: Option<PathBuf>,
    pub sentences_per_block: usize,
    pub max_words_to_add_per_block: u32,
    pub target_ct_threshold: f32,
    pub words_per_level: u32,
}

pub fn run_corpus_generation(
    _project_config: &Config,
    args: &GenerationArgs,
) -> Result<(), Box<dyn Error>> {
    println!("[DEBUG] Entered run_corpus_generation.");

    let (mut learner_profile, mut global_lemma_dictionary) =
        if let Some(start_profile_path) = &args.start_profile_path {
            load_profile_snapshot(start_profile_path).unwrap_or_else(|e| {
                eprintln!("[DEBUG] Error loading profile: {}. Starting new.", e);
                (NumericalLearnerProfile::new(), GlobalLemmaDictionary::new())
            })
        } else {
            (NumericalLearnerProfile::new(), GlobalLemmaDictionary::new())
        };

    fs::create_dir_all(&args.tts_output_dir)?;
    fs::create_dir_all(&args.profiles_dir)?;

    let corpus_sequence: Vec<String> = BufReader::new(File::open(&args.sequence_path)?)
        .lines()
        .filter_map(Result::ok)
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    
    if corpus_sequence.is_empty() { return Ok(()); }
    println!("[DEBUG] Found {} books in sequence: {:?}", corpus_sequence.len(), corpus_sequence);

    let analysis_log_path = args.profiles_dir.join("corpus_analysis_log.txt");
    let mut book_instance_counter: HashMap<String, usize> = HashMap::new();

    for book_stem in &corpus_sequence {
        let count = book_instance_counter.entry(book_stem.clone()).or_insert(0);
        *count += 1;
        let book_instance_unique_id = format!("{}_inst{:02}", book_stem, *count);
        println!("\n[DEBUG] --- Processing book instance: {} ---", book_instance_unique_id);

        let base_content_path = PathBuf::from(&_project_config.content_project_dir);
        let json_file_path = base_content_path
            .join(&args.input_json_dir)
            .join(format!("{}.stage8.json", book_stem));
        
        println!("[DEBUG] Attempting to read JSON file: {}", json_file_path.display());
        
        let json_content_str = match fs::read_to_string(&json_file_path) {
            Ok(content) => content,
            Err(e) => { eprintln!("[DEBUG] FAILED to read JSON file: {}. Skipping book.", e); continue; }
        };
        println!("[DEBUG] Successfully read JSON file.");

        let json_chapter = json_parser::parse_chapter_from_json(&json_content_str)?;
        global_lemma_dictionary.populate_from_json_chapter(&json_chapter);
        let (mut numerical_chapter, book_frequency_map) = preprocessor::json_chapter_to_numerical(&json_chapter, &mut global_lemma_dictionary);
        
        let num_sentences_in_book = numerical_chapter.sentences_numerical.len();
        if num_sentences_in_book == 0 { eprintln!("[DEBUG] No sentences found in {}. Skipping.", book_stem); continue; }
        println!("[DEBUG] Parsed {} sentences.", num_sentences_in_book);

        let mut blocks: Vec<BlockInfo> = Vec::new();
        if num_sentences_in_book > 0 {
            let mut start_idx = 0;
            while start_idx < num_sentences_in_book {
                let end_idx = (start_idx + args.sentences_per_block).min(num_sentences_in_book);
                blocks.push(BlockInfo { start_index: start_idx, end_index: end_idx });
                start_idx = end_idx;
            }
            if blocks.len() > 1 && (blocks.last().unwrap().end_index - blocks.last().unwrap().start_index) < args.sentences_per_block / 2 {
                if let Some(last_block) = blocks.pop() {
                    if let Some(second_last_block) = blocks.last_mut() {
                        second_last_block.end_index = last_block.end_index;
                    }
                }
            }
        }
        
        if blocks.is_empty() { println!("[DEBUG] Calculated 0 blocks to process. Skipping book."); continue; }
        println!("[DEBUG] Calculated {} blocks to process.", blocks.len());
        
        let mut book_level_stats: HashMap<OutputLevel, usize> = HashMap::new();
        let mut book_segment_stats: HashMap<SegmentType, usize> = HashMap::new();
        let mut final_book_text_parts: Vec<String> = Vec::new();

        for (block_idx, block_info) in blocks.iter().enumerate() {
            println!("[DEBUG]   Processing block {} (sentences {}-{})", block_idx + 1, block_info.start_index, block_info.end_index - 1);
            
            let mut side_tally_profile = learner_profile.clone();
            let mut words_added_this_block = 0;

            loop {
                let mut block_outputs: Vec<ChosenLevelOutput> = Vec::new();
                // --- FIX: Use the imported type directly ---
                //let mut all_possible_upgrades: Vec<PriceAndCost> = Vec::new();
                let mut all_possible_upgrades: Vec<crate::simulation::numerical_types::PriceAndCost> = Vec::new();
                let mut expressed_lemmas_this_pass: Vec<u32> = Vec::new();
                let mut expressed_english_words_this_pass = 0;

                let block_sentences_slice = &mut numerical_chapter.sentences_numerical[block_info.start_index..block_info.end_index];
                
                for sentence in block_sentences_slice.iter_mut() {
                    let output = core_algo::determine_and_annotate_sentence_expression(sentence, &side_tally_profile, &global_lemma_dictionary);
                    expressed_lemmas_this_pass.extend(&output.lemma_ids);
                    expressed_english_words_this_pass += output.english_word_count;
                    block_outputs.push(output);

                    if sentence.l0_upgrade_pc.price > 0 {
                        all_possible_upgrades.push(sentence.l0_upgrade_pc.clone());
                    }
                    for pc in sentence.l1_segment_upgrade_pcs.values() {
                        if pc.price > 0 {
                            all_possible_upgrades.push(pc.clone());
                        }
                    }
                }

                let mut known_lemmas = 0;
                let mut active_lemmas = 0;
                for &id in &expressed_lemmas_this_pass {
                    if let Some(info) = side_tally_profile.get_lemma_info(id) {
                        if info.state == LemmaState::Known { known_lemmas += 1; }
                        else if info.state == LemmaState::Active { active_lemmas += 1; }
                    }
                }
                
                let numerator = expressed_english_words_this_pass + known_lemmas;
                let denominator = expressed_english_words_this_pass + known_lemmas + active_lemmas;
                let ct_score = if denominator > 0 { numerator as f32 / denominator as f32 } else { 1.0 };
                
                println!("[DEBUG]     CT Pass: {:.2}%. (Num: {}, Denom: {}). Words Added: {}/{}", ct_score * 100.0, numerator, denominator, words_added_this_block, args.max_words_to_add_per_block);

                let finalize_block_and_break = |
                    final_book_text_parts: &mut Vec<String>,
                    book_level_stats: &mut HashMap<OutputLevel, usize>,
                    book_segment_stats: &mut HashMap<SegmentType, usize>,
                    learner_profile: &mut NumericalLearnerProfile
                | -> Result<(), Box<dyn Error>> {
                    let all_string_sentence_blocks: Vec<_> = json_chapter.content_blocks.iter().filter_map(|cb| match cb {
                        JsonContentBlock::Sentence(s) => Some(s),
                        _ => None,
                    }).collect();
                    let string_sentences_slice = &all_string_sentence_blocks[block_info.start_index..block_info.end_index];
                    let final_block_text = text_generator::generate_final_text_for_block_from_levels(string_sentences_slice, &block_outputs)?;
                    final_book_text_parts.push(final_block_text);

                    for output in &block_outputs {
                        *book_level_stats.entry(output.level).or_insert(0) += 1;
                        match output.level {
                            OutputLevel::AdvancedWeave => if let Some(choices) = &output.l0_segment_choices {
                                for choice in choices {
                                    let seg_type = match choice {
                                        L0SegmentChoice::Adv(_) => SegmentType::AdvancedSpanish,
                                        L0SegmentChoice::SimplerAdv(_) => SegmentType::ModerateSpanish,
                                    };
                                    *book_segment_stats.entry(seg_type).or_insert(0) += 1;
                                }
                            },
                            OutputLevel::SimpleHybrid => if let Some(choices) = &output.l1_part_choices {
                                for choice in choices {
                                    let seg_type = match choice {
                                        L1PartChoice::Spanish(_) => SegmentType::SimpleSpanish,
                                        L1PartChoice::Hybrid {..} => SegmentType::EnglishDiglot,
                                        L1PartChoice::English(_) => SegmentType::English,
                                    };
                                    *book_segment_stats.entry(seg_type).or_insert(0) += 1;
                                }
                            },
                        }
                    }
                    learner_profile.record_exposures(&expressed_lemmas_this_pass, &global_lemma_dictionary);
                    Ok(())
                };

                if ct_score <= args.target_ct_threshold || words_added_this_block >= args.max_words_to_add_per_block {
                    println!("[DEBUG]     CT threshold met or max words added. Finalizing block.");
                    finalize_block_and_break(&mut final_book_text_parts, &mut book_level_stats, &mut book_segment_stats, &mut learner_profile)?;
                    break;
                }

                if all_possible_upgrades.is_empty() {
                    println!("[DEBUG]     No more upgrades of any price available. Finalizing block.");
                    finalize_block_and_break(&mut final_book_text_parts, &mut book_level_stats, &mut book_segment_stats, &mut learner_profile)?;
                    break;
                }

                let min_price = all_possible_upgrades.iter().map(|pc| pc.price).min().unwrap_or(0);
                
                if min_price == 0 {
                     println!("[DEBUG]     No upgrades with price > 0 found. Finalizing block.");
                     finalize_block_and_break(&mut final_book_text_parts, &mut book_level_stats, &mut book_segment_stats, &mut learner_profile)?;
                     break;
                }

                if (words_added_this_block + min_price) > args.max_words_to_add_per_block {
                    println!("[DEBUG]     Next cheapest upgrade (Price {}) would exceed max words per block. Finalizing.", min_price);
                    finalize_block_and_break(&mut final_book_text_parts, &mut book_level_stats, &mut book_segment_stats, &mut learner_profile)?;
                    break;
                }

                let best_candidate_to_activate = all_possible_upgrades
                    .iter()
                    .filter(|pc| pc.price == min_price)
                    .max_by_key(|pc| pc.cost.iter().map(|id| book_frequency_map.get(id).cloned().unwrap_or(0)).sum::<u32>());

                if let Some(candidate) = best_candidate_to_activate {
                    let cost_lemmas = &candidate.cost;
                    println!("[DEBUG]     -> Activating Price-{} upgrade. Lemma IDs: {:?}", min_price, cost_lemmas);
                    for &lemma_id in cost_lemmas {
                        side_tally_profile.set_lemma_state(lemma_id, LemmaState::Active);
                    }
                    words_added_this_block += cost_lemmas.len() as u32;
                } else {
                    println!("[DEBUG]     Could not select a best candidate. Finalizing block.");
                    finalize_block_and_break(&mut final_book_text_parts, &mut book_level_stats, &mut book_segment_stats, &mut learner_profile)?;
                    break;
                }
            }
        }

        log_analysis_to_file(&analysis_log_path, &book_instance_unique_id, &book_level_stats, &book_segment_stats, num_sentences_in_book, &learner_profile)?;
        let tts_output_file_path = args.tts_output_dir.join(format!("{}.txt", book_instance_unique_id));
        fs::write(&tts_output_file_path, final_book_text_parts.join("\n\n"))?;
        println!("[DEBUG]   Saved TTS input to: {}", tts_output_file_path.display());
        let out_profile_path = args.profiles_dir.join(format!("{}_out.profile.json", book_instance_unique_id));
        save_profile_snapshot(&learner_profile, &global_lemma_dictionary, &out_profile_path)?;
        println!("[DEBUG]   Saved out-profile to: {}", out_profile_path.display());
    }

    println!("\n[DEBUG] Corpus generation run finished.");
    Ok(())
}