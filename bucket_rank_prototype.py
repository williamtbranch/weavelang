"""
Prototype: bucketed-stem frequency lookup vs current direct-lemma lookup.

For every word token in La_Llarona_ULa34.txt:
  - direct_rank = freq_list.get(normalize(spacy_lemma))
  - bucket_rank = min { freq_list[lem] for lem in freq_list with stem(lem)==stem(surface) }

Reports per-token rank distributions to show whether bucketing pulls the
"falsely rare" surface-form entries down to their true root rank.
"""

import io
import re
import sys
import unicodedata
from collections import Counter, defaultdict

import spacy
from nltk.stem.snowball import SnowballStemmer

sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")

INPUT = (
    r"E:\Bill\Documents\development\audiolingual\weave_out"
    r"\La_Llarona_v2_pipeline_test\La_Llarona\whole_book\tts_files"
    r"\La_Llarona_ULa34.txt"
)
FREQ_LIST = r"e:\Bill\development\weavelang\assets\frequency_lists\es_master_frequency_list.txt"

stemmer = SnowballStemmer("spanish")


def normalize(s: str) -> str:
    """Mirror src/domain/normalization.rs::normalize_spanish_lemma."""
    s = (s or "").strip().lower()
    if not s:
        return ""
    s = s.split()[0]
    s = re.sub(r"^[^a-záéíóúüñ]+|[^a-záéíóúüñ]+$", "", s)
    if not s or re.search(r"\d", s):
        return ""
    s = "".join(c for c in unicodedata.normalize("NFD", s)
                if unicodedata.category(c) != "Mn")
    return s


def stem(norm: str) -> str:
    if not norm:
        return ""
    # SnowballStemmer expects the original (with diacritics ideally), but our
    # normalize already stripped them. Stemmer still works fine on stripped.
    return stemmer.stem(norm)


def load_freq_list(path: str):
    """Return (lemma_rank: dict[str,int], bucket_rank: dict[str,int],
    bucket_members: dict[str, list[(lemma, rank)]])."""
    lemma_rank = {}
    bucket_rank = {}
    bucket_members = defaultdict(list)
    with open(path, encoding="utf-8") as f:
        next(f)  # header
        for line in f:
            parts = line.rstrip("\n").split("\t")
            if len(parts) < 2:
                continue
            lemma_raw, rank_s = parts[0], parts[1]
            try:
                rank = int(rank_s)
            except ValueError:
                continue
            n = normalize(lemma_raw)
            if not n:
                continue
            # Keep the lowest rank if dups normalize to the same key.
            if n not in lemma_rank or rank < lemma_rank[n]:
                lemma_rank[n] = rank
            st = stem(n)
            if not st:
                continue
            bucket_members[st].append((n, rank))
            if st not in bucket_rank or rank < bucket_rank[st]:
                bucket_rank[st] = rank
    return lemma_rank, bucket_rank, bucket_members


def main() -> int:
    print("Loading frequency list…")
    lemma_rank, bucket_rank, bucket_members = load_freq_list(FREQ_LIST)
    print(f"  {len(lemma_rank):,} unique normalized lemmas")
    print(f"  {len(bucket_rank):,} unique stem buckets")
    print(f"  Avg lemmas per bucket: {len(lemma_rank)/len(bucket_rank):.2f}")

    print("\nLoading spaCy es_core_news_lg…")
    nlp = spacy.load("es_core_news_lg", disable=["ner", "parser"])
    text = open(INPUT, encoding="utf-8").read()
    doc = nlp(text)

    n_tokens = 0
    direct_hits = 0
    bucket_lem_hits = 0
    bucket_surf_hits = 0
    bucket_min_hits = 0
    direct_ranks = []
    bucket_lem_ranks = []   # stem(spacy_lemma) only
    bucket_surf_ranks = []  # stem(surface) only
    bucket_min_ranks = []   # min of the two
    rescued = []
    regressed = []

    for tok in doc:
        if tok.is_space or tok.is_punct or not tok.text.strip():
            continue
        if re.fullmatch(r"\d+([.,]\d+)?", tok.text):
            continue
        surf = tok.text
        spacy_lem_norm = normalize(tok.lemma_ or surf)
        lem_stem = stem(spacy_lem_norm)
        surf_stem = stem(normalize(surf))
        if not (lem_stem or surf_stem):
            continue
        n_tokens += 1

        d = lemma_rank.get(spacy_lem_norm)
        bl = bucket_rank.get(lem_stem) if lem_stem else None
        bs = bucket_rank.get(surf_stem) if surf_stem else None
        bm = None
        if bl is not None and bs is not None:
            bm = min(bl, bs)
        elif bl is not None:
            bm = bl
        elif bs is not None:
            bm = bs

        if d is not None:
            direct_hits += 1
            direct_ranks.append(d)
        if bl is not None:
            bucket_lem_hits += 1
            bucket_lem_ranks.append(bl)
        if bs is not None:
            bucket_surf_hits += 1
            bucket_surf_ranks.append(bs)
        if bm is not None:
            bucket_min_hits += 1
            bucket_min_ranks.append(bm)

        if d is not None and bm is not None and d > bm * 5 and d > 1000:
            rescued.append((surf, tok.lemma_, spacy_lem_norm, d,
                            lem_stem, surf_stem, bm))
        if d is not None and bm is not None and bm > d * 5 and bm > 1000:
            regressed.append((surf, tok.lemma_, spacy_lem_norm, d,
                              lem_stem, surf_stem, bm))

    print(f"\nWord tokens analyzed: {n_tokens:,}")
    print(f"  Direct (spaCy lemma -> rank) hits: {direct_hits:,}")
    print(f"  stem(spaCy lemma) bucket hits:     {bucket_lem_hits:,}")
    print(f"  stem(surface)     bucket hits:     {bucket_surf_hits:,}")
    print(f"  min-of-both       bucket hits:     {bucket_min_hits:,}")

    def stats(label, ranks):
        if not ranks:
            print(f"  {label}: no hits")
            return
        rs = sorted(ranks)
        n = len(rs)
        mean = sum(rs) / n
        median = rs[n // 2]
        p90 = rs[int(n * 0.9)]
        p99 = rs[min(n - 1, int(n * 0.99))]
        mx = rs[-1]
        gt10k = sum(1 for r in rs if r > 10_000)
        gt50k = sum(1 for r in rs if r > 50_000)
        print(f"  {label}:")
        print(f"    mean={mean:>10,.0f}  median={median:>6,}  p90={p90:>7,}  "
              f"p99={p99:>8,}  max={mx:>9,}  >10k={gt10k:>3}  >50k={gt50k:>3}")

    print("\n--- Rank distribution (lower = more common) ---")
    stats("Direct          ", direct_ranks)
    stats("stem(lemma)     ", bucket_lem_ranks)
    stats("stem(surface)   ", bucket_surf_ranks)
    stats("min(lem,surface)", bucket_min_ranks)

    print(f"\nTokens RESCUED (direct >> bucket-min, ratio > 5x, direct > 1k): "
          f"{len(rescued)}")
    rescued.sort(key=lambda r: -r[3])
    for surf, lem, lemn, d, ls, ss, bm in rescued[:20]:
        print(f"  {surf!r:<22} spaCy={lem!r:<22} direct={d:>8,}  "
              f"lem_stem={ls!r:<10} surf_stem={ss!r:<10} bucket_min={bm:>5,}")

    print(f"\nTokens REGRESSED (bucket-min >> direct, ratio > 5x, bm > 1k): "
          f"{len(regressed)}")
    regressed.sort(key=lambda r: -r[6])
    for surf, lem, lemn, d, ls, ss, bm in regressed[:20]:
        print(f"  {surf!r:<22} spaCy={lem!r:<22} direct={d:>6,}  "
              f"lem_stem={ls!r:<10} surf_stem={ss!r:<10} bucket_min={bm:>8,}")

    # Inspect a few specific known-bad cases.
    print("\n--- Spot-check known-bad cases ---")
    for w in ["niños", "ninos", "niño", "conductores", "conductor",
              "corres", "correr", "camioneros", "camionero", "gritándoles"]:
        n = normalize(w)
        st_ = stem(n)
        d = lemma_rank.get(n)
        b = bucket_rank.get(st_)
        print(f"  surf={w!r:<14} norm={n!r:<14} stem={st_!r:<10} "
              f"direct={d}  bucket={b}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
