# Search Engine in Rust

A search engine being built from scratch in Rust, following Manning's *Introduction to Information Retrieval* textbook chapter by chapter. No Lucene, no Elasticsearch, no libraries doing the interesting work. Everything — indexing, compression, spell correction, ranking — is written by hand.

Currently indexes 11,314 documents from the 20 Newsgroups dataset. Returns ranked, phrase-matched results with spell correction in under a second.

## What it does

```
Enter your search query:
Uniteed Sttates of Ameeriica

→ Corrected to: united states of america
→ 37 documents contain all four terms
→ 14 contain the exact phrase in sequence
→ Ranked by tf-idf with cosine normalization
→ Top result: a 13-line post focused entirely on the USA
→ Bottom result: an 1,835-line firearms archive that mentions it in passing
```

## Architecture

**Index construction** — documents are processed in blocks of 4,000. Each block is flushed to disk when memory fills, then all blocks are merged into a single contiguous index file. Dictionary stays in RAM, postings live on disk.

**Custom serialization** — no bincode. I wrote my own VByte encoder/decoder that gap-encodes sorted doc IDs and positions. Index size dropped from 34.8MB to 8.3MB (76% reduction). Six serialization call sites, all replaced with ~40 lines of Rust.

**Spell correction** — trigram index → Jaccard similarity filter → Levenshtein edit distance ranking. Handles multiple misspelled words in a single query.

**Phrase matching** — positional index tracks where each term appears in each document. Phrase filtering verifies consecutive positions across all query terms.

**Ranked retrieval** — tf-idf scoring with cosine normalization. Document vector lengths are precomputed during merge. At query time: compute raw score, divide by precomputed length. Rewards topical focus over document length.

## Query pipeline

```
user query
  → spell correction (trigram + Jaccard + edit distance)
  → postings retrieval from disk (seek by offset, read exact bytes)
  → sorted intersection (smallest posting list first)
  → phrase filtering (positional adjacency check)
  → tf-idf ranking with cosine normalization
  → ranked results with file paths
```

## Project structure

```
src/
├── main.rs              — orchestration, query loop
├── traverse.rs           — recursive document ingestion
├── cleanup.rs            — tokenization (alphanumeric, lowercase)
├── encode_decode.rs      — VByte encoding/decoding, serialization
├── block_merge.rs        — BSBI block merge, vector length precomputation
├── get_posting.rs        — disk reads by offset
├── intersect.rs          — two-pointer sorted intersection
├── phrase_check.rs       — positional phrase verification
├── spell_check.rs        — trigram + Jaccard + Levenshtein pipeline
├── three_gram_index.rs   — trigram index construction
└── tf_idf_index.rs       — scoring and ranking
```

## Chapters implemented

| Ch | Topic | What I built |
|----|-------|-------------|
| 1 | Boolean retrieval | Inverted index, postings intersection |
| 2 | Vocabulary and postings | Positional index, phrase queries |
| 3 | Dictionaries and tolerant retrieval | Trigram index, spell correction, edit distance |
| 4 | Index construction | Block-based construction, merge to disk, RAM dictionary |
| 5 | Index compression | VByte encoding, gap encoding, custom serialization |
| 6 | Scoring and ranking | tf-idf, cosine normalization, precomputed doc lengths |

## Performance

**Index:** 11,314 documents, 138,743 unique terms, 8.3MB compressed index on disk.

**Query** ("United States of America" with two misspelled words):

| Step | Time |
|------|------|
| Spell correction | ~600ms |
| Postings retrieval | ~25ms |
| Intersection | ~250µs |
| Phrase filtering | ~17ms |
| Ranking | <1ms |
| **Total** | **~665ms** |

## What's next

BM25 scoring (chapter 11). k-way external merge to fix the current all-blocks-in-RAM merge. Eventually: dense retrieval, learned ranking, and a real corpus beyond 20 Newsgroups.

## Build and run

```bash
cargo build --release
cargo run --release
```

Expects the 20 Newsgroups training set at the path specified in `main.rs`. Update the `root` variable to point to your local copy.
