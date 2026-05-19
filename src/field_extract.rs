use regex::Regex;
use std::sync::OnceLock;

pub struct ExtractedFields {
    pub title:   String,
    pub headers: String,
    pub code:    String,
    pub body:    String,
}

// Compiled once at first call, reused forever. Regex compilation is expensive.
fn regexes() -> &'static Regexes {
    static R: OnceLock<Regexes> = OnceLock::new();
    R.get_or_init(Regexes::new)
}

struct Regexes {
    anchor:           Regex,
    image:            Regex,
    link:             Regex,
    fenced:           Regex,
    inline_code:      Regex,
    h1:               Regex,
    h234:             Regex,
    emphasis_double:  Regex,
    emphasis_single:  Regex,
}

impl Regexes {
    fn new() -> Self {
        Regexes {
            anchor:          Regex::new(r#"<a\s+name="[^"]*"\s*></a>"#).unwrap(),
            image:           Regex::new(r"!\[[^\]]*\]\([^)]*\)").unwrap(),
            link:            Regex::new(r"\[([^\]]+)\]\([^)]*\)").unwrap(),
            fenced:          Regex::new(r"(?s)```[^\n]*\n(.*?)```").unwrap(),
            inline_code:     Regex::new(r"`([^`\n]+)`").unwrap(),
            h1:              Regex::new(r"(?m)^#\s+(.+?)\s*$").unwrap(),
            h234:            Regex::new(r"(?m)^#{2,4}\s+(.+?)\s*$").unwrap(),
            emphasis_double: Regex::new(r"\*\*|__").unwrap(),
            emphasis_single: Regex::new(r"(^|[^A-Za-z0-9_])[*_]($|[^A-Za-z0-9_])").unwrap(),
        }
    }
}

pub fn extract_fields(raw: &str) -> ExtractedFields {
    let r = regexes();

    // 1. strip anchors and images
    let text = r.anchor.replace_all(raw, " ");
    let text = r.image.replace_all(&text, " ");

    // 2. pull fenced code blocks first — before anything else touches them
    let mut code_parts: Vec<String> = Vec::new();
    for cap in r.fenced.captures_iter(&text) {
        code_parts.push(cap[1].to_string());
    }
    let text_no_fenced = r.fenced.replace_all(&text, " ");

    // pull inline code
    for cap in r.inline_code.captures_iter(&text_no_fenced) {
        code_parts.push(cap[1].to_string());
    }
    let text_no_code = r.inline_code.replace_all(&text_no_fenced, " ");

    // 3. title — first H1
    let title_raw = r.h1
        .captures(&text_no_code)
        .map(|c| c[1].to_string())
        .unwrap_or_default();

    // 4. headers — all H2/H3/H4
    let headers_raw = r.h234
        .captures_iter(&text_no_code)
        .map(|c| c[1].to_string())
        .collect::<Vec<_>>()
        .join(" ");

    // 5. body — everything left after removing H1/H2/H3/H4 lines
    let body_text = r.h1.replace_all(&text_no_code, " ");
    let body_text = r.h234.replace_all(&body_text, " ");
    let body_text = r.link.replace_all(&body_text, "$1"); // keep anchor text, drop URL

    // 6. strip markdown emphasis from non-code fields
    //    code field stays raw — * has semantic meaning in globs, IAM, C pointers
    let strip_emphasis = |s: &str| -> String {
        let s = r.emphasis_double.replace_all(s, " ");
        r.emphasis_single.replace_all(&s, "$1 $2").into_owned()
    };

    ExtractedFields {
        title:   strip_emphasis(&title_raw),
        headers: strip_emphasis(&headers_raw),
        code:    code_parts.join("\n"),
        body:    strip_emphasis(&body_text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_title() {
        let raw = "# My Title\n\nsome body text";
        let f = extract_fields(raw);
        assert_eq!(f.title.trim(), "My Title");
    }

    #[test]
    fn code_does_not_bleed_into_body() {
        let raw = "# Title\n\n```python\nprint('hello')\n```\n\nsome body";
        let f = extract_fields(raw);
        assert!(f.code.contains("print"), "code missing from code field");
        assert!(!f.body.contains("print"), "code bled into body");
    }

    #[test]
    fn strips_emphasis_from_body_not_code() {
        let raw = "# Title\n\n**bold text** here\n\n```\nfn foo() -> *mut T\n```";
        let f = extract_fields(raw);
        assert!(!f.body.contains("**"), "** not stripped from body");
        assert!(f.code.contains("*mut T"), "* incorrectly stripped from code");
    }

    #[test]
    fn empty_doc_does_not_panic() {
        let f = extract_fields("");
        assert!(f.title.is_empty());
        assert!(f.code.is_empty());
    }
}