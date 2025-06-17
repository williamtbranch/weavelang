from typing import Any, Dict

from .base import SpaCyStage, logger


class LemmatizeAdvSpanish(SpaCyStage):
    """
    Stage 2: Lemmatizes the 'adv_spanish_full' text for each sentence block.
    """

    def __init__(self, book_stem: str, config: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            config=config,
            common_resources=common_resources,
            stage_number=2,
            stage_name="LemmatizeAdvSpanish",
        )

    def _process_data(self, data: Dict[str, Any]) -> Dict[str, Any]:
        """
        Uses the Spanish SpaCy model to lemmatize the advanced Spanish text.

        Args:
            data: The full JSON data structure from the previous stage.

        Returns:
            The modified data structure with lemmas added.
        """
        spacy_es = self.resources.get("spacy_models", {}).get("es")
        if not spacy_es:
            logger.critical(
                "      -> CRITICAL: Spanish SpaCy model not found in common_resources."
            )
            # We should probably halt here, but for now we'll just return the data unmodified.
            # A more robust solution might raise an exception.
            return data

        for block in data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                # Ensure the adv_spanish_full object and lemmas list exist
                adv_spanish_obj = block.setdefault("adv_spanish_full", {})
                adv_spanish_obj.setdefault("lemmas", [])

                source_text = adv_spanish_obj.get("text", "")
                if source_text.strip():
                    doc = spacy_es(source_text)
                    # Create a list of lowercase lemmas, excluding punctuation, spaces, and proper nouns
                    lemmas = [
                        token.lemma_.lower()
                        for token in doc
                        if not token.is_punct
                        and not token.is_space
                        and token.pos_ != "PROPN"
                    ]
                    adv_spanish_obj["lemmas"] = lemmas
                else:
                    # If there's no source text, ensure the lemmas list is empty
                    adv_spanish_obj["lemmas"] = []

            # Mark this block as processed for this stage
            block.setdefault("llm_call_status", {})[f"stage{self.stage_number}"] = (
                "COMPLETED_SPACY"
            )

        return data
