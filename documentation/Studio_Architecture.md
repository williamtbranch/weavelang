# Studio Architecture

This file collects authoring-time concepts that aren't obvious from the code
but that content creators need to understand to read the GUI/terminal
correctly.

## Wlemma Buckets

Every frequency-rank shown in the studio (the `<NNN>` after a lemma in
`show detail`, `print`, `show mapping`, the GUI text view, etc.) is a
**wlemma bucket rank**, not the raw rank of the lemma string.

### What a wlemma is

A *wlemma* is a stemmed lemma key. At spaCy ingestion time we run the
target-language stemmer (Snowball Spanish today) over both the surface form
and the spaCy lemma; the resulting stem is the wlemma. Every word in the
sentence carries its wlemma alongside its lemma.

### Why it exists

spaCy regularly fails to lemmatize Spanish and returns the surface form
(`Niños` → `Niños`, `Corres` → `Corres`, `gritándoles` → `gritándoles`).
Because the master frequency list was also built with spaCy, those surface
forms appear as their own very-rare entries (`niños` at rank 52,370 next to
`niño` at 154). Without bucketing, a sentence using a top-200 word in any
inflected form would score as advanced and never make it into a basic
weave.

A wlemma collapses every inflection of a word family into a single bucket
keyed by the stem. The bucket's rank is the **minimum** rank of any member,
so the family is always graded by its most common form.

### How lookups resolve

When the algorithm or a UI surface needs the rank for a word it calls
`compute_wlemmas_for_word(surface, lemmas, stemmer, ranks)`, which:

1. Stems the surface form and stems each candidate lemma.
2. Looks each stem up in the bucket-rank map.
3. Picks the lower-rank stem (ties → the lemma stem).

That stem becomes the wlemma; its bucket rank is the rank you see in the
UI. This is what lets `gritándoles` resolve to the `gritar` bucket even
when spaCy returned `gritándoles` as the lemma.

### Practical impact for authoring

- The displayed `lemma<rank>` shows the bucket rank, not the lemma's own
  rank. Two different lemmas can show the same rank if they share a bucket.
- Use the terminal command **`wlemma <word>`** to inspect a bucket: it
  prints the stem, its rank, and every lemma in the loaded frequency list
  that maps to the same bucket.
- If you see a rank that surprises you, the bucket is the explanation.
  `wlemma <word>` will show you which lemma's rank is driving it.

### Schema and migration

`AppState.schema_version = 2` indicates a project file whose words,
segments, tiers and mapping entries all carry wlemma data. Older projects
are auto-upgraded on load (a re-save is required to persist the upgrade);
the CLI command `weavelang_cli upgrade-wvl <path> [--lang <code>]` upgrades
files in bulk.

### Pointers

- Full motivation, design rules, and phase-by-phase implementation history:
  [Wlemma_Migration_Plan.md](Wlemma_Migration_Plan.md).
- Stemmer trait and language factory: `src/domain/stemmer.rs`.
- Bucket construction and global accessors:
  `src/simulation/frequency_manager.rs` (see `build_bucket_rank`,
  `rank_of_wlemma`, `inspect_bucket`).
- Per-word wlemma computation: `src/domain/wlemma.rs`
  (`compute_wlemmas_for_word`).
