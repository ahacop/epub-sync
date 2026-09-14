//! The text of the spine documents, for measuring the book.

use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};
use roxmltree::Node;

use crate::opf;

/// The text of every document in `spine`, joined with newlines.
pub(crate) fn read(epub: &Path, spine: &[String]) -> Result<String> {
    let file = std::fs::File::open(epub).with_context(|| format!("open {}", epub.display()))?;
    let mut archive =
        zip::ZipArchive::new(file).with_context(|| format!("read {}", epub.display()))?;
    let mut text = String::new();
    for path in spine {
        let mut entry = archive
            .by_name(path)
            .with_context(|| format!("no {path} in the EPUB"))?;
        let mut xhtml = String::new();
        entry
            .read_to_string(&mut xhtml)
            .with_context(|| format!("read {path}"))?;
        text.push_str(&body_text(&xhtml).with_context(|| format!("parse {path}"))?);
        text.push('\n');
    }
    Ok(text)
}

/// The text of an XHTML document: every text node under `body`, with a
/// space between nodes. Text inside `script` and `style` is left out.
fn body_text(xhtml: &str) -> Result<String> {
    let xhtml = replace_html_entities(xhtml);
    let doc = opf::parse_xml(&xhtml)?;
    let mut out = String::new();
    if let Some(body) = doc
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "body")
    {
        collect_text(body, &mut out);
    }
    Ok(out)
}

fn collect_text(node: Node, out: &mut String) {
    for child in node.children() {
        if child.is_text() {
            out.push_str(child.text().unwrap_or(""));
            out.push(' ');
        } else if child.is_element() && !matches!(child.tag_name().name(), "script" | "style") {
            collect_text(child, out);
        }
    }
}

/// Replaces the named entities XHTML takes from its DTD, such as `&nbsp;`,
/// which an XML parser cannot resolve without the DTD. `&nbsp;` becomes a
/// space, so the words on both sides stay apart. Every other named entity
/// outside the five XML ones is dropped, which leaves a word such as
/// `caf&eacute;` one word short of a letter. The five XML entities and
/// numeric references stay for the parser.
fn replace_html_entities(xhtml: &str) -> String {
    let mut out = String::with_capacity(xhtml.len());
    let mut rest = xhtml;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let name_len = after
            .find(|c: char| !c.is_ascii_alphanumeric())
            .unwrap_or(after.len());
        let name = &after[..name_len];
        let is_named = !name.is_empty() && after[name_len..].starts_with(';');
        let is_xml = matches!(name, "amp" | "lt" | "gt" | "quot" | "apos") || name.starts_with('#');
        if is_named && !is_xml {
            if name == "nbsp" {
                out.push(' ');
            }
            rest = &after[name_len + 1..];
        } else {
            out.push('&');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_the_text_of_a_paragraph_with_inline_markup() {
        let text = body_text(
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>T</title></head>
            <body><p>It was <i>dark</i> and <b>stormy</b>.</p></body></html>"#,
        )
        .unwrap();
        assert_eq!(
            text.split_whitespace().collect::<Vec<_>>(),
            ["It", "was", "dark", "and", "stormy", "."]
        );
    }

    #[test]
    fn joins_two_paragraphs_with_a_space() {
        let text = body_text(
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>One two.</p><p>Three.</p></body></html>"#,
        )
        .unwrap();
        assert_eq!(text, "One two. Three. ");
    }

    #[test]
    fn leaves_out_script_and_style() {
        let text = body_text(
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><body>
            <style>p { color: red }</style><script>var x = 1;</script><p>Kept.</p></body></html>"#,
        )
        .unwrap();
        assert_eq!(text.trim(), "Kept.");
    }

    #[test]
    fn resolves_the_entities_the_dtd_would_define() {
        let text = body_text(concat!(
            r#"<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">"#,
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>Mr.&nbsp;Darcy &amp; caf&eacute; &#8212; &lt;end&gt;</p></body></html>"#,
        ))
        .unwrap();
        assert_eq!(text.trim(), "Mr. Darcy & caf \u{2014} <end>");
    }
}
