Study format (SF) v2: aligned-audio interlacing for deep study flow.

Goal
- Keep sentence-to-sentence flow (not one sentence at a time).
- Avoid expensive/fragile re-synthesis of repeated multilingual text.
- Reuse already-generated level audio and concatenate it into SF output.

Output naming
- Text reference file: <stem>ULsf.txt
- Audio output file: <stem>ULsf.wav (or project output format)

Core design shift
- Old model: generate new SF text per sentence and synthesize directly.
- New model: SF is primarily an audio assembly product built by interlacing existing level audio chunks.
- Requirement: all levels used by SF must already exist as chapter (or whole_book) audio outputs.

Why this change
- Direct SF synthesis has quality failures with repeated text (loops/silence/hallucinated fills).
- SF has much more text than single-level outputs and is costlier to generate.
- Existing per-level chapter audio is already chunked and validated in normal workflow.

Level selection rules
- SF uses manifest-defined level mapping as the single source of truth.
- The default manifest should start at level 16 and jump by 3 through level 34.
- Authors are expected to edit this mapping to match what they actually produced.
- No per-level text diffing is required in SF v2.
- Frontier must remain OFF for level generation used by SF.

Sentence grouping / chunking
- SF output should be assembled in groups of source sentences (chunked flow), not sentence-by-sentence alternation.
- Chunk boundaries are derived from source text using the same chunking algorithm used for TTS preparation.
- These boundaries must be persisted and reused across all selected levels.

Alignment index
- A chunk alignment JSON must be stored in the chapter (or whole_book) directory.
- Canonical filename: _sf_alignment_map.json
- Generated automatically on the first TTS audio run for that chapter/book when it does not yet exist.
- All subsequent level runs use the same file — never recalculated unless explicitly forced.
- This file defines canonical sentence-aligned chunk boundaries and metadata used for every selected level.
- Boundaries are calculated once from source sentences, then reused for all levels.
- It must include:
	- chunk_max_chars used to create boundaries
	- sentence-range coverage per chunk
	- chunk ordering/index
	- a hash/signature of source text used for alignment generation
	- total sentence count

Per-level chunk metadata
- Every level chunk-audio directory should include a small metadata file.
- Canonical filename: _sf_chunk_meta.json
- Derived from the canonical _sf_alignment_map.json at the time the level audio is produced.
- It records:
	- alignment map signature/hash
	- total sentence count
	- chunk count
	- chunk_max_chars
- SF preflight must require these to match the canonical alignment index exactly.

Suggested schema (example)
{
	"version": 1,
	"chunk_max_chars": 500,
	"source_hash": "sha256:...",
	"chunks": [
		{ "index": 0, "start_sentence": 1, "end_sentence": 6 },
		{ "index": 1, "start_sentence": 7, "end_sentence": 12 }
	]
}

Safety and rewrite policy
- If alignment JSON already exists and audio files exist, do not silently rewrite alignment.
- If user requests rewrite with a different chunk_max_chars, emit a warning/error requiring explicit force/confirm behavior.
- Rewriting alignment after audio exists invalidates cross-level chunk compatibility.
- Sentence edits are effectively locked once audio production starts; insertion/deletion changing sentence count invalidates compatibility.

SF assembly pipeline
1. Validate alignment JSON exists and matches source hash.
2. Resolve SF level plan from manifest only.
3. Verify required level audio and chunk metadata exist for all planned levels.
4. For each chunk index, interlace audio in SF order:
	 - source/base chunk first
	 - then each configured level chunk
5. Concatenate interlaced chunks into final SF audio.
6. Produce <stem>ULsf.txt as reference text by concatenating corresponding chunk text in the same order.
7. Insert pacing gaps:
	 - small gap between level variants within a chunk group (default 150 ms)
	 - slightly larger gap between chunk groups (default 350 ms)

Failure policy
- SF assembly is strict: if any configured level/chunk is missing or incompatible, fail the build.
- The fix path is to update the manifest mapping to match actually available audio (or generate missing artifacts).

Important: reference text is informational only in SF v2. Audio is built from existing chunk audio, not newly synthesized from ULsf text.

Handling non-uniform level availability
- Real workflow may use mixed gaps (example: 17, 20, 23, 25, 27...).
- SF supports explicit per-book/per-chapter level plans via manifest mapping.
- There is no CLI start/step fallback for SF output selection in v2.

Manifest concept (example)
[study_format]
enabled = true
source_level = "r"
levels = ["16", "19", "22", "25", "28", "31", "34"]
require_all_audio = true

Compatibility note
- Existing chapter(s) without consistent chunk boundaries cannot be assembled reliably into SF.
- For those, user must regenerate affected level audio using the alignment scheme.

Future considerations
- Support per-chapter override versus whole_book default SF plan.
- Keep defaults at 150 ms (intra-level) and 350 ms (inter-chunk), but allow future overrides in manifest.
- Add optional crossfade mode (off by default).
- Add command-level audit output showing missing levels/chunks before assembly starts.
- For illustration timing, align by chunk groups and summed chunk durations (not sentence-by-sentence).

