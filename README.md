# WeaveLang

**Automated bilingual reading material generator using the Diglot Weave method — powered by LLMs and built in Rust.**

WeaveLang takes literary source texts and produces graded, bilingual reading material that smoothly transitions a learner from their native language into a target language. Words and phrases are progressively swapped from the base language (e.g. English) to the target language (e.g. Spanish) as the reader's vocabulary grows, so that by the end of a book the reader is comprehending full target-language prose — without ever needing a dictionary.

**See it in action:** the [WeaveLang YouTube channel](https://www.youtube.com/channel/UCSQM9TJ_ol9fIj-VmOhLPlQ) has sample audiovisual content generated entirely by WeaveLang.

---

## What Is the Diglot Weave?

The **Diglot Weave** (from Greek *di* "two" + *glot* "tongue") is a technique for second-language vocabulary acquisition first described by Robbins Burling in 1968. The core idea is simple: embed target-language words into a base-language text, beginning with the most frequent and contextually transparent words, and progressively increase the proportion of target-language content as the reader acquires vocabulary.

A learner at an early level might see:

> The **gato** (cat) sat on the **mesa** (table).

A few chapters later:

> El **gato** se sentó en la mesa **cerca de** (near) la ventana.

And eventually:

> El gato se sentó en la mesa cerca de la ventana y observó la lluvia.

Research consistently shows that Diglot Weave techniques produce significant gains in vocabulary acquisition and retention compared to traditional methods (see [Research References](#research-references) below).

## Key Features

- **Four-tier simplification gradient** — Source texts are rendered at four complexity levels (Advanced, Moderate, Basic, Simple), each calibrated to a target vocabulary density. Learners transition between *adjacent* tiers only, preventing jarring stylistic jumps.
- **AVD scoring** — A Tail-Weighted Average Vocabulary Density metric (based on word-frequency percentiles) objectively measures text difficulty and drives the levelling system.
- **Per-word and per-phrase mapping** — LLM-generated bilingual mappings at the token level let the weaver swap individual words and multi-word expressions in place.
- **LLM-powered pipeline** — Uses Claude and Gemini models to generate simplified tiers, phrase mappings, and lemmatisation, with automatic validation and retry logic.
- **Interactive GUI** — An [egui](https://github.com/emilk/egui)-based desktop application for reviewing, editing, and generating weaves sentence by sentence.
- **CLI & batch modes** — Run calibration, generation, and level-hunting from the command line for bulk processing.
- **Multi-language support** — Currently supports English → Spanish and English → Japanese, with a language manifest that makes adding new pairs straightforward.

## Architecture

WeaveLang has two components:

| Component | Language | Status |
|-----------|----------|--------|
| **Rust application** (GUI + weaving engine + CLI) | Rust / egui | **Active — main line** |
| **Python data-generation pipeline** (`llm2books/`) | Python | **Deprecated — scheduled for removal** |

The **Rust application** is the primary codebase. It provides the interactive GUI, the weaving/fusion engine, AVD scoring and calibration, and a CLI for batch operations. It communicates with LLM APIs directly and can also bridge to the Python pipeline over HTTP (port 3030) when needed for NLP tasks that still live in Python.

The **Python pipeline** (`llm2books/`) was the original prototype used for multi-stage LLM orchestration, spaCy-based tokenisation, and Stanza segmentation. It is effectively deprecated: all critical functionality is being ported to the Rust side, and the Python code will be removed from the repository in a future release.

## Getting Started

### Prerequisites

- **Rust** toolchain (stable, 2021 edition)
- API keys for **Google Gemini** and/or **Anthropic Claude** (stored in OS credential vault)

### Build

```bash
cargo build --release
```

### Configure

Copy `config.toml.example` to `config.toml` and set your content project directory and model preferences. API keys are stored in your operating system's credential vault — not in config files.

### Run

```bash
# Interactive GUI
cargo run -- gui

# Batch weave generation
cargo run -- generate --tool-root-dir <path> -s <sequence_file> \
    --input-json-dir <json_dir> --tts-output-dir <audio_dir> \
    --profiles-dir <profiles_dir>

# AVD calibration
cargo run -- calibrate --book-json <path> --output-path <path> \
    --master-avd-scale <path>

# Level optimisation (hunt)
cargo run -- hunt --canonical-json <path> --max-user-levels 50 \
    --output-csv <path>
```

## How It Works

1. **Segment** — Source text is split into sentences.
2. **Simplify** — Each sentence is rendered at four complexity tiers by an LLM, targeting specific vocabulary-density bands.
3. **Map** — Forward and inverse phrase mappings are generated between base and target language for each tier.
4. **Score** — Every tier variant is scored with the AVD metric so it can be assigned to the right learner level.
5. **Weave** — At read time, the engine selects which words to show in the target language based on the learner's current level and accumulated vocabulary, drawing from adjacent tiers to keep the reading experience smooth.

## Supported Languages

| Base Language | Target Language | Status |
|---------------|-----------------|--------|
| English | Spanish | Supported |
| English | Japanese | Work Started |

New language pairs can be added by defining entries in `assets/languages.toml` and providing prompt overrides in `assets/prompts/<from>-<to>/`.

## Research References

The Diglot Weave technique has a body of academic research supporting its effectiveness for vocabulary acquisition:

- **Burling, R.** (1968). *Some outlandish proposals for the teaching of foreign languages*. Language Learning, 18(1–2), 61–75. — The foundational paper proposing the Diglot Weave concept.
- **Nemati, A. & Maleki, E.** (2014). *The effect of teaching vocabulary through the Diglot–Weave technique on vocabulary learning of Iranian high school students*. Procedia — Social and Behavioral Sciences, 98, 1340–1345. [doi:10.1016/j.sbspro.2014.03.551](https://doi.org/10.1016/j.sbspro.2014.03.551) — Found significant vocabulary gains using Diglot Weave over control groups.
- **Katemba, C. V. & Sitompul, N. A.** (2018). *A comparison of using Diglot Weave technique and Student Team Achievement Division on student vocabulary achievement*. Human Behaviour, Development and Society, 17. — Demonstrated Diglot Weave outperformed STAD for vocabulary retention.
- **Simanjuntak, O. V. & Simanjuntak, D. C.** (2018). *Students' vocabulary knowledge: Comparative study enhancing between Semantic Mapping and Diglot Weave techniques*. Acuity: Journal of English Language Pedagogy, Literature and Culture, 3(2), 85–97. [doi:10.35974/acuity.v3i2.671](https://doi.org/10.35974/acuity.v3i2.671) — Found Diglot Weave produced higher vocabulary gains than Semantic Mapping.
- **Huong, N. T. T. & Huong, T. T. M.** (2021). *Improving students' vocabulary using the Diglot-Weave technique*. DLU Journal of Science. — Confirmed the technique's effectiveness in Vietnamese EFL classrooms.

For a broader overview of the research landscape, search [Google Scholar for "Diglot Weave"](https://scholar.google.com/scholar?q=%22diglot+weave%22+vocabulary+language+learning).

## Contributing

Contributions are welcome. Please open an issue to discuss proposed changes before submitting a pull request.

## License

[MIT](LICENSE.md) — Copyright (c) 2025–2026 Bill Branch.

---

<sub>**Keywords:** diglot weave, bilingual reading, graded reader, second language acquisition, SLA, vocabulary acquisition, L2 reading, language learning software, comprehensible input, bilingual text generator, code-switching, extensive reading, interlinear text, parallel text, LLM language learning, AI language learning, spaced repetition reading, Krashen input hypothesis, natural approach, English to Spanish, English to Japanese</sub>
