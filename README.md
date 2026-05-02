# Search Engine in Rust

A search engine being built from scratch in Rust, following Manning's *Introduction to Information Retrieval* textbook chapter by chapter. No Lucene, no Elasticsearch, no libraries doing the interesting work. Indexing, compression, spell correction, ranking, tiered retrieval, proximity scoring — all written by hand.

Currently indexes 11,314 documents from the 20 Newsgroups dataset. Returns ranked, proximity-boosted results with spell correction in single-digit milliseconds for most queries.

## What it does

```
Enter your search query:
united states of america

→ TIER 0: 2 candidates after intersection
→ Doc 9059 (1,835 lines, firearms archive): tf-idf score 0.0926
→ Doc 8802 (13 lines, Cold War post): not yet scored
→ TIER 1: 0 candidates
→ TIER 2: 1 new candidate (doc 8802)
→ Doc 8802 has the literal phrase "United States of America" in line 1
→ ω = 4 (smallest window containing all 4 terms)
→ Boost: 1 + 4/4 = 2.0
→ Doc 8802 final score: 0.6723

Top result:  doc 8802 — score 0.6723  (13-line focused post, ω=4)
Second:      doc 9059 — score 0.1581  (1,835-line archive, ω=40, terms scattered)

Ratio: 4.25x. Proximity scoring amplified the cosine-norm finding.
Total query time: 49.6 ms.
```

## Architecture

**Index construction.** Documents are processed in blocks of 4,000. Each block is flushed to disk when memory fills, then all blocks are merged via a streaming k-way merge into a single contiguous index file. Dictionary stays in RAM, postings live on disk.

**Custom serialization.** No bincode. Hand-rolled VByte encoder/decoder that gap-encodes sorted doc IDs and positions. Index size dropped from 34.8MB to 8.3MB — 76% reduction. Six serialization call sites, ~40 lines of Rust.

**Tiered indexes.** Each term's posting list partitioned by term frequency into three tiers (T1 = high-tf docs, T2 = medium, T3 = low). Single-pass partition during merge — three jobs (doc_freq, tier split, doc_vec_len contribution) ride one pass over the assembled merged_postings with zero clones. Thresholds tuned from the 1.7M-pair tf distribution: T1=5, T2=2, putting 4% of postings in tier 1 and 9% in tier 2.

**Tier-fallback retrieval.** Query runs against tier 1 first. If results < K, fall back to tier 2; still < K, fall back to tier 3. Cross-tier dedup via HashSet. Final sort across all tiers — load-bearing, not defensive: a tier-2 short doc can outscore a tier-0 long doc once cosine normalization divides by document length.

**Spell correction.** Trigram index → Jaccard similarity filter → Levenshtein edit distance ranking. Handles multiple misspelled words in a single query.

**TF-IDF ranking with cosine normalization.** Document vector lengths precomputed at index time. Query-time score: sum of tf × idf across query terms, divided by precomputed length. Rewards topical focus over document length.

**Query-term proximity scoring (Ch 7.2.2).** For each candidate doc, compute ω = the smallest window containing at least one position from each query term. ω is computed in O(total positions) via a k-pointer min-advance scan. The doc's score is then multiplied by `1 + k/ω`, where k = number of query terms. ω = k recovers strict phrase matching as the special case where boost = 2.0. Single-term queries skip the boost via a `k >= 2` guard.

## Query pipeline

```
user query
  → spell correction (trigram + Jaccard + edit distance)
  → for tier in 0..3:
       → postings retrieval from disk (seek by offset, read exact bytes per tier)
       → sorted intersection (smallest posting list first)
       → cross-tier dedup
       → for each candidate doc:
              → tf-idf score using corpus-wide doc_freq from term_index
              → cosine normalization (divide by precomputed doc_vec_len)
              → proximity boost: doc_score *= 1 + k / window_calc(positions)
       → if cumulative results >= K, break
  → final sort across all tiers (load-bearing — handles cross-tier ranking inversions)
  → top-K results with file paths
```

## Project structure

```
src/
├── main.rs              — orchestration, query loop, tier-fallback loop
├── traverse.rs           — recursive document ingestion
├── cleanup.rs            — tokenization (alphanumeric, lowercase)
├── encode_decode.rs      — VByte encoding/decoding, serialization
├── block_merge.rs        — BSBI streaming k-way merge, single-pass tier partition,
│                            doc_vec_len precomputation, TermEntry construction
├── get_posting.rs        — per-tier disk reads by offset
├── intersect.rs          — two-pointer sorted intersection, smallest-first ordering
├── spell_check.rs        — trigram + Jaccard + Levenshtein pipeline
├── three_gram_index.rs   — trigram index construction
├── tf_idf_index.rs       — tf-idf, cosine norm, omega_calc, window_calc,
│                            boost_calc, rank_results
├── phrase_check.rs       — retired (mod declaration removed from main.rs);
│                            kept on disk as historical reference. Strict phrase
│                            matching is now the ω = k special case of proximity scoring.
```

## Chapters implemented

| Ch    | Topic                              | What was built                                                                 |
|-------|------------------------------------|--------------------------------------------------------------------------------|
| 1     | Boolean retrieval                  | Inverted index, postings intersection                                          |
| 2     | Vocabulary and postings            | Positional index, phrase queries                                               |
| 3     | Tolerant retrieval                 | Trigram index, spell correction, edit distance                                 |
| 4     | Index construction                 | Block-based construction, streaming k-way merge, RAM dictionary                |
| 5     | Index compression                  | VByte encoding, gap encoding, custom serialization (76% smaller)               |
| 6     | Scoring and ranking                | tf-idf, cosine normalization, precomputed doc lengths                          |
| 7.2.1 | Tiered indexes                     | Histogram-tuned thresholds, single-pass tier partition, tier-fallback loop     |
| 7.2.2 | Query-term proximity               | ω computation via k-pointer min-advance, multiplicative boost `1 + k/ω`        |

**Skipped intentionally:** 7.1.3 (champion lists — generalized by tiered indexes), 7.1.6 (cluster pruning — out of modern lineage), 7.2.3 / 7.2.4 / 7.3 / 7.4 (synthesis sections, not in trajectory).

**Deferred:** 7.1.5 (impact ordering) — composes with BM25 as the scoring function, implement after Ch 11.

## Performance

**Index:** 11,314 documents, 138,743 unique terms, 8.3MB compressed index on disk. Tier 1 holds 4% of postings, tier 2 holds 9%, tier 3 holds 87%.

**Query latency** on the 12-query regression suite (debug build):

| Query type                         | Latency        | Notes                                              |
|------------------------------------|----------------|----------------------------------------------------|
| Single-term (`israeli`, `nasa`)    | 1.3 – 3.9 ms   | Tier 0 alone fills K=10                            |
| Multi-term phrase (`gun control`)  | 4.4 – 5.7 ms   | Tier 0 ω=2, boost=2.0                              |
| Multi-term scattered               | 4.0 – 9.4 ms   | Falls through to tier 2, smaller boost             |
| 4-term with stop word              | **49.6 ms**    | Reading "of" posting list (60KB, 3,231 docs)       |

The 49.6 ms outlier on `united states of america` is driven entirely by reading the posting list for "of." That term contributes near-zero to ranking (idf ≈ 0.12) but pays full I/O cost. BM25's aggressive idf weighting will let stop-word skipping become safe — filed for the next release.

**Ranking quality.** On the 20 Newsgroups corpus, the engine demonstrates that **tf-idf + cosine normalization + tiered fallback + proximity scoring compound** to surface focused short documents over long documents that mention query terms in passing:

```
Query: "united states of america"
Doc 8802 (13 lines, "Cold War: Who REALLY Won?")  →  score 0.6723   (rank 1)
Doc 9059 (1,835 lines, firearms archive)           →  score 0.1581   (rank 2)
Ratio: 4.25x
```

Pre-proximity (Release 0.6), the same ratio was 2.3x via cosine normalization alone. Adding proximity scoring nearly doubled the gap because doc 8802 contains the literal phrase tightly (ω=4) and doc 9059 has the same words scattered across hundreds of positions (ω=40).

## Recovery on multi-word queries

Release 0.6 used a strict phrase filter that killed valid AND-matches when query terms didn't appear in literal word order. Five queries on the test suite returned zero results despite having 13–22 candidate documents.

Release 0.7 (proximity scoring) replaces that binary filter with a continuous boost. Result on the canonical case:

```
Query: "israeli palestinian"
Release 0.6:  22 candidates, 0 returned (phrase filter killed all)
Release 0.7:  22 candidates, 10 returned (all from talk.politics.mideast,
                                          ranked correctly, top hit doc 359)
```

All five previously-zero queries recover under proximity scoring.

## What's next

- **Cranfield benchmark** (1,398 docs, 225 expert-judged queries since the 1960s). Replace 12-query qualitative tests with MAP / P@10 / NDCG@10 against real relevance judgments.
- **BM25 (Ch 11.4.3).** Replaces tf-idf + cosine norm with one formula. Better tf saturation, better length normalization. Cuts the stop-word latency problem.
- **Threshold experiment.** (3, 1) vs (5, 2) on the regression suite.
- **Then production lineage:** impact-ordered postings → WAND → block-max WAND → dense retrieval / hybrid search.

## Build and run

```bash
cargo build --release
cargo run --release
```

Expects the 20 Newsgroups training set at the path specified in `main.rs`. Update the `root` variable to point to your local copy.

## Articles

Each release is documented in long form on [krithik.xyz](https://krithik.xyz):

1. Inverted positional index + two-pointer intersection
2. Phrase search + spell correction
3. Reading from disk
4. VByte compression
5. TF-IDF + cosine normalization
6. Tiered indexes
7. Proximity scoring
