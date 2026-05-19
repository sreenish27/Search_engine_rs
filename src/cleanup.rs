use std::fs;

// Tokenizer for AWS docs markdown.
//
// Keep list: . - _ : / ' @ *
// These survived analysis of the 14,266-file corpus as token-internal chars
// in AWS-shaped strings (ARNs, IAM actions, instance types, paths, URLs,
// emails, IAM wildcards, contractions). Everything else is a split point.
//
// Normalization:
//   - curly quotes -> straight quotes (so `don't` and `don't` collapse)
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
        '\u{2018}' | '\u{2019}' => Some("'"),   // ' ' -> '
        '\u{201C}' | '\u{201D}' => Some("\""),  // " " -> "
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

/// Split a string on CamelCase boundaries.
/// Returns the individual pieces only if a split actually occurred.
/// If no boundary found, returns empty vec (caller keeps the original).
///
/// Examples:
///   "RunInstances" -> ["Run", "Instances"]
///   "getXMLData"   -> ["get", "XML", "Data"]
///   "API"          -> []   (no boundary)
///   "bucket"       -> []   (no boundary)
fn camelcase_split(s: &str) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    let mut result = Vec::new();
    let mut current = String::new();

    for (i, &c) in chars.iter().enumerate() {
        let is_boundary = i > 0 && (
            // lowercase/digit -> uppercase: aB
            (chars[i-1].is_ascii_lowercase() || chars[i-1].is_ascii_digit()) && c.is_ascii_uppercase()
            // uppercase -> uppercase-lowercase: ABc (end of acronym like XML)
            || (i + 1 < chars.len() && chars[i-1].is_ascii_uppercase() && c.is_ascii_uppercase() && chars[i+1].is_ascii_lowercase())
        );
        if is_boundary && !current.is_empty() {
            result.push(current.clone());
            current.clear();
        }
        current.push(c);
    }
    if !current.is_empty() {
        result.push(current);
    }
    // only return pieces if we actually split — otherwise caller keeps original
    if result.len() > 1 { result } else { Vec::new() }
}

/// Split a token on CamelCase boundaries AND underscores.
/// Returns the original token PLUS all split pieces (case preserved).
/// Caller is responsible for lowercasing each piece.
///
/// Examples:
///   "RunInstances"     -> ["RunInstances", "Run", "Instances"]
///   "API_RunInstances" -> ["API_RunInstances", "API", "RunInstances", "Run", "Instances"]
///   "getXMLData"       -> ["getXMLData", "get", "XML", "Data"]
///   "bucket"           -> ["bucket"]
///   "API"              -> ["API"]
fn split_compound(token: &str) -> Vec<String> {
    let mut pieces: Vec<String> = vec![token.to_string()];

    // split on underscores first
    let underscore_parts: Vec<&str> = token.split('_').filter(|s| !s.is_empty()).collect();

    // only add underscore parts if the split actually changed anything
    let underscore_changed = underscore_parts.len() > 1
        || (underscore_parts.len() == 1 && underscore_parts[0] != token);

    for part in &underscore_parts {
        if underscore_changed {
            pieces.push(part.to_string());
        }
        // CamelCase split each part (or the whole token if no underscores)
        pieces.extend(camelcase_split(part));
    }

    pieces
}

/// Tokenize a document into a flat list of lowercased terms.
///
/// Walks the string once. Builds tokens out of consecutive token chars.
/// Splits on anything else (whitespace, punctuation not in the keep list).
/// Trims dangling keep-chars (e.g. `cause:` -> `cause`, but `s3:getobject` is preserved).
/// Splits CamelCase and underscore compounds — original token + split pieces all indexed.
pub fn split_string(content: String) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    for raw in content.chars() {
        let replacement = normalize_special(raw);
        let chars_to_process: Box<dyn Iterator<Item = char>> = match replacement {
            Some(s) => Box::new(s.chars()),
            None => Box::new(std::iter::once(raw)),
        };

        for c in chars_to_process {
            if is_token_char(c) {
                current.push(c); // keep case — needed for CamelCase splitting
            } else if !current.is_empty() {
                let trimmed = trim_keep_chars(&current);
                if !trimmed.is_empty() && trimmed.len() <= MAX_TOKEN_LEN {
                    // split on CamelCase + underscores, lowercase each piece
                    for piece in split_compound(trimmed) {
                        let lower = piece.to_lowercase();
                        if !lower.is_empty() && lower.len() <= MAX_TOKEN_LEN {
                            tokens.push(lower);
                        }
                    }
                }
                current.clear();
            }
        }
    }
    // handle trailing token
    if !current.is_empty() {
        let trimmed = trim_keep_chars(&current);
        if !trimmed.is_empty() && trimmed.len() <= MAX_TOKEN_LEN {
            for piece in split_compound(trimmed) {
                let lower = piece.to_lowercase();
                if !lower.is_empty() && lower.len() <= MAX_TOKEN_LEN {
                    tokens.push(lower);
                }
            }
        }
    }
    tokens
}

// --- tests ---
#[cfg(test)]
mod tests {
    use super::*;

    fn tokenize(s: &str) -> Vec<String> {
        split_string(s.to_string())
    }

    #[test]
    fn keeps_iam_action() {
        // s3:GetObject — no underscore, CamelCase splits "GetObject" -> "Get" + "Object"
        // original preserved: s3:getobject
        let toks = tokenize("s3:GetObject");
        assert!(toks.contains(&"s3:getobject".to_string()));
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
        // now produces original + split pieces
        let toks = tokenize("API_AssociateEncryptionConfig");
        assert!(toks.contains(&"api_associateencryptionconfig".to_string()));
        assert!(toks.contains(&"api".to_string()));
        assert!(toks.contains(&"associate".to_string()));
        assert!(toks.contains(&"encryption".to_string()));
        assert!(toks.contains(&"config".to_string()));
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
        assert_eq!(tokenize("don\u{2019}t"), vec!["don't"]);
    }

    #[test]
    fn splits_on_parens() {
        let toks = tokenize("CheckDomainTransferability(string)");
        assert!(toks.contains(&"checkdomaintransferability".to_string()));
        assert!(toks.contains(&"string".to_string()));
    }

    #[test]
    fn splits_on_comma() {
        assert_eq!(tokenize("a, b, c"), vec!["a", "b", "c"]);
    }

    #[test]
    fn strips_cause_label_correctly() {
        let tokens = tokenize("Cause: The specified cluster");
        assert!(tokens.contains(&"cause".to_string()));
        assert!(tokens.contains(&"the".to_string()));
        assert!(tokens.contains(&"specified".to_string()));
        assert!(tokens.contains(&"cluster".to_string()));
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
    fn drops_oversized_tokens() {
        let blob = "a".repeat(100);
        assert!(tokenize(&blob).is_empty());
    }

    #[test]
    fn keeps_realistic_long_arn() {
        let arn = "arn:aws:iam::123456789012:role/MyVeryLongRoleName";
        assert!(arn.len() <= 64);
        assert!(tokenize(arn).contains(&arn.to_lowercase()));
    }

    #[test]
    fn unicode_superscript_excluded_from_tokens() {
        assert_eq!(tokenize("messagesdeleted¹"), vec!["messagesdeleted"]);
    }

    #[test]
    fn unicode_letters_excluded_from_tokens() {
        let toks = tokenize("café naïve");
        for t in &toks {
            assert!(t.chars().all(|c| c.is_ascii()));
        }
    }

    #[test]
    fn normalizes_fi_ligature() {
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
        assert!(tokenize(":::").is_empty());
        assert!(tokenize("---").is_empty());
        assert!(tokenize("...").is_empty());
        assert!(tokenize("@@@").is_empty());
    }

    #[test]
    fn pure_wildcard_or_slash_runs_survive_as_noise() {
        assert_eq!(tokenize("***"), vec!["***"]);
        assert_eq!(tokenize("///"), vec!["///"]);
    }

    // --- new CamelCase / compound splitting tests ---

    #[test]
    fn camelcase_splits_simple() {
        let toks = tokenize("RunInstances");
        assert!(toks.contains(&"runinstances".to_string()));
        assert!(toks.contains(&"run".to_string()));
        assert!(toks.contains(&"instances".to_string()));
    }

    #[test]
    fn camelcase_splits_acronym() {
        // getXMLData -> ["getxmldata", "get", "xml", "data"]
        let toks = tokenize("getXMLData");
        assert!(toks.contains(&"getxmldata".to_string()));
        assert!(toks.contains(&"get".to_string()));
        assert!(toks.contains(&"xml".to_string()));
        assert!(toks.contains(&"data".to_string()));
    }

    #[test]
    fn underscore_and_camelcase_combined() {
        let toks = tokenize("API_RunInstances");
        assert!(toks.contains(&"api_runinstances".to_string()));
        assert!(toks.contains(&"api".to_string()));
        assert!(toks.contains(&"runinstances".to_string()));
        assert!(toks.contains(&"run".to_string()));
        assert!(toks.contains(&"instances".to_string()));
    }

    #[test]
    fn single_word_no_split() {
        // plain lowercase — no boundaries, no duplicates
        assert_eq!(tokenize("bucket"), vec!["bucket"]);
    }

    #[test]
    fn all_caps_no_split() {
        // "API" — all uppercase, no boundary fires
        assert_eq!(tokenize("API"), vec!["api"]);
    }
}