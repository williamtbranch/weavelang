Project Documentation: WeaveLang - Spanish CI Learning Application - Version 6
Document Version: 6.0
Last Updated: (Date of this session)
Note for LLM (Context for Future Sessions):
This document is the primary specification for the WeaveLang project. It describes a major refactoring of the Rust simulation engine to use a simplified two-level generation hierarchy and a more holistic, token-based Comprehensibility Threshold (CT) calculation. This new model is more elegant, efficient, and accurately reflects the core learning principles.
1. Project Overview & Goal
Name: WeaveLang - Spanish CI Learning Application
Goal: To facilitate Spanish language acquisition for learners, using a Comprehensible Input (CI) methodology. This project focuses on creating robust, pre-processed learning content from literary works.
Methodology: A hybrid data pipeline is used for pre-processing. LLMs are leveraged for creative translation and simplification, while the SpaCy NLP library is used for deterministic, high-quality linguistic tasks. The final structured data is then processed by a newly refactored Rust application which simulates a learner's progress and generates scaffolded audio script files.
2. Core Learning Methodology: The Simplified Two-Level Model
The core of the Rust simulation engine has been refactored to a vastly simpler and more powerful two-level generation hierarchy. The previous L0-L6 levels contained logical redundancies; the new model eliminates them by recognizing that "full" Spanish levels are simply specific outcomes of a more general "weaving" process.
The New Generation Hierarchy
The Rust application attempts to generate text for each sentence by trying the following levels in order:
Level 0: The Advanced Weave
Description: This level attempts to construct the most advanced, natural Spanish sentence possible for the learner's current profile. It weaves together advanced_text and simpler_text segments from the JSON data.
Logic: For each sentence segment, it first attempts to use the advanced_text. If the learner does not know all the words in that segment, it falls back and attempts to use the corresponding simpler_text.
Failure Condition: If, for any single segment in the sentence, neither the advanced nor the simpler version is comprehensible, this entire level is considered a failure. The algorithm then immediately proceeds to Level 1.
Level 1: The Simple Hybrid (The Ultimate Fallback)
Description: This is the no-fail fallback level that guarantees a comprehensible output. It weaves together simple Spanish phrases, English phrases, and targeted Spanish word substitutions (diglotting).
Logic: For each L3-aligned sentence segment, it first attempts to use the simple Spanish version. If that is not comprehensible, it falls back to the corresponding English phrase. Within that English phrase, it then attempts to make a single, targeted substitution of an English word with a known Spanish word (a diglot). If no viable diglot substitution is possible, the plain English phrase is used.
Failure Condition: None. This level always produces a valid, expressed sentence, ranging from mostly Spanish to pure English.
How the New Levels Subsume the Old Hierarchy
This new two-level model is a direct simplification that fully encompasses the functionality of the old L0-L6 model:
Old L0 (Full AdvS) & L2 (Full SimplerAdvS) are now outcomes of New L0:
An "Old L0" sentence is generated when the "Advanced Weave" successfully uses the advanced_text for every segment.
An "Old L2" sentence is generated when the "Advanced Weave" falls back to and successfully uses the simpler_text for every segment.
Old L3, L4, L5, & L6 are now outcomes of New L1:
An "Old L3" (Full Simple Spanish) sentence is generated when the "Simple Hybrid" successfully uses the simple Spanish phrase for every segment.
An "Old L4" (Woven Simple/English) sentence is generated when the "Simple Hybrid" uses a mix of Spanish and English phrases.
An "Old L5" (Diglot) sentence is generated when the "Simple Hybrid" results in a full English sentence with one or more single-word Spanish substitutions.
An "Old L6" (Full English) sentence is generated when the "Simple Hybrid" fails to use any Spanish phrases or find any viable diglot substitutions.
3. The New Comprehensibility Threshold (CT) Model
To match the new holistic generation model, the CT calculation has been fundamentally improved. Instead of only measuring the Spanish portion, it now calculates a score based on every token (English word or Spanish lemma) in the final expressed sentence.
The Holistic CT Formula
The CT is calculated as the ratio of "effortless" tokens to the "total comprehensible" tokens.
Numerator (The "i" - Effortless Words):
Count of all Expressed English Words
+
Count of all Expressed Spanish Lemmas with state = 'Known'
Denominator (The "i+1" - Total Comprehensible Input):
Count of all Expressed English Words
+
Count of all Expressed Spanish Lemmas with state = 'Known'
+
Count of all Expressed Spanish Lemmas with state = 'Active'
This formula correctly models the learning process. The Active words are the +1 challenge; they are part of the input the learner is expected to understand (denominator) but are not yet effortless (excluded from the numerator), thus appropriately lowering the CT score and representing the cognitive load of learning.
Efficient English Word Counting
To avoid performance issues from repeatedly counting words in English strings during the regeneration loop, the word counts are now pre-calculated and stored.
Process: During the one-time preprocessor step (when converting JsonChapter to NumericalChapter), the Rust application counts the words in every English sentence and English phrase span.
Storage: These counts are stored as a usize field directly in the NumericalProcessedSentence and NumericalPhraseAlignmentToEng structs.
Result: The core simulation algorithm no longer performs string processing. It simply retrieves these pre-calculated integers, making the CT calculation extremely fast. When a diglot substitution occurs, it simply subtracts 1 from the stored count for that phrase.
4. The Data Pre-processing Pipeline (Python Orchestrator)
The Python data pipeline remains a critical and stable component. It is responsible for producing the rich, structured stage7.json files that are the input to the Rust simulation engine. Its hybrid LLM/SpaCy workflow is unchanged.
Key Principle: Use LLMs for subjective, creative tasks (translation, simplification, contextual mapping). Use SpaCy for objective, deterministic linguistic analysis (lemmatization, segmentation). The stage7.json files produced by this pipeline are the direct input for the newly refactored Rust simulation engine.
5. Next Steps for Development
Implement Rust Refactoring: Update the Rust codebase (core_algo.rs, text_generator.rs, etc.) to match the new two-level hierarchy and holistic CT calculation as detailed in this document.
Run Full Corpus Generation: Execute the run_corpus_gen.ps1 script to process the entire corpus using the new, more efficient and elegant algorithm.
Analyze & Tune: Examine the newly generated corpus_analysis_log.txt. The distribution between "Advanced Weave" and "Simple Hybrid" will provide clear, high-level feedback on the learning progression and inform parameter tuning (e.g., target_ct_threshold).
6. Create smart level system
It is proposed to make each level introduce the next set of lemmas by frequency such that the words in a text contain 2% more words than the previous level. 
Additionally as each book is being processed, it should gradually ramp the lemma limit up to the next level so that for example, a level 5 book by the time its last paragraph is generated, is effectively at level 6. In this way the students comprehension limits are stretched with every book. This of course does not imply that they are now ready for the next level each time they get to the end of a book. It is likely that for intermediate levels, the student may need to listen to books at that level a few times. Once he no longer feels challenged (especially by the end of the boo), he might consider moving on to a harder level.
We need a method to find where these 2% increment points are on average across various books and set in stone what the vocabulary set is for each level. All level n books should use the same vocabulary. The *stretch* that happens throughout the book could either be custom for that book using a few passes of the weavelang simulator and calculating the 2% stretch for that book in particular, or if it doesn't make much of a difference, the book always just ends at the next level.