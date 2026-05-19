#!/usr/bin/env python3
"""
analyze_fields.py — extract field structure from AWS markdown docs.

Mirrors the Rust tokenizer's logic (cleanup.rs):
  - lowercase
  - normalize curly quotes + ligatures
  - keep chars: . - _ : / ' @ *
  - strict ASCII alphanumeric only (no Unicode letters)
  - trim edge punctuation: . - _ : ' @  (NOT * or /)
  - drop tokens > 64 chars

Extracts four fields per doc:
  - title       (first H1)
  - headers     (all H2/H3/H4)
  - code        (fenced blocks + inline spans)
  - body        (everything else)

Outputs (in ./output/):
  - all_fields.txt
  - per_doc_fields.csv
  - field_stats.csv
  - field_overlap.csv
  - samples/                — random doc extractions for sanity check
  - analysis_summary.md
"""

import csv
import random
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path


# ---------- config ----------
CORPUS_DIR = "corpus"
OUTPUT_DIR = "output"
SAMPLES_TO_DUMP = 8       # how many random docs to dump as samples
SEED = 42

KEEP = set(".-_:/'@*")
TRIM_AT_EDGES = set(".-_:'@")
MAX_TOKEN_LEN = 64

LIGATURES = {
    "\u2018": "'", "\u2019": "'",
    "\u201C": '"', "\u201D": '"',
    "\uFB00": "ff", "\uFB01": "fi", "\uFB02": "fl",
    "\uFB03": "ffi", "\uFB04": "ffl",
}


# ---------- tokenizer (mirrors Rust cleanup.rs) ----------
def normalize(text: str) -> str:
    """Curly quotes + ligatures -> ASCII equivalents."""
    return "".join(LIGATURES.get(c, c) for c in text)


def is_token_char(c: str) -> bool:
    return (c.isascii() and c.isalnum()) or c in KEEP


def trim_edges(tok: str) -> str:
    return tok.strip("".join(TRIM_AT_EDGES))


def tokenize(text: str) -> list:
    """Token stream matching Rust split_string semantics."""
    text = normalize(text).lower()
    tokens = []
    current = []
    for c in text:
        if is_token_char(c):
            current.append(c)
        else:
            if current:
                tok = trim_edges("".join(current))
                if tok and len(tok) <= MAX_TOKEN_LEN:
                    tokens.append(tok)
                current = []
    if current:
        tok = trim_edges("".join(current))
        if tok and len(tok) <= MAX_TOKEN_LEN:
            tokens.append(tok)
    return tokens


# ---------- markdown field extraction ----------
RE_ANCHOR     = re.compile(r"<a\s+name=\"[^\"]*\"\s*></a>")
RE_IMAGE      = re.compile(r"!\[[^\]]*\]\([^)]*\)")
RE_LINK       = re.compile(r"\[([^\]]+)\]\([^)]*\)")   # keep anchor text, drop url
RE_FENCED     = re.compile(r"```[^\n]*\n(.*?)```", re.DOTALL)
RE_INLINE     = re.compile(r"`([^`\n]+)`")
RE_H1         = re.compile(r"^#\s+(.+?)\s*$", re.MULTILINE)
RE_H234       = re.compile(r"^#{2,4}\s+(.+?)\s*$", re.MULTILINE)
# Markdown emphasis markers — strip after code is extracted, before tokenizing.
# Handles: **bold**, __bold__, *italic*, _italic_, and stray ** or __ runs.
# Note: order matters — strip doubles BEFORE singles so ** doesn't become * *
RE_EMPHASIS_DOUBLE = re.compile(r"\*\*|__")
RE_EMPHASIS_SINGLE = re.compile(r"(?<![A-Za-z0-9_])[*_](?![A-Za-z0-9_])")


def extract_fields(raw: str) -> dict:
    """Return {title, headers, code, body} as strings (pre-tokenization)."""
    # 1. strip anchors and images entirely
    text = RE_ANCHOR.sub(" ", raw)
    text = RE_IMAGE.sub(" ", text)

    # 2. pull code first (fenced + inline) so it doesn't bleed into body
    code_parts = []
    for m in RE_FENCED.finditer(text):
        code_parts.append(m.group(1))
    text_no_fenced = RE_FENCED.sub(" ", text)

    for m in RE_INLINE.finditer(text_no_fenced):
        code_parts.append(m.group(1))
    text_no_code = RE_INLINE.sub(" ", text_no_fenced)

    # 3. pull title (first H1)
    title_match = RE_H1.search(text_no_code)
    title = title_match.group(1) if title_match else ""

    # 4. pull headers (H2/H3/H4)
    headers = " ".join(RE_H234.findall(text_no_code))

    # 5. body = everything else
    #    remove the H1/H2/H3/H4 lines from body
    body_text = RE_H1.sub(" ", text_no_code)
    body_text = RE_H234.sub(" ", body_text)
    #    flatten markdown link syntax -> keep anchor text only
    body_text = RE_LINK.sub(r"\1", body_text)

    # 6. strip markdown emphasis (**, __, *, _) from non-code fields.
    #    Code field stays raw because * has semantic meaning there (e.g. C pointers,
    #    glob patterns, IAM wildcards in JSON examples).
    def strip_emphasis(s: str) -> str:
        s = RE_EMPHASIS_DOUBLE.sub(" ", s)
        s = RE_EMPHASIS_SINGLE.sub(" ", s)
        return s

    title     = strip_emphasis(title)
    headers   = strip_emphasis(headers)
    body_text = strip_emphasis(body_text)

    return {
        "title":   title,
        "headers": headers,
        "code":    "\n".join(code_parts),
        "body":    body_text,
    }


# ---------- analysis ----------
def analyze_corpus(corpus_root: Path, out_root: Path):
    out_root.mkdir(parents=True, exist_ok=True)
    samples_dir = out_root / "samples"
    samples_dir.mkdir(exist_ok=True)

    md_files = sorted(corpus_root.rglob("*.md"))
    if not md_files:
        print(f"error: no .md files found under {corpus_root}", file=sys.stderr)
        sys.exit(1)

    print(f"scanning {len(md_files)} markdown files...")

    # accumulators
    field_token_total = Counter()   # field -> total tokens corpus-wide
    field_doc_count   = Counter()   # field -> # docs where field is non-empty
    field_lengths     = defaultdict(list)   # field -> [token_count per doc]
    overlap           = Counter()           # frozenset of present fields -> count
    per_doc_rows      = []                  # rows for per_doc_fields.csv

    # pick sample docs reproducibly
    random.seed(SEED)
    sample_indices = set(random.sample(range(len(md_files)), min(SAMPLES_TO_DUMP, len(md_files))))

    for i, path in enumerate(md_files):
        try:
            raw = path.read_text(encoding="utf-8", errors="replace")
        except Exception as e:
            print(f"  skip {path}: {e}", file=sys.stderr)
            continue

        fields = extract_fields(raw)
        token_counts = {f: len(tokenize(text)) for f, text in fields.items()}

        present = frozenset(f for f, n in token_counts.items() if n > 0)
        overlap[present] += 1

        for f, n in token_counts.items():
            field_token_total[f] += n
            if n > 0:
                field_doc_count[f] += 1
                field_lengths[f].append(n)

        # rel path from corpus root for readability
        rel = path.relative_to(corpus_root)
        per_doc_rows.append({
            "doc_path": str(rel),
            "tokens_title":   token_counts["title"],
            "tokens_headers": token_counts["headers"],
            "tokens_code":    token_counts["code"],
            "tokens_body":    token_counts["body"],
            "tokens_total":   sum(token_counts.values()),
        })

        # dump samples
        if i in sample_indices:
            sample_path = samples_dir / (rel.as_posix().replace("/", "__") + ".txt")
            with open(sample_path, "w", encoding="utf-8") as f:
                f.write(f"# source: {rel}\n\n")
                for fname in ("title", "headers", "code", "body"):
                    text = fields[fname]
                    toks = tokenize(text)
                    f.write(f"--- FIELD: {fname} ({len(toks)} tokens) ---\n")
                    f.write(f"RAW (first 500 chars):\n{text[:500]}\n\n")
                    f.write(f"TOKENS (first 40):\n{toks[:40]}\n\n")

        if (i + 1) % 2000 == 0:
            print(f"  processed {i+1} files")

    n_docs = len(md_files)
    print(f"done. processed {n_docs} files.\n")

    # ---------- writes ----------
    # 1. all_fields.txt
    with open(out_root / "all_fields.txt", "w") as f:
        for fname in ("title", "headers", "code", "body"):
            f.write(f"{fname}\n")

    # 2. per_doc_fields.csv
    with open(out_root / "per_doc_fields.csv", "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=per_doc_rows[0].keys())
        w.writeheader()
        w.writerows(per_doc_rows)

    # 3. field_stats.csv
    with open(out_root / "field_stats.csv", "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["field", "total_tokens", "docs_present", "doc_coverage_pct",
                    "avg_tokens_when_present", "min_tokens", "max_tokens", "median_tokens"])
        for fname in ("title", "headers", "code", "body"):
            lengths = field_lengths[fname]
            if lengths:
                lengths_sorted = sorted(lengths)
                med = lengths_sorted[len(lengths_sorted) // 2]
                avg = sum(lengths) / len(lengths)
                mn, mx = lengths_sorted[0], lengths_sorted[-1]
            else:
                med = avg = mn = mx = 0
            w.writerow([
                fname,
                field_token_total[fname],
                field_doc_count[fname],
                f"{field_doc_count[fname] / n_docs * 100:.2f}",
                f"{avg:.2f}",
                mn, mx, med,
            ])

    # 4. field_overlap.csv
    with open(out_root / "field_overlap.csv", "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["fields_present", "doc_count", "pct_of_corpus"])
        for combo, count in sorted(overlap.items(), key=lambda x: -x[1]):
            combo_str = "+".join(sorted(combo)) if combo else "(none)"
            w.writerow([combo_str, count, f"{count / n_docs * 100:.2f}"])

    # 5. analysis_summary.md
    with open(out_root / "analysis_summary.md", "w") as f:
        f.write("# AWS Corpus Field Analysis\n\n")
        f.write(f"**Total docs scanned:** {n_docs}\n\n")
        f.write("## Field stats\n\n")
        f.write("| field | total tokens | docs present | coverage | avg len | median | min | max |\n")
        f.write("|---|---:|---:|---:|---:|---:|---:|---:|\n")
        for fname in ("title", "headers", "code", "body"):
            lengths = field_lengths[fname]
            if lengths:
                lengths_sorted = sorted(lengths)
                med = lengths_sorted[len(lengths_sorted) // 2]
                avg = sum(lengths) / len(lengths)
                mn, mx = lengths_sorted[0], lengths_sorted[-1]
            else:
                med = avg = mn = mx = 0
            cov = field_doc_count[fname] / n_docs * 100
            f.write(f"| `{fname}` | {field_token_total[fname]:,} | {field_doc_count[fname]:,} "
                    f"| {cov:.2f}% | {avg:.1f} | {med} | {mn} | {mx} |\n")

        f.write("\n## Field combinations\n\n")
        f.write("How many docs have which combo of non-empty fields.\n\n")
        f.write("| fields_present | doc_count | pct |\n|---|---:|---:|\n")
        for combo, count in sorted(overlap.items(), key=lambda x: -x[1])[:10]:
            combo_str = "+".join(sorted(combo)) if combo else "(none)"
            f.write(f"| {combo_str} | {count:,} | {count / n_docs * 100:.2f}% |\n")

        f.write("\n## Notes for BM25F tuning\n\n")
        f.write("- Coverage tells you how often each field is even present. "
                "A field present in <5% of docs is rarely worth its own weight.\n")
        f.write("- Length tells you how aggressive `b` (length normalization) should be. "
                "Short uniform fields (title) want low `b`. Long variable fields (body) want higher `b`.\n")
        f.write("- Check `samples/` to eyeball whether extraction is clean. If code is leaking into body or "
                "headers are bleeding into title, the regex needs adjustment.\n")

    print(f"output written to: {out_root.resolve()}")


if __name__ == "__main__":
    corpus_arg = sys.argv[1] if len(sys.argv) > 1 else CORPUS_DIR
    out_arg    = sys.argv[2] if len(sys.argv) > 2 else OUTPUT_DIR
    analyze_corpus(Path(corpus_arg), Path(out_arg))