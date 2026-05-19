# Search Engine in Rust

A search engine built from scratch in Rust. No Lucene, no Elasticsearch, no libraries doing the interesting work. Indexing, compression, spell correction, BM25F scoring, field extraction, CamelCase tokenization — all written by hand.

Started as a chapter-by-chapter walk through Manning's *Introduction to Information Retrieval* on the 20 Newsgroups toy corpus. Pivoted in May 2026 to a real corpus: **AWS documentation** (14,266 markdown files across 18 services). Same math, real users — developers searching `s3:GetObject`, `arn:aws:iam`, `t2.micro`, and the typos those queries pick up along the way.

## What it does

```
Enter your search query:
RunInstances

→ Spell check: runinstances (no correction needed)
→ Postings retrieved (62 docs in 1.9ms)
→ Intersection: 62 candidates
→ BM25F scoring (4 fields: title, headers, code, body)
→ Top result: API_RunInstances.md (score 10.40)
  Reference doc for the EC2 RunInstances API operation.
→ Total query time: 3.4 ms
```

```
Enter your search query:
arn:aws:s3

→ Tokenizer preserves the ARN intact
→ 205 matching docs
→ Top result: AmazonS3/.../batch-ops-iam-role-policies.md
→ Total query time: 6.1 ms
```

```
Enter your search query:
permssion

→ Spell check: permssion → permission (1,734 docs)
→ Top result: AWSEC2/.../API_CreateNetworkInterfacePermission.md
→ Total query time: 13.4 ms
```

## Architecture

**Index construction.** Documents are processed in blocks of 4,000 using SPIMI (Single-Pass In-Memory Indexing). Each block is flushed to disk when memory fills, then merged via a streaming k-way merge into a single contiguous index file. Dictionary stays in RAM; postings live on disk.

**Custom serialization.** No bincode. Hand-rolled VByte encoder that gap-encodes sorted doc IDs and positions. Six serialization call sites, ~40 lines of Rust, ~76% reduction in index size on the toy corpus.

**Field-keyed posting format.** Postings are nested four deep: `{term → doc_id → field → positions}`. Position gaps reset per field, doc-id gaps persist globally, empty fields cost zero bytes. The `Field` enum uses `#[repr(u8)]` for free serialization and exhaustive matching in the scorer.

**Field extraction.** `field_extract.rs` parses raw markdown into four field strings:
- **title** — first H1
- **headers** — all H2/H3/H4 concatenated
- **code** — fenced blocks + inline spans
- **body** — everything else

Bold/italic markdown emphasis is stripped from non-code fields before tokenization. Code field stays raw because `*` carries meaning there (IAM wildcards, glob patterns).

**Tokenizer.** AWS-aware keep list (`. - _ : / ' @ *`) preserves tokens like `s3:GetObject`, `arn:aws:s3:::my-bucket`, `t2.micro`, `us-east-1`, `s3:Get*`. Asymmetric edge trimming so `cause:` trims to `cause` but `s3:Get*` keeps its wildcard. Curly quotes and Latin ligatures normalize to ASCII. ASCII-only alphanumerics so footnote superscripts like `¹` don't contaminate tokens.

**CamelCase + underscore splitting.** Every compound token is split into pieces and indexed at the same position as the original:

```
RunInstances        → runinstances, run, instances
API_RunInstances    → api_runinstances, api, runinstances, run, instances
getXMLData          → getxmldata, get, xml, data
```

Two boundary rules: lowercase-or-digit → uppercase (`RunInstances` at `n→I`), and uppercase → uppercase-lowercase (`XMLData` at `L→D`, preserves acronyms).

**Spell correction.** Trigram index → doc-frequency filter → Jaccard similarity filter → Levenshtein edit distance ranking. Sorts on `(edit_distance ASC, doc_freq DESC)` so `instnace` resolves to `instance` (~5,000 docs) instead of `instace` (1 doc).

**BM25F ranking.** Field-aware scoring with per-field length normalization and global IDF. Field weighting happens inside the pseudo-tf calculation so BM25's saturation curve applies once across the doc, not per field:

```
IDF(t)      = ln(1 + (N − df + 0.5) / (df + 0.5))
norm_tf_f   = tf_f / (1 − b_f + b_f · dl_f / avgdl_f)
tilde_tf    = Σ_f  w_f · norm_tf_f
score(t,d)  = IDF(t) · tilde_tf · (k₁ + 1) / (tilde_tf + k₁)
```

Default field weights: title 3.0, headers 2.0, code 1.5, body 1.0. Per-field `b` values: 0.3 / 0.5 / 0.7 / 0.75 (lower `b` for title because titles are short and uniform; higher `b` for body because bodies are long and variable).

## Query pipeline

```
user query
  → spell correction (trigram → doc-freq filter → Jaccard → Levenshtein)
  → for each term: read posting list from disk (one seek + read per term)
  → sorted intersection across all term postings (smallest list first)
  → for each candidate doc:
        → BM25F score using global IDF + per-field tf/dl/avgdl + field weights
  → sort descending by score
  → top-K results with file paths
```

## Project structure

```
src/
├── main.rs              — orchestration, query loop
├── traverse.rs           — recursive document ingestion, SPIMI block flushes
├── field_extract.rs      — regex-driven markdown → 4 field strings
├── cleanup.rs            — tokenization, CamelCase + underscore splitting
├── encode_decode.rs      — VByte + gap encoding, field-keyed serialization,
│                            Field enum (#[repr(u8)])
├── block_merge.rs        — streaming k-way merge, single contiguous index file,
│                            TermEntry construction
├── get_posting.rs        — disk reads by offset
├── intersect.rs          — two-pointer sorted intersection, smallest-first
├── spell_check.rs        — trigram + doc-freq filter + Jaccard + Levenshtein
├── three_gram_index.rs   — trigram index construction
├── tf_idf_index.rs       — BM25F scoring (legacy filename retained)
└── phrase_check.rs       — retired (kept on disk as historical reference)
```

## Performance

**Index** (AWS docs corpus, 14,266 documents, 18 services):

| Metric              | Value           |
|---------------------|-----------------|
| Indexing time       | ~150 seconds    |
| Merge time          | ~17 seconds     |
| Unique terms        | ~180,000        |
| `final_index.bin`   | ~30 MB          |

**Query latency** across the 13-query golden set:

| Query type                           | Latency        |
|--------------------------------------|----------------|
| Single rare term (`RunInstances`)    | 3 – 5 ms       |
| Single common term (`s3`)            | ~45 ms         |
| 2-term phrase (`vpc peering`)        | ~25 ms         |
| 3-term query (`iam policy syntax`)   | 50 – 70 ms     |
| Typo correction (`permssion`)        | 13 ms total    |

**Golden set quality.** On a 13-query qualitative test set ranging from navigational (`RunInstances`, `vpc peering`) to conceptual (`lambda cold start`) to high-frequency (`s3`, `bucket policy permissions`) to typos (`permssion`):

| Result        | Count |
|---------------|-------|
| A / A+        | 6     |
| B / C+        | 5     |
| Failures left | 2     |

Failures are now diagnosable — the math is explicit, per-field, and exposable. The two remaining failures hit BM25F's structural ceiling: bag-of-words scoring can't distinguish "this doc is *about* the term" from "this doc mentions the term twenty times."

## Articles

Each release is documented in long form on [krithik.xyz](https://krithik.xyz):

**Foundation series** (20 Newsgroups corpus):
1. Inverted positional index + two-pointer intersection
2. Phrase search + spell correction
3. Reading from disk
4. VByte compression
5. TF-IDF + cosine normalization
6. Tiered indexes
7. Proximity scoring

**Real corpus series** (AWS documentation):
8. Why I'm changing course: from the Manning book to AWS docs
9. The AWS corpus, the tokenizer, and the spell corrector
10. Killing tiers to make room for BM25F
11. Fields: extracting structure from AWS markdown for BM25F
12. CamelCase tokenization and the BM25F ceiling

## What's next

- **PageRank from the cross-link graph.** AWS docs cross-link extensively; pages with high inbound link count are more authoritative. The graph is already implicit from the scraper's Pass 2.
- **Evaluation harness.** 30+ hand-judged queries with relevance labels. P@1, P@10, MAP, NDCG. The 13-query golden set was a placeholder.
- **HTTP server + demo.** axum, Cloudflare-fronted demo, walkthrough video.

## Build and run

```bash
cargo build --release
cargo run --release
```

Expects the AWS docs corpus at the path specified in `main.rs`. Update the `root` variable to point to your local copy.

## License

Code is open source. Articles on [krithik.xyz](https://krithik.xyz) describe what each piece does and why.