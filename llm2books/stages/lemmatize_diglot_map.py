from typing import Any, Dict

from .base import SpaCyStage, logger


class LemmatizeDiglotMap(SpaCyStage):
    """
    Stage 8: Lemmatizes the 'exact_spanish_form' in each diglot map entry to
    populate the final 'spanish_lemma' field.
    """

    def __init__(self, book_stem: str, config: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            config=config,
            common_resources=common_resources,
            stage_number=8,
            stage_name="LemmatizeDiglotMap",
        )

    def _process_data(self, data: Dict[str, Any]) -> Dict[str, Any]:
        """
        Uses the Spanish SpaCy model to lemmatize the diglot map entries.

        Args:
            data: The full JSON data structure from Stage 7.

        Returns:
            The final, fully populated data structure for the book.
        """
        spacy_es = self.resources.get("spacy_models", {}).get("es")
        if not spacy_es:
            logger.critical(
                "      -> CRITICAL: Spanish SpaCy model not found in common_resources."
            )
            return data

        for block in data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                for entry in block.get("diglot_map_entries", []):
                    # Default the lemma to the English word for special cases like PROPER_NOUN or NO_SUB.
                    # This provides a useful, non-empty fallback.
                    spa_lemma = entry.get("english_word", "").lower()

                    # Only attempt to lemmatize if the entry is marked as a viable substitution.
                    if entry.get("note") == "viable":
                        spa_form = entry.get("exact_spanish_form", "")
                        if spa_form:
                            doc = spacy_es(spa_form)
                            # Find the first non-punctuation token to get the lemma from.
                            # This handles cases where the LLM might have included punctuation by mistake.
                            main_token = next((t for t in doc if not t.is_punct), None)
                            if main_token:
                                spa_lemma = main_token.lemma_.lower()
                            else:
                                # Fallback if the form was only punctuation (unlikely but safe)
                                spa_lemma = spa_form.lower()

                    entry["spanish_lemma"] = spa_lemma

            # Mark this block as processed for the final stage
            block.setdefault("llm_call_status", {})[f"stage{self.stage_number}"] = (
                "COMPLETED_SPACY"
            )

        return data
