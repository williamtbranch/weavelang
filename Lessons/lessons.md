# Auto-Lesson Generation Pipeline

Mirror of the Flashcards pipeline, retargeted to produce **per-lemma narrated
mini-lessons** that feed Weavelang as Spanish-source teaching books.

## Goals

- One lesson per lemma rank, ranks 0001–2000.
- 24 lessons per Weavelang input file → 84 books total
  (`lessons_0001-0024.txt` … `lessons_1977-2000.txt`).
- Each book becomes one Weavelang project; each of its 24 lessons is one
  chapter inside that project.
- Cadence: ~6 lessons / day → one book every 4 days → ~84 books over ~1 year.

## Directory layout

```
Lessons/
  lessons.md                       # this file
  lemma_senses_all.jsonl           # concat of Flashcards/lemma_senses_*.jsonl (grouped by lemma)
  special_words.txt                # ranks needing extra prompt guidance (one per line)
  generate_lesson.prompt           # default LLM prompt; emits the lesson body only (no headers)
  generate_book.py                 # driver: produces ONE book at a time (book_number arg)
  assemble_book.py                 # concatenates LLM_OUT/*.txt → Books/lessons_NNNN-MMMM.txt
  generate_book.log
  prompts/
    lesson_0001_el.prompt          # plugin prompt, appended to generate_lesson.prompt for this rank
    lesson_0014_ser.prompt
    ...
  LLM_OUT/
    lesson_0001.txt                # raw LLM output, body only (no %%META lines)
    lesson_0002.txt
    ...
    lesson_2000.txt
  Books/
    lessons_0001-0024.txt          # Weavelang input; 24 chapters, one %%META block per lesson
    lessons_0025-0048.txt
    ...
    lessons_1977-2000.txt
```

Filenames use **4-digit zero-padded** rank so they sort lexicographically
in `LLM_OUT/` and `Books/`.

## Input

`Flashcards/lemma_senses_*.jsonl` already contain everything we need
(`rank`, `normalized_lemma`, `unnormalized`, `pos`, `gloss`, `spanish`,
`english`, plus per-sense rows). One-time concat, grouped by
`normalized_lemma` so all senses of a word reach the LLM in a single
record:

```json
{
  "rank": 1,
  "normalized_lemma": "el",
  "senses": [
    {"unnormalized": "el",  "pos": "determiner", "gloss": "the (masc. sg. def. art.)", "spanish": "...", "english": "..."},
    {"unnormalized": "él",  "pos": "pronoun",    "gloss": "he, him",                    "spanish": "...", "english": "..."}
  ]
}
```

That grouping happens once during the concat step; output:
`Lessons/lemma_senses_all.jsonl` (2000 records).

## Vocabulary constraint

**No prompt-level vocabulary cap.** Weavelang's level system handles
restriction at weave time as long as each lesson's `lm_entry` matches
its lemma rank. The LLM may use whatever Spanish is natural for a fluent
narrator.

## Lesson body conventions (locked from `lesson_01_el.txt`)

- Casual `tú`, narrator addressing the learner directly.
- Mini-story with a named character; concrete scenes; sensory detail.
- Recap paragraph at end (`Resumen rapidito…`).
- Per-sense coverage when the lemma has multiple senses (e.g. `el` vs `él`).
- Body only — **no `%%META` lines** in `LLM_OUT/`. The assembler adds them.

### Length / runtime targets

Lessons serve a dual purpose: teach the lemma **and** deliver comprehensible
input. Target spoken runtime is **6–10 minutes minimum** for a typical
lesson, and up to **~30 minutes** for marquee lemmas (e.g. *ser*, *estar*,
*haber*).

Short/concrete lemmas (e.g. *manzana* "apple") still need to hit the 6-min
floor, so the prompt instructs the LLM to expand into a **mini-Wikipedia-like
brief** for the word: etymology, history, cultivation/usage, regional
variants, idioms, fun facts — woven naturally into the narrative or as a
*"Dato curioso"* aside. This project is, in effect, a fun spoken Wikipedia
of the 2000 most frequent Spanish lemmas.

High-load lemmas (verb *to be*, irregular high-frequency verbs, polysemous
function words) are flagged via `special_words.txt` and given a plugin
prompt that demands fuller treatment of conjugations, registers, and
etymology.

### Special words & plugin prompts

File: `Lessons/special_words.txt` — one line per rank that needs extra
attention. Format:

```
# rank<TAB>normalized_lemma<TAB>note
0001\tel\tdeterminer vs pronoun él, accent contrast
0014\tser\tcore copula; full conjugation tour; ~30 min
```

For any rank listed there, the driver looks for a plugin prompt at
`Lessons/prompts/lesson_{rank:04d}_{lemma}.prompt` and **appends its
contents to the bottom of `generate_lesson.prompt`** before sending to
the LLM. Missing plugin file with a special-word entry → driver errors
out (fail loud; we want to notice).

Workflow when a generated lesson disappoints:

1. Add the rank+lemma to `special_words.txt`.
2. Author `Lessons/prompts/lesson_{rank:04d}_{lemma}.prompt` with the
   targeted guidance.
3. Delete `Lessons/LLM_OUT/lesson_{rank:04d}.txt`.
4. Re-run the generator (it will produce just that one lesson).
5. Re-run the assembler to refresh the affected book.

## LLM driver (`generate_book.py`)

Mirrors `Flashcards/generate_full_deck.py` but with **strict single-book
scope** to prevent runaway spend over the year-long rollout.

- Model: **Claude Opus 4.7** (same as Flashcards generator).
- Anthropic SDK + keyring lookup of `anthropic_api_key.weavelang`.
- **Required positional arg: `book_number`** (1..84). The driver computes
  `rank_min = (book-1)*24 + 1`, `rank_max = book*24`. There is **no**
  `--rank-min` / `--rank-max`, no `--all`, no batch mode. Producing more
  than one book requires invoking the script multiple times by hand.
- **Up-to-date guard**: before doing anything, the driver checks whether
  `Books/lessons_{rank_min:04d}-{rank_max:04d}.txt` exists and is newer
  than every dependency:
    - all 24 `LLM_OUT/lesson_{rank:04d}.txt`,
    - `generate_lesson.prompt`,
    - any plugin prompt in `prompts/` referenced by `special_words.txt`
      for ranks in this book,
    - `special_words.txt` itself,
    - `lemma_senses_all.jsonl`.
  If the book file is up-to-date the driver **refuses to run** and
  exits 0 with a message. Override only via `--force` (which still
  produces just this one book).
- **Resume within the book**: any of the 24 ranks already present in
  `LLM_OUT/` are skipped (file existence is the resume signal). Delete
  a single `LLM_OUT/lesson_NNNN.txt` to force regeneration of just that
  lesson.
- **Special-word handling**: for each rank in the run, if the rank is
  listed in `special_words.txt`, the driver loads the matching
  `prompts/lesson_{rank:04d}_{lemma}.prompt` and concatenates it to the
  bottom of `generate_lesson.prompt` for that single LLM call.
- After all 24 lessons are present, the driver invokes the assembler
  in-process to rebuild the book file. End state: one fresh book on
  disk, ready to import into Weavelang.

## Book assembler (`assemble_books.py`)

Walks `LLM_OUT/` in rank order; every 24 lessons, emits a book file in
`Books/`:

```
%%META source_language: es%%
%%META target_language: es%%
%%META teaching_mode: on%%
%%META lm_entry: bas=1%%
%%META chapter: lesson_0001_el%%

<body of lesson_0001.txt>

%%META lm_entry: bas=2%%
%%META chapter: lesson_0002_de%%

<body of lesson_0002.txt>

...
```

Idempotent: re-running rebuilds any book whose member lessons changed
(or simply skips if all 24 lessons exist and the book file's mtime is
fresher than every member).

### Weavelang chapter directive (implemented)

`%%META chapter: <name>%%` is recognised by `src/parsing/source_parser.rs`.
It anchors a chapter at the **next sentence**; end indices are computed
automatically (each chapter ends one sentence before the next starts;
the last chapter ends at the document's last sentence). The engine
populates `state.chapters` on import and flips `chapter_mode = true`,
so the assembler's output drops in directly without any UI step.

## Resolved decisions

1. **Length target** → 6–10 min minimum spoken runtime per lesson;
   up to ~30 min for marquee lemmas. Padding strategy = mini-Wikipedia
   asides (etymology, history, fun facts). See *Length / runtime
   targets* above.
2. **POS branching** → handled inline in the single main prompt with an
   explicit "if the lemma is a verb / determiner / pronoun / preposition
   / noun / adjective / adverb, do X" section. POS-per-sense is captured
   in `lemma_senses_all.jsonl` via the per-sense `pos` field. If a
   particular case turns out to need more depth, escalate it via
   `special_words.txt` + plugin prompt rather than forking the main
   prompt.
3. **Spot-check protocol** → user reads every book before release;
   small fixes are made by hand-editing `LLM_OUT/lesson_NNNN.txt` and
   re-running the assembler. Larger issues → `special_words.txt` +
   plugin prompt + regenerate that one lesson.
4. **`lm_entry` value** → `bas=N` mirrors the rank for all 2000 lemmas.
   Each lemma advances the base level by exactly 1.
5. **Smoke-test scope** → book 1 (ranks 1–24) is smoke-tested manually,
   uploaded to YouTube, and lessons learned feed back into the prompt
   before book 2. Books are produced one at a time across ~1 year; the
   single-book driver guardrail enforces this.

## Variability / theme injection (future enhancement)

After reviewing book 1, the lessons share recognisable surface patterns
across chapters: stock openings/closings (`Hola, qué tal` /
`Resumen rapidito, antes de despedirnos` / `Que tengas un día
estupendo`), a recurring "primo / prima" metaphor for diacritic pairs,
and a small repertoire of mini-story settings (kitchen with abuela,
school courtyard, mountain pueblo). Within one book this is a feature
(consistent narrator persona); across 83 books it risks listener
fatigue.

**Standard fix**: parameterise the prompt with a small set of theme /
setting / opening / closing slots and randomly pick 1–2 per lesson.
This is essentially the **template-with-slot-fillers** pattern from
data-augmentation / NLG literature (e.g. Jia & Liang 2016 on
compositional templates; the "template bank + random sampling"
approach used in instruction-tuning datasets like FLAN). For our
purposes a single-stage random pick from curated lists is enough — we
don't need full grammar-based generation.

**Proposed implementation** (when we get to it):

1. Add `Lessons/themes.toml` with curated lists, e.g.

   ```toml
   [openings]
   variants = [
     "Hola, qué tal.",
     "Buenas, amigo.",
     "Hola de nuevo.",
     "¿Cómo va todo?",
     # ...
   ]

   [closings]
   variants = [
     "Que tengas un día estupendo, y nos vemos en la siguiente lección.",
     "Hasta la próxima, cuídate mucho.",
     # ...
   ]

   [story_settings]
   variants = [
     "una cocina rural con olor a pan recién hecho",
     "un patio de escuela una mañana de otoño",
     "un mercado de pueblo un sábado por la mañana",
     "una biblioteca pequeña en un día de lluvia",
     "un taller de carpintería con virutas en el suelo",
     "un huerto al atardecer",
     # ...
   ]

   [character_pool]
   variants = [
     "Mateo, niño de unos diez años",
     "Lucía, una niña curiosa de nueve años",
     "el abuelo Tomás, panadero jubilado",
     "la abuela Remedios, que vive en Asturias",
     # ...
   ]

   [tone_garnish]
   variants = [
     "incluye un refrán popular",
     "incluye una breve canción de cuna o rima",
     "menciona un dato curioso de etimología",
     "termina con una pregunta abierta para el oyente",
     # ...
   ]
   ```

2. `generate_book.py` loads `themes.toml` and, per lesson, samples
   (deterministically seeded by `rank`, so regeneration is
   reproducible) one element from each list.

3. Sampled values are interpolated into `generate_lesson.prompt` via
   simple `{{opening}} / {{closing}} / {{setting}} / {{character}} /
   {{garnish}}` placeholders. The prompt instructs the LLM to **use
   the suggested elements naturally** rather than parroting them
   verbatim — phrasing them as "consider opening with something like
   …" gives the LLM room to adapt while still steering the
   distribution.

4. The plugin-prompt mechanism (`special_words.txt` + per-rank
   `prompts/lesson_NNNN_*.prompt`) wins over the global theme pool —
   marquee verbs may need their own bespoke setup that overrides the
   random pick.

**Determinism note**: seed the RNG with the lemma rank so the same
rank always picks the same theme. Keeps regeneration reproducible
and avoids "the lesson silently changed flavour when I re-ran it"
confusion.

**Why not let the LLM "be more varied"?** Tested empirically across
many domains: open-ended "vary your style" instructions don't
actually increase distributional diversity — the model converges on
its own modes. Explicit slot-filler sampling is the cheap, reliable
fix.

**Status**: not blocking book 2. Revisit after listening to books 1–3
and confirming the repetition is genuinely tiresome (it may not be —
consistent narrator persona has its own value).

## Lemma whitelist / pre-known forms (open question)

When the `el` lesson casually uses `del` ("la casa **del** abuelo")
to teach the `de + el` contraction, Weavelang's level mapper sees
`del` as out-of-scope at `bas=1` because `del` doesn't have its own
lemma entry until rank 10. Same problem for `al` (rank 26ish), and
likely a long tail of contracted/cliticised forms.

**Engine status**: there is no manual lemma whitelist mechanism today.
`known_lemmas` (`bin/simulator.rs`) and `frontier_known_lemmas`
(`simulation/core_algo.rs`) are runtime-tracked sets populated by
encountered ranks during simulation, not user-editable allowlists.
The audit-time `unknown_lemmas` (`engine.rs` ~line 3955) is
diagnostic output only.

**Two paths forward** (defer until pattern recurs):

1. **Per-book mapping patch** — manually edit the lemma-mapping JSON
   for affected books to alias `del`→`de`/`el`. Surgical, no engine
   change. Use for one-offs.

2. **Global pre-known list** — new asset
   `assets/always_known_es.txt` (or per-target-language); engine
   loads it during the level-system bring-up and unions into
   `known_lemmas` from chapter 1 / `bas=1`. Catches `al`, `del`, and
   any other "functionally L1 from day 1" forms with one edit.
   Roughly half-day of work: load file, plumb into `LevelSystem`
   bring-up, gate with a flag so it only applies in
   `teaching_mode: on` projects (regular reading projects should
   keep the strict audit). Worth it if more contractions trip the
   mapper across early books.

**Decision**: ship book 1 with manual mapping edits for `del`. Track
how many similar forward-references show up in books 2–5; if it's
more than ~3, build option 2.

## TODO (must-fix before pipeline ships)

- [x] **Weavelang chapter directive** — `%%META chapter: <name>%%`
  implemented in `src/parsing/source_parser.rs`; engine populates
  `state.chapters` and sets `chapter_mode = true` on import.
- [x] One-time `Flashcards/lemma_senses_*.jsonl` →
  `Lessons/lemma_senses_all.jsonl` merge, grouped by `normalized_lemma`.
  Driver: [Lessons/merge_lemma_senses.py](merge_lemma_senses.py).
  Result: **1978 lemma records / 4363 senses / ranks 1–2000**, with 22
  ranks missing from the source jsonls (presumably filtered during
  Flashcards generation): 299, 347, 350, 416, 524, 568, 740, 807, 987,
  1028, 1117, 1129, 1148, 1298, 1340, 1378, 1542, 1655, 1742, 1825,
  1957, 1980. Decision pending: pack books densely (24 lemmas per
  book regardless of rank, ~83 books) or back-fill the gaps first.

  **Resolution**: skip them. Looked up against
  `assets/frequency_lists/es_master_frequency_list.txt`; all 22 are
  corpus noise — single letters (`i`, `c`, `b`, `n`, `m`, `d`, `s`,
  `x`, `g`, `v`, `f`, `h`), spaCy lemmatisation errors that produced
  non-words (`deberiar`, `podriar`, `hagar`, `digar`, `oer`),
  inflected forms that failed to lemmatise (`dio`, `dame`, `dijiste`),
  and fragments (`dema`, `ce`). None merit a lesson. Books pack
  densely: 1978 lemmas / 24 ≈ **82 full books + 1 partial book of 10
  lessons** (or fold the remainder into book 82 to keep all books at
  exactly 24 chapters — TBD). **Each lesson preserves its true
  frequency rank** (`lm_entry: bas=N` uses the rank from
  `lemma_senses_all.jsonl`, not the position within the book), so
  Weavelang's level system stays correctly synchronised with the
  global lemma frequency ordering even when ranks have gaps.
- [x] Author `Lessons/generate_lesson.prompt` (no vocabulary cap;
  includes POS-handling section, Wikipedia-padding instructions,
  6–10 min runtime target).
- [x] Author `Lessons/generate_book.py` (single-book, up-to-date guard,
  plugin-prompt support).
- [x] Author `Lessons/assemble_book.py` (idempotent rebuild of one
  `Books/lessons_NNNN-MMMM.txt`).
- [ ] Seed `Lessons/special_words.txt` with the obvious heavy hitters
  (*ser*, *estar*, *haber*, *tener*, *hacer*, *ir*, *poder*, *querer*,
  *de*, *que*, *no*, *se*, …).
- [ ] **`source_is_basic` directive** — assertion that the source file is
  already authored at the basic/simple-reader level. When set (with
  `simple_mode: on`), the engine skips the LLM simplify pass that would
  otherwise produce the basic tier in the source's own language; that
  tier instead receives a verbatim copy of `base` (still segmented).
  The cross-language basic tier is generated normally.

  | Project | Stage skipped | Tier copied verbatim from `base` | Tier still LLM-generated |
  |---|---|---|---|
  | en-es | `GenerateBasicBase` (`simplify_to_basic_english`) | `basic_base` ← `base` | `basic_target` ← `basic_base` |
  | es-es | `GenerateBasicTarget` (`basic_target_simplify_es`) | `basic_target` ← `base` | `basic_base` ← `basic_target` |

  Implementation: add `pub source_is_basic: bool` to `SourceMeta` and
  `AppState`; extend `StageResolution` with `copy_from_source_tier:
  bool`; pass `source_is_basic` into `stage_dispatch`; LLM job runner
  short-circuits when `copy_from_source_tier` is true (clones source
  tier → target tier verbatim, no API call). `assemble_book.py` stamps
  `%%META source_is_basic: on%%` on every Books/lessons_*.txt.

  **Status**: book 1 already paid for the simplification pass on
  chapter 1; chapters 2–24 of book 1 can be re-derived once the
  directive ships (delete `basic_target` cache for those chapters,
  re-run stage). Books 2–83 benefit automatically (~25k LLM calls
  saved across the rollout).

## Reference: existing artifacts

- Hand-authored gold standard: [Lessons/lesson_01_el.txt](lesson_01_el.txt)
- Flashcards prompts:
  [Flashcards/generate_final_cards_prompt_a.prompt](../Flashcards/generate_final_cards_prompt_a.prompt),
  [Flashcards/generate_final_cards_prompt_b.prompt](../Flashcards/generate_final_cards_prompt_b.prompt)
- Flashcards driver:
  [Flashcards/generate_full_deck.py](../Flashcards/generate_full_deck.py)
- Lemma input segments:
  [Flashcards/lemma_senses_1-20.jsonl](../Flashcards/lemma_senses_1-20.jsonl),
  [Flashcards/lemma_senses_21-100.jsonl](../Flashcards/lemma_senses_21-100.jsonl),
  [Flashcards/lemma_senses_101-500.jsonl](../Flashcards/lemma_senses_101-500.jsonl),
  [Flashcards/lemma_senses_501-2000.jsonl](../Flashcards/lemma_senses_501-2000.jsonl)
