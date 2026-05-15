use std::fs;

// Tokenizer for AWS docs markdown.
//
// Keep list: . - _ : / ' @ *
// These survived analysis of the 14,266-file corpus as token-internal chars
// in AWS-shaped strings (ARNs, IAM actions, instance types, paths, URLs,
// emails, IAM wildcards, contractions). Everything else is a split point.
//
// Normalization:
//   - curly quotes -> straight quotes (so `don't` and `don’t` collapse)
//   - lowercase

const KEEP: &[char] = &['.', '-', '_', ':', '/', '\'', '@', '*'];

/// Max token length. Real AWS tokens (full ARNs, snake_case API names) top out
/// around 50 chars. Anything longer is base64, binary garbage, or stray data
/// that has no search value. Dropping it saves memory and IDF noise.
const MAX_TOKEN_LEN: usize = 64;

/// Read file bytes, decode as UTF-8 lossily (invalid bytes -> replacement char),
/// return as a String.
pub fn read_contents(file_path: &str) -> String {
    let content = fs::read(file_path).unwrap();
    String::from_utf8_lossy(&content).to_string()
}

/// Normalize curly punctuation and common Latin ligatures to ASCII equivalents.
/// Returns Some(replacement) for known special chars; None means "use char as-is."
///
/// Ligatures expand to multiple chars (ﬁ -> "fi"), so we can't return a single char.
fn normalize_special(c: char) -> Option<&'static str> {
    match c {
        // curly quotes
        '\u{2018}' | '\u{2019}' => Some("'"),   // ‘ ’ -> '
        '\u{201C}' | '\u{201D}' => Some("\""),  // “ ” -> "
        // latin ligatures (19 occurrences in AWS corpus: ﬁ ﬂ ﬃ; covering all 5 for safety)
        '\u{FB00}' => Some("ff"),   // ﬀ
        '\u{FB01}' => Some("fi"),   // ﬁ
        '\u{FB02}' => Some("fl"),   // ﬂ
        '\u{FB03}' => Some("ffi"),  // ﬃ
        '\u{FB04}' => Some("ffl"),  // ﬄ
        _ => None,
    }
}

/// Returns true if c is a token character.
/// We deliberately restrict to ASCII alphanumeric (not Unicode-wide
/// is_alphanumeric()) because the corpus uses Unicode-category "Number" chars
/// like superscript `¹` (U+00B9) as footnote markers fused to words
/// (e.g. `numberofmessagesdeleted¹`). Those are noise tokens — they have
/// garbage IDF and have crashed byte-slicing code downstream. Strict ASCII
/// is the right gate; the keep list handles AWS-specific connector chars.
fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || KEEP.contains(&c)
}

/// Chars from the keep list that are meaningful ONLY when internal to a token.
/// Trimmed from the edges of tokens:
///   `.` `-` `_` `:` `'` `@`
///
/// NOT trimmed (these carry meaning at edges):
///   `*` — IAM wildcards like `s3:Get*` need the trailing star preserved
///   `/` — URL paths like `/api/endpoint` need the leading slash preserved
const TRIM_AT_EDGES: &[char] = &['.', '-', '_', ':', '\'', '@'];

/// Trim leading/trailing edge-meaningless keep-chars from a token.
/// Wildcards (`*`) and path roots (`/`) are preserved at edges.
fn trim_keep_chars(token: &str) -> &str {
    token
        .trim_start_matches(|c: char| TRIM_AT_EDGES.contains(&c))
        .trim_end_matches(|c: char| TRIM_AT_EDGES.contains(&c))
}

/// Tokenize a document into a flat list of lowercased terms.
///
/// Walks the string once. Builds tokens out of consecutive token chars.
/// Splits on anything else (whitespace, punctuation not in the keep list).
/// Trims dangling keep-chars (e.g. `cause:` -> `cause`, but `s3:getobject` is preserved).
pub fn split_string(content: String) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    for raw in content.chars() {
        // Expand the char into one or more replacement chars (or keep as-is).
        // Need an iterator-friendly representation so we can handle both
        // single-char and multi-char cases uniformly.
        let replacement = normalize_special(raw);
        let chars_to_process: Box<dyn Iterator<Item = char>> = match replacement {
            Some(s) => Box::new(s.chars()),
            None => Box::new(std::iter::once(raw)),
        };

        for c in chars_to_process {
            if is_token_char(c) {
                current.extend(c.to_lowercase());
            } else if !current.is_empty() {
                let trimmed = trim_keep_chars(&current);
                if !trimmed.is_empty() && trimmed.len() <= MAX_TOKEN_LEN {
                    tokens.push(trimmed.to_string());
                }
                current.clear();
            }
        }
    }
    if !current.is_empty() {
        let trimmed = trim_keep_chars(&current);
        if !trimmed.is_empty() && trimmed.len() <= MAX_TOKEN_LEN {
            tokens.push(trimmed.to_string());
        }
    }
    tokens
}

// --- tests ---
// run with: cargo test --bin <your_bin> cleanup
#[cfg(test)]
mod tests {
    use super::*;

    fn tokenize(s: &str) -> Vec<String> {
        split_string(s.to_string())
    }

    #[test]
    fn keeps_iam_action() {
        assert_eq!(tokenize("s3:GetObject"), vec!["s3:getobject"]);
    }

    #[test]
    fn keeps_arn() {
        assert_eq!(
            tokenize("arn:aws:s3:::my-bucket"),
            vec!["arn:aws:s3:::my-bucket"]
        );
    }

    #[test]
    fn keeps_instance_type() {
        assert_eq!(tokenize("t2.micro"), vec!["t2.micro"]);
    }

    #[test]
    fn keeps_hyphenated_id() {
        assert_eq!(tokenize("vpc-a1b2c3d4"), vec!["vpc-a1b2c3d4"]);
    }

    #[test]
    fn keeps_region_code() {
        assert_eq!(tokenize("us-east-1"), vec!["us-east-1"]);
    }

    #[test]
    fn keeps_snake_case_identifier() {
        assert_eq!(
            tokenize("API_AssociateEncryptionConfig"),
            vec!["api_associateencryptionconfig"]
        );
    }

    #[test]
    fn keeps_http_version() {
        assert_eq!(tokenize("HTTP/1.1"), vec!["http/1.1"]);
    }

    #[test]
    fn keeps_content_type() {
        assert_eq!(tokenize("application/json"), vec!["application/json"]);
    }

    #[test]
    fn keeps_url_path() {
        assert_eq!(
            tokenize("/2013-04-01/trafficpolicyinstances"),
            vec!["/2013-04-01/trafficpolicyinstances"]
        );
    }

    #[test]
    fn keeps_email() {
        assert_eq!(
            tokenize("noreply@example.com"),
            vec!["noreply@example.com"]
        );
    }

    #[test]
    fn keeps_iam_wildcard() {
        assert_eq!(tokenize("s3:Get*"), vec!["s3:get*"]);
    }

    #[test]
    fn keeps_contraction_straight() {
        assert_eq!(tokenize("don't"), vec!["don't"]);
    }

    #[test]
    fn keeps_contraction_curly() {
        // U+2019 should normalize to U+0027
        assert_eq!(tokenize("don\u{2019}t"), vec!["don't"]);
    }

    #[test]
    fn splits_on_parens() {
        assert_eq!(
            tokenize("CheckDomainTransferability(string)"),
            vec!["checkdomaintransferability", "string"]
        );
    }

    #[test]
    fn splits_on_comma() {
        assert_eq!(tokenize("a, b, c"), vec!["a", "b", "c"]);
    }

    #[test]
    fn strips_cause_label_correctly() {
        // **Cause:** The specified... -> the ** strip is the adapter's job,
        // but tokenizer should still produce clean tokens from the result.
        // After ** removal we'd have: "Cause: The specified..."
        let tokens = tokenize("Cause: The specified cluster");
        assert_eq!(tokens, vec!["cause", "the", "specified", "cluster"]);
    }

    #[test]
    fn handles_resource_cpu() {
        assert_eq!(tokenize("RESOURCE:CPU"), vec!["resource:cpu"]);
    }

    #[test]
    fn handles_cli_command() {
        assert_eq!(
            tokenize("aws cloudfront update-distribution"),
            vec!["aws", "cloudfront", "update-distribution"]
        );
    }

    #[test]
    fn handles_empty_string() {
        let v: Vec<String> = Vec::new();
        assert_eq!(tokenize(""), v);
    }

    #[test]
    fn handles_only_punctuation() {
        let v: Vec<String> = Vec::new();
        assert_eq!(tokenize("!!! ??? ,,,"), v);
    }

    #[test]
    fn markdown_syntax_passes_through_when_adjacent_to_keep_chars() {
        // The tokenizer is NOT responsible for stripping markdown syntax.
        // That's the adapter's job (next layer up). Here we just verify
        // that the tokenizer behaves predictably when fed raw markdown.
        //
        // `**Note**` becomes one token because `*` is in the keep list
        // (needed for IAM wildcards like `s3:Get*`). The adapter will
        // strip `**` before this function ever sees the text.
        //
        // `##` is fully discarded because the trailing whitespace splits it,
        // and trim_keep_chars would remove any leading `#`-adjacent runs
        // (but `#` isn't even in the keep list, so it splits immediately).
        assert_eq!(
            tokenize("## **Note** [text](link.md)"),
            vec!["**note**", "text", "link.md"]
        );
    }

    #[test]
    fn adapter_stripped_input_tokenizes_cleanly() {
        // Simulates what split_string sees AFTER the markdown adapter has done its job:
        // `**Note**` -> `Note`, `## Heading` -> `Heading`, etc.
        assert_eq!(
            tokenize("Note text link.md"),
            vec!["note", "text", "link.md"]
        );
    }

    #[test]
    fn drops_oversized_tokens() {
        // 100-char base64-like blob. No real AWS query needs this.
        let blob = "a".repeat(100);
        assert!(tokenize(&blob).is_empty());
    }

    #[test]
    fn keeps_realistic_long_arn() {
        // A realistic full ARN, ~55 chars, should survive the 64-char cap.
        let arn = "arn:aws:iam::123456789012:role/MyVeryLongRoleName";
        assert!(arn.len() <= 64);
        assert_eq!(tokenize(arn), vec![arn.to_lowercase()]);
    }

    #[test]
    fn unicode_superscript_excluded_from_tokens() {
        // U+00B9 ¹ is a Unicode "Number" but it's a footnote marker, not a letter.
        // We deliberately exclude it via is_ascii_alphanumeric so it acts as a splitter.
        // The bad token `word¹` becomes `word` instead of crashing downstream byte-slicing.
        assert_eq!(tokenize("messagesdeleted¹"), vec!["messagesdeleted"]);
    }

    #[test]
    fn unicode_letters_excluded_from_tokens() {
        // CJK and accented Latin are also excluded. AWS docs are English ASCII.
        // The 2 Chinese page snippets in the corpus simply produce no tokens.
        // If you ever localize for non-ASCII corpora, revisit this.
        let toks = tokenize("café naïve");
        // 'caf', 'é' splits, 'naï' splits, 've' — depending on where Unicode lands
        // The exact tokenization isn't important; just verify nothing crashes
        // and no token contains non-ASCII.
        for t in &toks {
            assert!(t.chars().all(|c| c.is_ascii()));
        }
    }

    #[test]
    fn normalizes_fi_ligature() {
        // U+FB01 "ﬁ" should expand to "fi"
        assert_eq!(tokenize("signiﬁcant"), vec!["significant"]);
    }

    #[test]
    fn normalizes_fl_ligature() {
        assert_eq!(tokenize("ﬂow"), vec!["flow"]);
    }

    #[test]
    fn normalizes_ffi_ligature() {
        assert_eq!(tokenize("oﬃce"), vec!["office"]);
    }

    #[test]
    fn pure_punctuation_runs_get_dropped() {
        // Edge-trimmable chars: empty after trim, dropped.
        assert!(tokenize(":::").is_empty());
        assert!(tokenize("---").is_empty());
        assert!(tokenize("...").is_empty());
        assert!(tokenize("@@@").is_empty());
    }

    #[test]
    fn pure_wildcard_or_slash_runs_survive_as_noise() {
        // `*` and `/` aren't edge-trimmed (they carry meaning at edges).
        // A bare `***` or `///` thus survives. This is acceptable noise:
        // these tokens have garbage IDF and never match real queries.
        // The cost of preserving `s3:Get*` and `/api/path` correctly.
        assert_eq!(tokenize("***"), vec!["***"]);
        assert_eq!(tokenize("///"), vec!["///"]);
    }
}