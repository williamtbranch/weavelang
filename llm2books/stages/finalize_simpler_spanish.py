from typing import Any, Dict

from .base import SpaCyStage, logger


class FinalizeSimplerSpanish(SpaCyStage):
    """
    Stage 4: Lemmatizes both advanced and simpler segments, and aggregates the
    simpler segments into a full sentence text and lemma list.
    """

    def __init__(self, book_stem: str, config: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            config=config,
            common_resources=common_resources,
            stage_number=4,
            stage_name="FinalizeSimplerSpanish",
        )

    def _process_data(self, data: Dict[str, Any]) -> Dict[str, Any]:
        """
        Performs the lemmatization and aggregation for Stage 4.

        Args:
            data: The full JSON data structure from Stage 3.

        Returns:
            The modified data structure with segment lemmas and the aggregated
            'simpler_adv_spanish_full' object populated.
        """
        spacy_es = self.resources.get("spacy_models", {}).get("es")
        if not spacy_es:
            logger.critical(
                "      -> CRITICAL: Spanish SpaCy model not found in common_resources."
            )
            return data

        for block in data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                all_simpler_texts: list[str] = []
                all_simpler_lemmas: list[str] = []

                # Loop through the segments to lemmatize and collect simpler parts
                for seg in block.get("adv_spanish_segments", []):
                    # 1. Lemmatize the original advanced_text segment
                    adv_doc = spacy_es(seg.get("advanced_text", ""))
                    seg["advanced_lemmas"] = [
                        token.lemma_.lower()
                        for token in adv_doc
                        if not token.is_punct
                        and not token.is_space
                        and token.pos_ != "PROPN"
                    ]

                    # 2. Lemmatize the new simpler_text segment
                    simpler_doc = spacy_es(seg.get("simpler_text", ""))
                    simpler_lemmas = [
                        token.lemma_.lower()
                        for token in simpler_doc
                        if not token.is_punct
                        and not token.is_space
                        and token.pos_ != "PROPN"
                    ]
                    seg["simpler_lemmas"] = simpler_lemmas

                    # 3. Collect the simpler parts for aggregation
                    all_simpler_texts.append(seg.get("simpler_text", ""))
                    all_simpler_lemmas.extend(simpler_lemmas)

                # 4. Assemble the aggregated full sentence object
                block["simpler_adv_spanish_full"] = {
                    "text": " ".join(all_simpler_texts).strip(),
                    "lemmas": all_simpler_lemmas,
                }

            # Mark this block as processed for this stage
            block.setdefault("llm_call_status", {})[f"stage{self.stage_number}"] = (
                "COMPLETED_SPACY"
            )

        return data
