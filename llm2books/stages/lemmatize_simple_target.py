# llm2books/stages/lemmatize_simple_target.py

from typing import Any, Dict

from .base import SpaCyStage, logger
from .. import helper

class LemmatizeSimpleTarget(SpaCyStage):
    """
    Stage 6 (V2): Tokenizes and lemmatizes the simple target language text
    from the LLM, populating the final V2 token structure.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem, 
            cli_args=cli_args, 
            common_resources=common_resources, 
            stage_number=6, 
            stage_name="LemmatizeSimpleTarget"
        )

    def _process_data(self, data: Dict[str, Any]) -> Dict[str, Any]:
        """
        Processes the simple_target tier, tokenizing and lemmatizing the text
        for each segment.
        """
        lang_config = self.resources["language_config"]
        target_lang_code = lang_config["target_code"]
        spacy_target = self.resources["spacy_models"][target_lang_code]

        for block in data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                simple_target_tier = next((t for t in block.get("tiers", []) if t["tier_id"] == "simple_target"), None)
                if not simple_target_tier:
                    continue

                all_tier_lemmas = set() # Use a set to avoid duplicate lemma IDs

                # Reconstruct the full text for lemmatization
                full_simple_text = simple_target_tier.get("full_text", "")
                if not full_simple_text.strip():
                    continue
                
                full_doc = spacy_target(full_simple_text)
                
                # Create a map of token text to its lemma for easy lookup
                token_to_lemma = {
                    token.text: helper.normalize_spanish_lemma(token.lemma_)
                    for token in full_doc if not token.is_punct and not token.is_space
                }

                # Now, process each segment to create the final tokenized structure
                for seg in simple_target_tier.get("segments", []):
                    # The text is currently stored in a temporary placeholder from Stage 5b
                    raw_seg_text = "".join(t.get("v", "") for t in seg.get("tokenized_text", []))
                    
                    seg_doc = spacy_target(raw_seg_text)
                    
                    # Use the central tokenizer to create the base structure
                    final_token_list = helper.create_v2_token_list(raw_seg_text, seg_doc)
                    
                    # Populate diglot indices and lemma IDs
                    diglot_idx_counter = 0
                    for token in final_token_list:
                        if token["t"] == "w":
                            token["di"] = diglot_idx_counter
                            diglot_idx_counter += 1
                            
                            lemma_str = token_to_lemma.get(token["v"])
                            if lemma_str:
                                # We need to convert the lemma string to a numeric ID.
                                # This requires a dictionary, which we'll handle in the Rust preprocessor.
                                # For now, we'll store the string.
                                # TODO: When Rust preprocessor is refactored, this will store an ID.
                                token["l"] = [lemma_str] # Store as a list
                                all_tier_lemmas.add(lemma_str)
                    
                    # Replace the placeholder with the final, rich token list
                    seg["tokenized_text"] = final_token_list
                
                # Store the aggregated lemmas for the entire tier
                simple_target_tier["lemmas"] = sorted(list(all_tier_lemmas))
            
            block.setdefault("llm_call_status", {})[f"stage{self.stage_number}"] = "COMPLETED_SPACY"
        
        return data