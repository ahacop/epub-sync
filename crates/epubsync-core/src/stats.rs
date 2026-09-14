//! The word count and the Flesch reading ease of a book. Import measures a
//! file that carries no word count, and migration 3 measures the books
//! that were imported before the app measured.
//!
//! The text is every text node under `body` in every spine document, in
//! spine order. `readsight` counts the words and scores the text with the
//! Flesch coefficients of the book's language.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};
use readsight::ReadSight;
use roxmltree::Node;

use crate::opf::{self, Opf};

/// Numbers about the book's text. A Standard Ebooks file carries them in
/// its OPF. Every other file gets them measured at import, and the app
/// writes them into the file in the form Standard Ebooks uses.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Stats {
    pub word_count: Option<u64>,
    /// The Flesch reading ease score: 0 to 100, higher is easier.
    pub reading_ease: Option<f64>,
}

impl Stats {
    /// The numbers the file carries. An element whose text is not a number
    /// counts as absent.
    pub fn from_opf(opf: &Opf) -> Stats {
        Stats {
            word_count: opf
                .word_count
                .as_ref()
                .and_then(|e| e.value.trim().parse().ok()),
            reading_ease: opf
                .reading_ease
                .as_ref()
                .and_then(|e| e.value.trim().parse().ok()),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.word_count.is_none() && self.reading_ease.is_none()
    }
}

/// The `readsight` engine for one language: the word rule and, when the
/// crate scores the language, the Flesch coefficients.
struct Engine {
    readsight: ReadSight,
    /// False for a language the crate has no patterns for. Such a book is
    /// counted with the English word rule and gets no reading ease.
    scores: bool,
}

/// The engines built so far, one per language code. Building an engine
/// parses the TeX hyphenation patterns of its language, so a caller that
/// measures many books keeps one cache across them.
#[derive(Default)]
pub struct Engines {
    by_code: HashMap<String, Engine>,
}

impl Engines {
    fn get(&mut self, code: &str) -> Result<&Engine> {
        if !self.by_code.contains_key(code) {
            let engine = match ReadSight::new(code) {
                Ok(readsight) => Engine {
                    readsight,
                    scores: true,
                },
                Err(readsight::Error::UnsupportedLanguage(_)) => Engine {
                    readsight: ReadSight::new("en-us")?,
                    scores: false,
                },
                Err(e) => return Err(e.into()),
            };
            self.by_code.insert(code.to_string(), engine);
        }
        Ok(&self.by_code[code])
    }
}

/// Measures the EPUB at `epub`, whose parsed OPF is `opf`. Every book gets
/// a word count. A book gets a reading ease when `readsight` has Flesch
/// coefficients for its `dc:language`. A book with no `dc:language` is
/// measured as English.
pub fn measure(epub: &Path, opf: &Opf, engines: &mut Engines) -> Result<Stats> {
    let text = body_text_of(epub, opf)?;
    let engine = engines.get(&readsight_code(opf.language.as_deref()))?;
    let word_count = Some(engine.readsight.word_count(&text).max(0) as u64);
    let reading_ease = if engine.scores {
        match engine.readsight.flesch_reading_ease(&text) {
            Ok(result) => Some(result.score),
            Err(readsight::Error::UnsupportedFormula { .. } | readsight::Error::EmptyText) => None,
            Err(e) => return Err(e.into()),
        }
    } else {
        None
    };
    Ok(Stats {
        word_count,
        reading_ease,
    })
}

/// The text of every spine document, joined with newlines.
fn body_text_of(epub: &Path, opf: &Opf) -> Result<String> {
    let file = std::fs::File::open(epub).with_context(|| format!("open {}", epub.display()))?;
    let mut archive =
        zip::ZipArchive::new(file).with_context(|| format!("read {}", epub.display()))?;
    let mut text = String::new();
    for path in &opf.spine {
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

/// The `readsight` code for a `dc:language` value. The crate names its
/// languages after the TeX hyphenation patterns, which split English by
/// region and German by spelling reform.
fn readsight_code(language: Option<&str>) -> String {
    let language = language.unwrap_or("en").trim().to_ascii_lowercase();
    let base = language.split('-').next().unwrap_or("en");
    match (base, language.as_str()) {
        ("en", "en-gb") => "en-gb".to_string(),
        ("en", _) => "en-us".to_string(),
        ("de", _) => "de-1996".to_string(),
        (base, _) => base.to_string(),
    }
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

    #[test]
    fn maps_the_language_to_the_crate_code() {
        assert_eq!(readsight_code(Some("en")), "en-us");
        assert_eq!(readsight_code(Some("en-US")), "en-us");
        assert_eq!(readsight_code(Some("en-GB")), "en-gb");
        assert_eq!(readsight_code(Some("de")), "de-1996");
        assert_eq!(readsight_code(Some("pt-BR")), "pt");
        assert_eq!(readsight_code(Some("la")), "la");
        assert_eq!(readsight_code(None), "en-us");
    }

    #[test]
    fn a_language_without_flesch_coefficients_gets_no_score() {
        let latin = ReadSight::new(&readsight_code(Some("la"))).unwrap();
        assert!(matches!(
            latin.flesch_reading_ease("Gallia est omnis divisa in partes tres."),
            Err(readsight::Error::UnsupportedFormula { .. })
        ));
        assert!(matches!(
            ReadSight::new(&readsight_code(Some("ja"))),
            Err(readsight::Error::UnsupportedLanguage(_))
        ));
    }
}
