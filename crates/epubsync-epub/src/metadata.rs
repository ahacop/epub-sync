//! The metadata record and the text numbers: the fields the library and
//! the file both hold.

use serde::{Deserialize, Serialize};

use crate::opf::Opf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Author {
    pub name: String,
    pub sort: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Series {
    pub name: String,
    pub number: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Metadata {
    pub title: String,
    pub authors: Vec<Author>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub series: Option<Series>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl Metadata {
    /// Builds the record from a parsed OPF. `make_sort` gives the sort name
    /// for a creator the file has none for.
    pub(crate) fn from_opf(opf: &Opf, make_sort: impl Fn(&str) -> String) -> Metadata {
        Metadata {
            title: opf
                .title
                .as_ref()
                .map(|t| t.value.clone())
                .unwrap_or_default(),
            authors: opf
                .creators
                .iter()
                .map(|c| Author {
                    name: c.name.clone(),
                    sort: c
                        .sort()
                        .map(str::to_string)
                        .unwrap_or_else(|| make_sort(&c.name)),
                })
                .collect(),
            series: opf.series.as_ref().map(|s| Series {
                name: s.name().to_string(),
                number: s.number(),
            }),
            publisher: opf
                .publisher
                .as_ref()
                .map(|p| p.value.clone())
                .filter(|p| !p.is_empty()),
            description: opf
                .description
                .as_ref()
                .map(|d| d.value.clone())
                .filter(|d| !d.is_empty()),
        }
    }
}

/// Numbers about the book's text. A Standard Ebooks file carries them in
/// its OPF as `schema:wordCount` and `schema:educationalLevel`. Every
/// other file gets them measured at import, and the app writes them into
/// the file in that same form.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Stats {
    pub word_count: Option<u64>,
    /// The Flesch reading ease score: 0 to 100, higher is easier.
    pub reading_ease: Option<f64>,
}

impl Stats {
    /// The numbers the file carries. An element whose text is not a number
    /// counts as absent.
    pub(crate) fn from_opf(opf: &Opf) -> Stats {
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

    pub fn is_empty(&self) -> bool {
        self.word_count.is_none() && self.reading_ease.is_none()
    }
}

/// Formats a series number the way Calibre does: no decimals for a whole
/// number, else as written.
pub fn format_series_number(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{n:.0}")
    } else {
        n.to_string()
    }
}
