//! Measures the word count and the Flesch reading ease of a book. Import
//! measures a file that carries no word count.
//!
//! `readsight` counts the words of the book text and scores it with the
//! Flesch coefficients of the book's language.

use std::collections::HashMap;

use anyhow::Result;
use epubsync_epub::Epub;
use readsight::ReadSight;

pub use epubsync_epub::Stats;

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

/// Measures the book. Every book gets a word count. A book gets a reading
/// ease when `readsight` has Flesch coefficients for its `dc:language`. A
/// book with no `dc:language` is measured as English.
pub fn measure(epub: &Epub, engines: &mut Engines) -> Result<Stats> {
    let text = epub.text()?;
    let engine = engines.get(&readsight_code(epub.language()))?;
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
