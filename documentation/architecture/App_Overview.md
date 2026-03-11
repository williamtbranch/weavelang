# WeaveLang App Overview

WeaveLang is a tool for creating **graded bilingual reading material**. It takes a source text in one language (e.g., English) and produces a set of **weave files** — mixed-language texts where the proportion of the target language (e.g., Spanish) increases with each user level.

A reader at level 1 reads almost entirely in their native language. By level 34, they're reading fluent target-language prose. The system bridges that gap gradually.

> **Related docs:**
> - [Terminal_Commands.md](Terminal_Commands.md) — Full command reference
> - [agent.md](agent.md) — AI co-pilot quick-start guide

---

## Core Concepts

### Project (.wvl file)
A project contains a **document** — an ordered list of sentences — plus configuration, language settings, and metadata. Projects are saved as `.wvl` files.

### Sentence
Each sentence has an ID (e.g., `S1`, `S2`) and contains multiple **tiers** and **mappings**. A sentence is the atomic unit of the weave.

### Tiers
A tier is a version of the sentence text at a particular complexity/language level. Each sentence requires **5 tiers** for a complete weave:

| Tier | ID | Language | Description |
|------|----|----------|-------------|
| **Base** | `base` | Source (en) | Original source text, as-is |
| **Advanced Target** | `advanced_target` | Target (es) | Full literary translation |
| **Moderate Target** | `moderate_target` | Target (es) | Simplified translation (shorter sentences, simpler vocab) |
| **Basic Target** | `basic_target` | Target (es) | Maximally simplified translation |
| **Basic Base** | `basic_base` | Source (en) | Simplified source English (matches basic_target word-for-word) |

### Mappings (Diglot)
Two word-level mappings connect the basic tiers:

| Mapping | Direction | Purpose |
|---------|-----------|---------|
| **Forward Diglot** | basic_base → basic_target | Maps each English word to its Spanish equivalent |
| **Inverse Diglot** | basic_target → basic_base | Maps each Spanish word back to English |

These mappings are what the weave algorithm uses to substitute individual words as the user level increases.

### Tier States
Each tier has a state:

| State | Meaning |
|-------|---------|
| **Valid** | Content is finalized and approved |
| **Dirty** | Content was edited but not yet validated (needs lemmatization) |
| **Stale** | A parent tier was modified, so this tier may be out of date |
| **Broken** | Content failed validation |

A sentence is **weave-ready** when all 5 tiers are Valid and both mappings exist.

---

## Tier Dependency Chain

Tiers are generated top-down. Editing a parent makes downstream tiers Stale.

```
base (source English)
├── advanced_target    [Translation: en → es, literary]
│   └── moderate_target    [Simplification: es → simpler es]
│       └── basic_target   [Simplification: es → simplest es]
└── basic_base             [Simplification: en → basic en]
    └── basic_target       [also depends on basic_base for mapping]
```

### Generation Stages (LLM)

These are the named stages used with `run generate <Stage> <start> <end>`:

| Stage Name | Source Tier | Target Tier | Operation |
|------------|-------------|-------------|-----------|
| `GenerateAdvancedTarget` | base | advanced_target | Translate to literary target language |
| `GenerateModerateTarget` | advanced_target | moderate_target | Simplify target (segment-level) |
| `GenerateBasicTarget` | moderate_target | basic_target | Simplify to basic target |
| `GenerateBasicBase` | base | basic_base | Simplify source to basic English |
| `GeneratePhraseMap` | basic_base | mapping: bb→bt | Generate forward diglot word map |
| `GenerateInversePhraseMap` | basic_target | mapping: bt→bb | Generate inverse diglot word map |

**Auto-mapping:** When generating `BasicBase` or `BasicTarget`, the engine automatically queues `GeneratePhraseMap` and `GenerateInversePhraseMap` after each batch.

### Typical Full Generation Order

For a new document with only `base` populated:

1. `run generate GenerateAdvancedTarget 1 23`
2. `run generate GenerateModerateTarget 1 23`
3. `run generate GenerateBasicBase 1 23`     ← also auto-generates mappings
4. `run generate GenerateBasicTarget 1 23`   ← also auto-generates mappings
5. Review and validate all tiers
6. `weave status` → should report Ready

---

## Segments and Tokens

### Segments (Advanced / Moderate tiers)
The advanced and moderate tiers contain **segments** — chunks of text (roughly clause or phrase level). Each segment has an ID, text, and lemma data. Segments allow fine-grained editing:

```
edit seg 7 mod S7_S2 "El pájaro voló lejos."
lemmatize 7 mod
validate 7 mod
```

### Tokens (Basic tiers)
The basic tiers use a flat **token stream** — individual words. The mapping table shows each word with its translation:

```
Word 0: "The"    → "Los"
Word 1: "Grimm"  → "Grimm"  [PROPER]
Word 2: "Story"  → "Cuentos"
Word 3: "Books"  → "Libros"
```

Token-level commands: `split`, `merge`, `insert`, `delete`, `edit_b`, `edit_target`.

---

## Weave Output

The weave algorithm combines all tiers and mappings to produce level-graded text files:

- **UL1.txt** — Nearly all source language (basic English)
- **UL11.txt** — ~25% target function words substituted
- **UL20.txt** — ~55% target, verb conjugations appear
- **UL34.txt** — Fully fluent target language (advanced tier)

The algorithm decides which words to substitute at each level based on the level map (`.lm` file) and word frequency data.

```powershell
.\copilot.ps1 cmd "set output_dir ./weave_output"
.\copilot.ps1 cmd "generate_weave all"     # All levels
.\copilot.ps1 cmd "generate_weave 11"      # Just UL11
```

---

## Weave Readiness Checklist

A sentence is weave-ready when:

- [x] `base` — Valid (populated from source import)
- [x] `advanced_target` — Valid (translated)
- [x] `moderate_target` — Valid (simplified from advanced)
- [x] `basic_target` — Valid (simplified from moderate)
- [x] `basic_base` — Valid (simplified from base)
- [x] Forward diglot mapping (basic_base → basic_target) — exists with entries
- [x] Inverse diglot mapping (basic_target → basic_base) — exists with entries

Quick checks:
```powershell
.\copilot.ps1 cmd "weave status"                   # Overall readiness
.\copilot.ps1 cmd "report sentences incomplete"    # List problem sentences
.\copilot.ps1 cmd "report sentence 7"              # Drill into one
```

---

## Proper Noun Lemmas

Proper nouns (names, places) are marked in the diglot mapping with `[PROPER]`. They are treated as "always known" by the weave algorithm — they don't count toward the substitution budget.

Per-sentence proper noun lemma lists can be managed:
```
list pn_lemmas 7
add pn_lemma 7 Grimm
rm pn_lemma 7 Grimm
```

---

## Calibration and Level Maps

- `calibrate [max_level]` — Runs calibration on the loaded document to produce a level map
- `import level_map <path>` — Loads an existing `.lm` level map
- `export level_map [path]` — Exports the current level map

The level map is required for weave generation. It defines which words appear at which user level.
