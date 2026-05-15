#!/usr/bin/env python3
"""
analyze_chars.py — scan the AWS docs corpus to figure out which non-alphanumeric
characters belong inside tokens vs. between them.

Reads every .md file under corpus/, counts each non-alphanumeric character, and
classifies each occurrence:
  - token-internal: alphanumeric on BOTH sides (e.g. the '-' in 's3-bucket')
  - boundary:       whitespace/punct on at least one side (e.g. the ',' in 'a, b')

Outputs:
  - prints a ranked table to stdout
  - writes char_analysis.csv (full report, all 87+ chars, with examples)

Run from the directory that contains corpus/:
    python analyze_chars.py
"""

import csv
from collections import Counter
from pathlib import Path
import sys


CORPUS_DIR = "corpus"
TOP_N = 30           # how many non-alphanum chars to print to stdout
CSV_PATH = "char_analysis.csv"


def classify_char(prev_ch, next_ch):
    """A char is token-internal if both neighbors are alphanumeric."""
    if prev_ch is None or next_ch is None:
        return "boundary"
    if prev_ch.isalnum() and next_ch.isalnum():
        return "internal"
    return "boundary"


def main():
    corpus = Path(CORPUS_DIR)
    if not corpus.exists():
        print(f"error: {CORPUS_DIR}/ not found. run from hello_cargo/.")
        sys.exit(1)

    total_chars = Counter()
    internal_chars = Counter()
    boundary_chars = Counter()
    examples = {}        # char -> set of short example tokens
    files_scanned = 0

    for path in corpus.rglob("*.md"):
        files_scanned += 1
        try:
            text = path.read_text(encoding="utf-8", errors="replace")
        except Exception:
            continue

        n = len(text)
        for i, ch in enumerate(text):
            if ch.isalnum() or ch.isspace():
                continue
            total_chars[ch] += 1
            prev_ch = text[i - 1] if i > 0 else None
            next_ch = text[i + 1] if i + 1 < n else None
            kind = classify_char(prev_ch, next_ch)
            if kind == "internal":
                internal_chars[ch] += 1
                if ch not in examples or len(examples[ch]) < 5:
                    left = i
                    while left > 0 and (text[left - 1].isalnum() or text[left - 1] in "._-:/"):
                        left -= 1
                    right = i
                    while right + 1 < n and (text[right + 1].isalnum() or text[right + 1] in "._-:/"):
                        right += 1
                    example = text[left:right + 1]
                    if 2 <= len(example) <= 60 and "\n" not in example:
                        examples.setdefault(ch, set()).add(example)
            else:
                boundary_chars[ch] += 1

        if files_scanned % 2000 == 0:
            print(f"  scanned {files_scanned} files...")

    print(f"\nscanned {files_scanned} files")
    print(f"total unique non-alphanumeric chars: {len(total_chars)}")
    print()

    # --- print top N to stdout ---
    print(f"{'char':>6} {'total':>12} {'internal':>12} {'boundary':>12}  {'internal%':>10}  examples")
    print("-" * 100)

    for ch, total in total_chars.most_common(TOP_N):
        internal = internal_chars[ch]
        boundary = boundary_chars[ch]
        pct = (internal / total * 100) if total else 0
        ex_set = examples.get(ch, set())
        ex_str = ", ".join(sorted(ex_set, key=len)[:3])
        display = repr(ch) if not ch.isprintable() or ch in " \t" else ch
        print(f"{display:>6} {total:>12,} {internal:>12,} {boundary:>12,}  {pct:>9.1f}%  {ex_str}")

    # --- write FULL report to CSV (all chars, not just top N) ---
    with open(CSV_PATH, "w", newline="", encoding="utf-8") as f:
        writer = csv.writer(f)
        writer.writerow([
            "char", "char_repr", "unicode_codepoint",
            "total", "internal", "boundary", "internal_pct",
            "recommendation", "examples"
        ])
        for ch, total in total_chars.most_common():
            internal = internal_chars[ch]
            boundary = boundary_chars[ch]
            pct = (internal / total * 100) if total else 0
            if pct > 50:
                rec = "KEEP"
            elif pct < 10:
                rec = "DISCARD"
            else:
                rec = "REVIEW"
            ex_set = examples.get(ch, set())
            ex_str = " | ".join(sorted(ex_set, key=len)[:5])
            writer.writerow([
                ch,
                repr(ch),
                f"U+{ord(ch):04X}",
                total,
                internal,
                boundary,
                f"{pct:.2f}",
                rec,
                ex_str,
            ])

    print()
    print(f"full report written to: {Path(CSV_PATH).resolve()}")
    print()
    print("interpretation:")
    print("  - internal% > 50%  -> probably KEEP (token-internal char)")
    print("  - internal% < 10%  -> probably DISCARD (boundary punctuation)")
    print("  - in between       -> eyeball the examples and decide")


if __name__ == "__main__":
    main()