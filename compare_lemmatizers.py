"""
One-off comparison of spaCy (with our normalize_spanish_lemma) vs Stanza
lemmatization on the advanced Spanish output.

For each lemmatizer:
  - Lemmatize every word
  - Build per-document frequency rank (rank 1 = most frequent in this doc)
  - Compute mean rank per token

Then compare:
  - % of word tokens whose lemma differs between the two
  - Mean rank per token (lower means better consolidation)
"""

import re
import sys
import io
import unicodedata
from collections import Counter

import spacy
import stanza

# Force UTF-8 stdout on Windows so arrows and accents print cleanly.
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")


INPUT = (
    r"E:\Bill\Documents\development\audiolingual\weave_out"
    r"\La_Llarona_v2_pipeline_test\La_Llarona\whole_book\tts_files"
    r"\La_Llarona_ULa34.txt"
)


def normalize_spanish_lemma(s: str) -> str:
    """Mirror of src/domain/normalization.rs::normalize_spanish_lemma.
    - lowercase
    - strip diacritics (keep ñ→n folding as the Rust code does)
    - keep only the first space-delimited segment
    - strip non-letter padding
    """
    s = s.strip().lower()
    if not s:
        return ""
    # Take first whitespace-segment, strip surrounding non-letters.
    s = s.split()[0]
    s = re.sub(r"^[^a-záéíóúüñ]+|[^a-záéíóúüñ]+$", "", s)
    if not s:
        return ""
    # Reject tokens that contain digits.
    if re.search(r"\d", s):
        return ""
    # Strip diacritics.
    s = "".join(c for c in unicodedata.normalize("NFD", s) if unicodedata.category(c) != "Mn")
    return s


def main() -> int:
    text = open(INPUT, encoding="utf-8").read()

    print(f"Input: {INPUT}")
    print(f"Chars: {len(text):,}\n")

    # --- spaCy (matching what the Rust app uses via linguistic_engine.py) ---
    print("Loading spaCy es_core_news_lg…")
    nlp = spacy.load("es_core_news_lg", disable=["ner", "parser"])
    doc = nlp(text)
    spacy_tokens = []  # (start, surface, normalized_lemma)
    for tok in doc:
        if tok.is_space or tok.is_punct:
            continue
        if not tok.text.strip():
            continue
        if re.fullmatch(r"\d+([.,]\d+)?", tok.text):
            continue
        norm = normalize_spanish_lemma(tok.lemma_ or tok.text)
        if norm:
            spacy_tokens.append((tok.idx, tok.text, norm))

    # --- Stanza ---
    print("Loading Stanza es (downloading model if needed)…")
    stanza.download("es", verbose=False)
    snlp = stanza.Pipeline(
        lang="es",
        processors="tokenize,mwt,pos,lemma",
        tokenize_pretokenized=False,
        verbose=False,
    )
    sdoc = snlp(text)
    stanza_tokens = []
    for sent in sdoc.sentences:
        for w in sent.words:
            if not w.text or not w.text.strip():
                continue
            if re.fullmatch(r"[\W_]+", w.text):
                continue
            if re.fullmatch(r"\d+([.,]\d+)?", w.text):
                continue
            lemma = w.lemma if w.lemma else w.text
            norm = normalize_spanish_lemma(lemma)
            if not norm:
                continue
            start = getattr(w, "start_char", None)
            if start is None:
                parent = getattr(w, "parent", None)
                if parent is not None:
                    start = getattr(parent, "start_char", None)
            stanza_tokens.append((start if start is not None else -1, w.text, norm))

    print(f"\nspaCy tokens kept:  {len(spacy_tokens):,}")
    print(f"Stanza tokens kept: {len(stanza_tokens):,}")

    # Align by character start offset; surface forms should match.
    i = j = 0
    matched = 0
    diffs = 0
    diff_examples = Counter()
    while i < len(spacy_tokens) and j < len(stanza_tokens):
        s_start, s_surf, s_norm = spacy_tokens[i]
        z_start, z_surf, z_norm = stanza_tokens[j]
        if z_start == -1:
            j += 1
            continue
        if abs(s_start - z_start) <= 1 and s_surf.lower() == z_surf.lower():
            matched += 1
            if s_norm != z_norm:
                diffs += 1
                diff_examples[(s_surf, s_norm, z_norm)] += 1
            i += 1
            j += 1
        elif s_start < z_start:
            i += 1
        else:
            j += 1

    print(f"\nCleanly aligned tokens: {matched:,}")
    if matched > 0:
        print(f"Lemma disagreements:    {diffs:,} ({100.0*diffs/matched:.2f}%)")

    print("\nTop 30 disagreement patterns (count, surface, spaCy, Stanza):")
    for (surf, sl, tl), c in diff_examples.most_common(30):
        print(f"  {c:>4}x  surf={surf!r:<22} spaCy={sl!r:<22} Stanza={tl!r}")

    # Per-document rank: rank 1 = most frequent lemma in this doc.
    def doc_rank_stats(toks):
        lemmas = [t[2] for t in toks]
        freq = Counter(lemmas)
        # Rank by descending frequency, ties broken alphabetically for stability.
        ordered = sorted(freq.items(), key=lambda kv: (-kv[1], kv[0]))
        rank = {lem: i + 1 for i, (lem, _) in enumerate(ordered)}
        token_ranks = [rank[l] for l in lemmas]
        mean = sum(token_ranks) / len(token_ranks)
        unique = len(freq)
        return mean, unique, len(lemmas)

    sm, su, sn = doc_rank_stats(spacy_tokens)
    tm, tu, tn = doc_rank_stats(stanza_tokens)

    print("\nIn-document rank statistics (rank 1 = most frequent in *this* text):")
    print(f"  spaCy:  mean rank = {sm:8.2f}  unique lemmas = {su:>5}  tokens = {sn:>6}")
    print(f"  Stanza: mean rank = {tm:8.2f}  unique lemmas = {tu:>5}  tokens = {tn:>6}")
    delta = sm - tm
    print(f"  Δ mean rank (spaCy − Stanza) = {delta:+.2f}")
    print(
        f"  Stanza unique-lemma reduction vs spaCy: "
        f"{su - tu:+d} ({100.0*(su-tu)/su:+.2f}%)"
    )

    return 0


if __name__ == "__main__":
    sys.exit(main())
