# Thoughts on the Leveling System

**Original notes:** September 2025  
**Updated:** February 2026

---

## 1. Early Leveling Design (September 2025)

I will have this updated at the point we add Japanese. For now let's discuss a leveling system. You can see the `es_master_level_map.txt` is a proposed system.

The problem is that English will hang on quite far into even the higher levels. I want to prioritize graduating English. My plan is to put some more thought into the inverse diglot prompt to encourage even more simplistic phrasing using low vocabulary with even the possibility of retries when the lemmas returned are higher than some limit. After the retry, the phrases with the lowest high lemma is chosen. I may not do this at all however.

What I do plan is once we are at level 27 (2119 vocab) we should have all the basic core words. This corresponds to these stats in the profiler for two test books, one with advanced archaic Spanish:

```
--- Analysis for Book Instance: quijote_test_L211_211.txt (at 2025-09-01 17:39:45) ---
  Output Word Count Summary:
    Total Target Words:    617
    Total Base Words:      124
    -------------------------
    Total Output Words:    741
    Base Lang Pct:       16.73%

  Sentence Level Distribution:
    L0 Advanced Weave:    44 sentences ( 88.00%)
    L1 Simple Hybrid:      6 sentences ( 12.00%)

  Segment Type Distribution (Total: 94 segments):
    Adv. Target Segments:     16 segments ( 17.02%)
    Simpler Target Segs:      19 segments ( 20.21%)
    Inv. Diglot Segments:     53 segments ( 56.38%)
    Base Diglot Segments:      6 segments (  6.38%)

  Final Profile State:
    Activated Lemmas: 2110
----------------------------------------------------------------------
```

```
--- Analysis for Book Instance: test_L211_211.txt (at 2025-09-01 17:39:45) ---
  Output Word Count Summary:
    Total Target Words:    497
    Total Base Words:       98
    -------------------------
    Total Output Words:    595
    Base Lang Pct:       16.47%

  Sentence Level Distribution:
    L0 Advanced Weave:    14 sentences ( 63.64%)
    L1 Simple Hybrid:      8 sentences ( 36.36%)

  Segment Type Distribution (Total: 74 segments):
    Adv. Target Segments:     30 segments ( 40.54%)
    Simpler Target Segs:      10 segments ( 13.51%)
    Inv. Diglot Segments:     26 segments ( 35.14%)
    Base Diglot Segments:      8 segments ( 10.81%)

  Final Profile State:
    Activated Lemmas: 2110
```

## 2. Forced English (FE) vs Full (F) Level Split

It is at this point the engine can have a distinction between two levels. Forced English level and full level. The full level is what we are doing now where a single level is used throughout the tiers. Our user level is what we use that is user facing that is meaningful. 

The intent is that each level from the user's point of view represents 2% increase in new Spanish by density on the page. If the user is at level 18 for instance and can comprehend 95% of the words they are coming across, then to move up to level 19 would mean they now comprehend 93%

We use the FE (forced English) and F-level (Full) together in the system to achieve the user level system.

If the F-level is at 27 which is the 2119 vocabulary, up to that point the engine behaves exactly as it does now. After that point, the F-level freezes for a time. We want to focus on increasing difficulty be gradually turning off English substitutes only but not advancing any more from Simper Spanish (Moderate) to Advanced. So the F-level is still used to determine whether or not to fall back from an advanced segment to the simpler segment in L0. This F-level of 27 would freeze for a few user levels until all Sentence are at L0 no inverse diglot.

The way this would be achieved is at this level 27 when we have the core vocabulary, the F-level locks and the FE-level continues on. In a sense these two are the same thing until this point. The FE-level needs to increase to some determined amount to increase the difficulty by 2% for each user level. At some point the FE-level will be so high that it matches the vocabulary of the frequency list and can go no higher. It is at this point that no English will be expressing and we are generating 100% moderate Spanish with some Advanced which were already starting to express by a vocabulary of ~2k.

It is at this point that we begin advancing the F-level again which controls the expression of the Advanced segments leading to a 2% increase in difficulty until we exhaust the full frequency list.

For each book this would be different so we would base our highest user level on the most advanced book.

Now I say all that to ask the question, what is the current literature on measuring text reading level. Are there formulas based on the variety of words? Others have already done research on this. 

## 3. The Cross-Book Consistency Problem

Currently my concern is that Grimm's Fairy tales level 30 for example would still feel easier than War and Peace level 30 just because even though they both have the same activated lemmas for both F-level and FE-level, War and Peace uses a higher density of the higher words that are available to both books.

At some point for simple children's books increasing the level would have no effect since their headroom is saturated. I have a plan to find the saturation point in a book and have that level saved into the JSON file as meta data called "natural_level". This tells the corpus generator to not allow an attempt to output at a higher level since it would not produce any meaningful difference.

The natural level is actually one level below what the true max level of the book is since every book produced will be set to stretch to the next level gradually throughout the book. (ie. A level 8 book ends on level 9). So the natural level must be one level below where the book would be 100% expressing the Advanced Spanish tier.

"Don Quijote" likely would have a very high natural level. The spread between the simplest of book's natural level and the most advanced is likely ~3 or 4 levels due to the exponential nature of the increase in vocabulary to increase the new word density by 2%.

One possible way to think about levels, if we could prove a high correlation between this system and the 2% increase in difficulty, would be to measure the average rank of the lemmas expressed in a particular instance of a book, a particular output at a set level. When we increase the level of the book, the average word when the ranks are all summed and divided by the number of words would increase. If we know that a movement of this average by 1% for instance is equal to a 2% increase in difficulty, then we have a way to determine a books level. 

Once we have this information for a book, a series of corpus runs can be ran as a simulation to find the points at which it hits various user level and these can be saved as a table in the json file.

It may be that for instance "Grimm's fairy tales" level F-level 30 is equivalent to "Don Quijote" F-level 28. We may even just state the precise vocabulary needed in the book to achieve the final user level.

In this way regardless of the difficulty of the original work, a user level n feels the same for all works set to that level.

---

## 4. Resolution: The AVD Calibration System (Implemented)

The concerns in §3 were addressed by the AVD (Average Vocabulary Density) calibration system built in late 2025. The system has two phases:

1. **AVD Hunter** (`avd_hunter.rs`) — Runs once against a canonical reference text. For each user level 1…N, binary-searches for the vocabulary size where exactly 2% of running text consists of new lemmas. Produces a universal master AVD scale CSV.

2. **Per-Book Calibrator** (`calibrator.rs`) — Runs per book. Pre-computes AVD scores for each tier × vocabulary size, then uses an L-Level state machine with phased catchup (BasMod → ModAdv → AdvOnly → Complete) to find the optimal VLevelRecipe at each user level that matches the universal target AVD.

The key insight: every book targets the **same AVD score** at every user level via the universal exponential formula `AVD(L) = exp((L - 0.02) / 4.15) - 1`. The calibrator finds *different* vocabulary recipes per book to hit those same targets. War and Peace needs a more restrictive recipe than Grimm's to achieve the same AVD at level 14 — because its source vocabulary is richer.

The "natural level" concept from §3 is implemented as the `is_maxed_out` flag: once all three tiers (Basic, Moderate, Advanced) are exhausted, remaining levels are clamped at the peak AVD.

The Metamorphosis calibration was reviewed manually and confirmed via user experience testing — levels feel incrementally harder.

---

## 5. Future Hypothesis: Curve Library via Peak AVD Sampling (February 2026)

### 5.1 The Hypothesis

A book's **peak AVD score** (the AVD at which the calibrator's `is_maxed_out` fires) may be estimable from a small sample of sentences — perhaps as few as 10. If so, the expensive per-book calibration could be replaced by a **curve library** lookup for most use cases.

The intuition: even a handful of sentences from Les Misérables will contain words with frequency ranks far higher than anything in a Grimm fairy tale. The tail-weighted AVD formula `(P85_Rank + 2 * P95_Rank) / 3` is designed to capture exactly this signal — the hardest words the learner will encounter — and that should stabilize quickly.

### 5.2 Proposed Experiment

1. Select several books spanning a wide difficulty range (e.g., Grimm's Fairy Tales, The Metamorphosis, Don Quijote, War and Peace).
2. For each book, take 5–10 random 10-sentence windows.
3. Run the calibrator on each window.
4. Compare the peak AVD from each window against the full-book peak AVD.
5. If variance is small (e.g., within 5–10%), the hypothesis holds.

### 5.3 The Curve Library

If confirmed, the endgame is:

1. **Build the library:** Process N books fully (expensive, one-time). For each, store peak AVD + the calibrated curriculum curve.
2. **Index by peak AVD:** Each processed book becomes a reference point on the difficulty spectrum.
3. **Fast-path for new books:** Sample a few sentences → compute peak AVD → find the nearest match in the library → use that book's pre-computed curve as a proxy.

The calibrator's phased state machine produces curves shaped by two things: the book's peak AVD and its vocabulary distribution across tiers. If books with similar peaks also have similar tier distributions — which is likely, since literary complexity correlates across tiers — then curves from the library should be nearly interchangeable.

### 5.4 Studio Application

This directly solves the Studio UX problem: when a content creator imports a short text (a single fairy tale, a newspaper article, a song), the system can:

1. Quickly estimate the text's difficulty via a small-sample AVD measurement.
2. Pull the nearest pre-computed curve from the library.
3. Skip the full calibration process entirely, producing a reasonable curriculum map in seconds instead of minutes.

This is a future feature — not needed for the current pipeline or testing plan — but the experiment in §5.2 would validate whether the approach is viable.