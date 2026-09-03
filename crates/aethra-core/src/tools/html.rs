//! Minimal HTML to readable text. Not a browser: no scripts, no CSS, no
//! layout. Good enough to hand a page to a language model and to verify that
//! a quoted span really appears in a source.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Extracted {
    pub title: Option<String>,
    pub text: String,
}

const SKIP_CONTENT: &[&str] = &["script", "style", "noscript", "svg", "template", "iframe", "canvas"];

const BLOCK_TAGS: &[&str] = &[
    "p", "div", "br", "li", "ul", "ol", "h1", "h2", "h3", "h4", "h5", "h6", "tr", "table", "thead", "tbody",
    "section", "article", "header", "footer", "nav", "aside", "main", "blockquote", "pre", "hr", "dl", "dt",
    "dd", "figure", "figcaption", "details", "summary", "form", "fieldset", "address", "body", "html", "head",
];

pub fn html_to_text(html: &str) -> Extracted {
    let mut out = String::with_capacity(html.len() / 2);
    let mut title: Option<String> = None;
    let mut rest = html;

    while let Some(lt) = rest.find('<') {
        push_text(&mut out, &rest[..lt]);
        rest = &rest[lt..];

        if rest.starts_with("<!--") {
            match rest.find("-->") {
                Some(end) => rest = &rest[end + 3..],
                None => {
                    rest = "";
                }
            }
            continue;
        }

        let Some(gt) = rest.find('>') else {
            rest = "";
            break;
        };
        let tag_body = &rest[1..gt];
        rest = &rest[gt + 1..];

        let (name, closing) = parse_tag_name(tag_body);
        let lname = name.to_ascii_lowercase();

        if !closing && SKIP_CONTENT.contains(&lname.as_str()) {
            rest = skip_past_closing(rest, &lname);
            continue;
        }
        if !closing && lname == "title" {
            if let Some(end) = find_ci(rest, "</title") {
                let t = collapse_ws(&decode_entities(&rest[..end]));
                if !t.is_empty() && title.is_none() {
                    title = Some(t);
                }
                rest = skip_past_closing(rest, "title");
            }
            continue;
        }

        if BLOCK_TAGS.contains(&lname.as_str()) {
            out.push('\n');
        } else {
            out.push(' ');
        }
    }
    push_text(&mut out, rest);

    Extracted {
        title,
        text: tidy(&out),
    }
}

fn parse_tag_name(tag_body: &str) -> (&str, bool) {
    let trimmed = tag_body.trim_start();
    let (closing, body) = match trimmed.strip_prefix('/') {
        Some(b) => (true, b),
        None => (false, trimmed),
    };
    let end = body
        .find(|c: char| c.is_whitespace() || c == '/' || c == '>')
        .unwrap_or(body.len());
    (&body[..end], closing)
}

/// Advances past `</name ...>`; if no closing tag exists, drops the rest.
fn skip_past_closing<'a>(rest: &'a str, name: &str) -> &'a str {
    let needle = format!("</{name}");
    match find_ci(rest, &needle) {
        Some(pos) => {
            let after = &rest[pos..];
            match after.find('>') {
                Some(g) => &after[g + 1..],
                None => "",
            }
        }
        None => "",
    }
}

/// ASCII case-insensitive search. Lowercasing ASCII keeps byte offsets valid.
fn find_ci(haystack: &str, needle_lower: &str) -> Option<usize> {
    haystack.to_ascii_lowercase().find(needle_lower)
}

fn push_text(out: &mut String, raw: &str) {
    if raw.is_empty() {
        return;
    }
    out.push_str(&decode_entities(raw));
}

pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let mut limit = rest.len().min(12);
        while !rest.is_char_boundary(limit) {
            limit -= 1;
        }
        let semi = rest[..limit].find(';');
        match semi {
            Some(end) => {
                let entity = &rest[1..end];
                match decode_one(entity) {
                    Some(c) => {
                        out.push(c);
                        rest = &rest[end + 1..];
                    }
                    None => {
                        out.push('&');
                        rest = &rest[1..];
                    }
                }
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn decode_one(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some(' '),
        "ndash" | "mdash" => Some('-'),
        "hellip" => Some('.'),
        "lsquo" | "rsquo" => Some('\''),
        "ldquo" | "rdquo" => Some('"'),
        "copy" => Some('c'),
        _ => {
            let num = entity.strip_prefix('#')?;
            let code = if let Some(hex) = num.strip_prefix(['x', 'X']) {
                u32::from_str_radix(hex, 16).ok()?
            } else {
                num.parse::<u32>().ok()?
            };
            char::from_u32(code)
        }
    }
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Trims each line, collapses inner whitespace, allows at most one blank line.
fn tidy(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut blank_run = 0usize;
    for line in s.lines() {
        let collapsed = collapse_ws(line);
        if collapsed.is_empty() {
            blank_run += 1;
            if blank_run == 1 && !out.is_empty() {
                out.push('\n');
            }
            continue;
        }
        blank_run = 0;
        out.push_str(&collapsed);
        out.push('\n');
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_tags_and_scripts() {
        let html = r#"<html><head><title>Hello &amp; welcome</title><style>p{color:red}</style></head>
        <body><h1>Heading</h1><p>Some <b>bold</b> text.</p><script>var x = "<p>not text</p>";</script>
        <!-- comment --><ul><li>one</li><li>two &#38; three</li></ul></body></html>"#;
        let e = html_to_text(html);
        assert_eq!(e.title.as_deref(), Some("Hello & welcome"));
        assert!(e.text.contains("Heading"));
        assert!(e.text.contains("Some bold text."));
        assert!(e.text.contains("two & three"));
        assert!(!e.text.contains("color:red"));
        assert!(!e.text.contains("not text"));
        assert!(!e.text.contains("comment"));
    }

    #[test]
    fn unterminated_input_does_not_panic() {
        let e = html_to_text("<div><p>open <script>alert(1)");
        assert!(e.text.contains("open"));
        let e2 = html_to_text("text & more &unknown; &#x41;");
        assert_eq!(e2.text, "text & more &unknown; A");
    }
}
