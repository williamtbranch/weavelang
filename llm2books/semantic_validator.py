# llm2books/semantic_validator.py
import logging
from typing import Dict, Any, List, Tuple
from sentence_transformers import SentenceTransformer, util

logger = logging.getLogger("pipeline")

# --- Configuration ---
MODEL_NAME = 'paraphrase-multilingual-MiniLM-L12-v2'
SIMILARITY_THRESHOLD = 0.35
FAILURE_RATE_THRESHOLD = 0.4
MIN_PHRASES_FOR_VALIDATION = 4

# --- Global Model Cache ---
_model_cache = None

def _get_model():
    """Lazy-loads the sentence transformer model."""
    global _model_cache
    if _model_cache is None:
        logger.info(f"      -> Loading semantic validation model ('{MODEL_NAME}'). This may take a moment on first run...")
        try:
            _model_cache = SentenceTransformer(MODEL_NAME)
            logger.info("      -> Semantic model loaded successfully.")
        except Exception as e:
            logger.error(f"CRITICAL: Could not load SentenceTransformer model '{MODEL_NAME}'.")
            logger.error(f"Please ensure you have an internet connection for the first download, and that the 'sentence-transformers' and 'torch' libraries are installed.")
            raise e
    return _model_cache

def _get_forward_phrases_to_validate(sentence_block: Dict[str, Any]) -> List[Tuple[str, str]]:
    # ... (this function is unchanged) ...
    pairs_to_validate = []
    base_tier = next((t for t in sentence_block.get("tiers", []) if t.get("tier_id") == "base"), None)
    if not base_tier: return []

    di_to_english_phrase: Dict[int, str] = {
        token["di"]: token["v"]
        for seg in base_tier.get("segments", [])
        for token in seg.get("tokenized_text", [])
        if token.get("t") == "w" and "di" in token
    }

    diglot_map = sentence_block.get("mappings", {}).get("simple_target_to_base_diglot", {})
    for seg_id, entries in diglot_map.items():
        for entry in entries:
            is_viable = entry[3]
            if is_viable:
                base_di, spanish_phrase = entry[0], entry[2]
                english_phrase = di_to_english_phrase.get(base_di)
                if english_phrase and spanish_phrase and spanish_phrase.upper() != "PROPER_NOUN":
                    pairs_to_validate.append((english_phrase, spanish_phrase))
    return pairs_to_validate

# --- THIS IS THE RESTORED/CORRECTED FUNCTION ---
def _get_inverse_phrases_to_validate(sentence_block: Dict[str, Any]) -> List[Tuple[str, str]]:
    """
    Extracts viable Spanish Phrase -> English Substitute pairs from the inverse diglot map.
    """
    pairs_to_validate = []
    simple_target_tier = next((t for t in sentence_block.get("tiers", []) if t.get("tier_id") == "simple_target"), None)
    if not simple_target_tier: return []

    inv_diglot_map = sentence_block.get("mappings", {}).get("simple_target_to_base_inv_diglot", {})
    for seg_id, entries in inv_diglot_map.items():
        seg = next((s for s in simple_target_tier.get("segments", []) if s.get("seg_id") == seg_id), None)
        if not seg: continue
        
        virtual_word_tokens = [tok for tok in seg.get("tokenized_text", []) if tok.get("t") == "w"]
        
        for entry in entries:
            v_token_idx, english_substitute = entry[0], entry[2]
            
            if english_substitute.upper() not in ["NO_SUB", "PROPER_NOUN"] and v_token_idx < len(virtual_word_tokens):
                spanish_phrase = virtual_word_tokens[v_token_idx].get("v", "")
                if spanish_phrase and english_substitute:
                    pairs_to_validate.append((spanish_phrase, english_substitute))
    return pairs_to_validate
# --- END OF RESTORED/CORRECTED FUNCTION ---

def _run_validation_logic(s_id: str, model: SentenceTransformer, phrase_pairs: List[Tuple[str, str]], source_lang_name: str, target_lang_name: str) -> bool:
    """Core validation logic, with the small sample exemption."""
    
    if len(phrase_pairs) < MIN_PHRASES_FOR_VALIDATION:
        logger.debug(f"      -> Skipping semantic validation for S_ID {s_id}: Too few viable phrases ({len(phrase_pairs)}) to validate reliably.")
        return True

    source_phrases = [pair[0] for pair in phrase_pairs]
    target_phrases = [pair[1] for pair in phrase_pairs]

    try:
        source_embeddings = model.encode(source_phrases, convert_to_tensor=True)
        target_embeddings = model.encode(target_phrases, convert_to_tensor=True)
        cosine_scores = util.cos_sim(source_embeddings, target_embeddings)

        failed_mappings = 0
        for i in range(len(phrase_pairs)):
            score = cosine_scores[i][i].item()
            if score < SIMILARITY_THRESHOLD:
                failed_mappings += 1
                logger.debug(f"      -> Low similarity for S_ID {s_id}: '{source_phrases[i]}' ({source_lang_name}) vs '{target_phrases[i]}' ({target_lang_name}) (Score: {score:.2f})")
        
        failure_rate = failed_mappings / len(phrase_pairs)
        if failure_rate > FAILURE_RATE_THRESHOLD:
            logger.warning(f"      -> S_ID {s_id} failed semantic validation. Failure rate ({failure_rate:.2%}) exceeds threshold ({FAILURE_RATE_THRESHOLD:.2%}).")
            return False
    except Exception as e:
        logger.error(f"An error occurred during semantic validation for S_ID {s_id}: {e}")
        return False
        
    return True

def validate_forward_mappings(sentence_block: Dict[str, Any]) -> bool:
    """Public validator for the forward (English -> Spanish) map."""
    s_id = sentence_block.get("s_id", "Unknown")
    model = _get_model()
    phrase_pairs = _get_forward_phrases_to_validate(sentence_block)
    return _run_validation_logic(s_id, model, phrase_pairs, "English", "Spanish")

# --- THIS IS THE RESTORED PUBLIC FUNCTION ---
def validate_inverse_mappings(sentence_block: Dict[str, Any]) -> bool:
    """Public validator for the inverse (Simple Spanish -> English Substitute) map."""
    s_id = sentence_block.get("s_id", "Unknown")
    model = _get_model()
    phrase_pairs = _get_inverse_phrases_to_validate(sentence_block)
    return _run_validation_logic(s_id, model, phrase_pairs, "Spanish", "English")