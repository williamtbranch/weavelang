# llm2books/stages/translate_basic_target.py
from typing import Any, Dict, List
import re

from .base import LLMStage, logger
from .. import llm_prompts, helper

# --- Configuration for the Human-in-the-Loop workflow ---
HUMAN_REVIEW_DIR_NAME = "human_review"
HUMAN_REVIEW_MARKER = "%%HUMAN_REVIEW_APPROVED%%"

class TranslateBasicTarget(LLMStage):
    """
    Stage 3 (V11): Reads the human-approved `basic_base` text file, translates it
    to create the `basic_target` tier, and then fully processes both tiers
    (tokenization, lemmatization) to prepare them for the mapping stages.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=3,
            stage_name="TranslateBasicTarget"
        )
        self.human_review_dir = self.pipeline_run_dir / HUMAN_REVIEW_DIR_NAME
        self.input_base_path = self.human_review_dir / f"{self.book_stem}.basic_en.txt" # Assuming base is 'en' for filename
        self.parser_type = "single_line"

        # This stage needs access to the SpaCy models
        self.spacy_base = self.resources["spacy_models"][self.resources["language_config"]["base_code"]]
        self.spacy_target = self.resources["spacy_models"][self.resources["language_config"]["target_code"]]
        
        # Cache the approved English text when preparing items
        self._approved_base_text: Dict[str, str] = {}

    def get_system_prompt(self) -> str:
        # A new prompt for high-fidelity, basic translation.
        return llm_prompts.get_system_prompt("translate_text_basic", self.resources["language_config"])

    def prepare_llm_items(self, book_data: Dict) -> List[Dict]:
        """
        Reads the approved basic base language file as the source of truth for translation.
        """
        logger.info(f"      -> Reading approved basic base text from: {self.input_base_path}")
        
        try:
            with open(self.input_base_path, 'r', encoding='utf-8') as f:
                content = f.read()
            if not content.strip().startswith(HUMAN_REVIEW_MARKER):
                raise FileNotFoundError(f"File is not approved. Uncomment the approval marker on the first line.")
        except (IOError, FileNotFoundError) as e:
            logger.error(f"      -> CRITICAL: Cannot read or find approved basic base file: {e}")
            raise e

        items_to_process = []
        sentence_regex = re.compile(r"^{(S\d+):\s*(.*)}$")
        for line in content.splitlines():
            match = sentence_regex.match(line.strip())
            if match:
                s_id, text = match.group(1), match.group(2).strip()
                if s_id and text:
                    self._approved_base_text[s_id] = text
                    items_to_process.append({"id": s_id, "text": text})
        
        return items_to_process

    def process_llm_results_for_block(self, block: Dict, llm_results: Dict[str, str]) -> Dict:
        """
        Takes the translated target text, finds the corresponding approved base text,
        and creates and processes both the `basic_base` and `basic_target` tiers.
        """
        s_id = block['s_id']
        
        basic_base_text = self._approved_base_text.get(s_id)
        basic_target_text = llm_results.get(s_id)

        if not basic_base_text or not basic_target_text:
            logger.warning(f"S_ID {s_id}: Missing source or translated text. Cannot process.")
            return block

        # Create and process the `basic_base` tier
        basic_base_tier = self._create_and_process_tier(
            tier_id="basic_base",
            full_text=basic_base_text,
            spacy_model=self.spacy_base,
            lang_code=self.resources["language_config"]["base_code"]
        )
        
        # Create and process the `basic_target` tier
        basic_target_tier = self._create_and_process_tier(
            tier_id="basic_target",
            full_text=basic_target_text,
            spacy_model=self.spacy_target,
            lang_code=self.resources["language_config"]["target_code"]
        )

        block.setdefault("tiers", []).extend([basic_base_tier, basic_target_tier])
        block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"
        return block

    def _create_and_process_tier(self, tier_id: str, full_text: str, spacy_model: Any, lang_code: str) -> Dict:
        """Helper to perform tokenization and lemmatization for a new tier."""
        tier = {"tier_id": tier_id, "full_text": full_text, "segments": [], "lemmas": []}
        doc = spacy_model(full_text)
        tokens = helper.create_golden_token_stream(doc)
        
        all_lemmas = set()
        di_counter = 0
        for token in tokens:
            if token['t'] == 'w':
                token['di'] = di_counter
                di_counter += 1
                
                for spacy_token in doc:
                    if spacy_token.text == token['v'] and spacy_token.idx == full_text.find(token['v']):
                        if lang_code == 'es':
                            lemma = helper.normalize_spanish_lemma(spacy_token.lemma_)
                        else:
                            lemma = spacy_token.lemma_.lower().strip()
                        if lemma:
                            token['l'] = [lemma]
                            all_lemmas.add(lemma)
                        break

        tier["segments"].append({
            "seg_id": "S1", "text": full_text,
            "tokenized_text": tokens, "lemmas": sorted(list(all_lemmas))
        })
        tier["lemmas"] = sorted(list(all_lemmas))
        return tier