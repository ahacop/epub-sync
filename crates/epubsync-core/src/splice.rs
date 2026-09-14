//! Writes a metadata record into OPF text by replacing the byte ranges of
//! the elements the app owns. Every other byte stays as it was.
//!
//! Each owned element is replaced in place. A field the record has and the
//! file lacks is inserted after the last owned element, in the indentation
//! the file uses. A field the file has and the record lacks is removed
//! together with the whitespace before it.

use std::ops::Range;

use crate::metadata::{Metadata, format_series_number};
use crate::opf::{Creator, Element, NS_OPF, Opf, SeriesForm, Version, indent_before};

/// One change to the text: the bytes in `range` become `text`.
struct Edit {
    range: Range<usize>,
    text: String,
}

/// The form a creator's sort name is written in.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FileAsForm {
    Attribute,
    Meta,
    Both,
}

/// Returns the OPF text with `record` written into it.
pub fn splice(opf: &Opf, record: &Metadata) -> String {
    let text = &opf.text;
    let mut edits: Vec<Edit> = Vec::new();
    let mut inserts: Vec<String> = Vec::new();
    let mut needs_opf_prefix = false;
    let opf_prefix = opf.opf_prefix.clone().unwrap_or_else(|| "opf".to_string());
    let mut next_id = IdMaker::new(text);

    // Title.
    match &opf.title {
        Some(title) => edits.push(Edit {
            range: title.range.clone(),
            text: replace_text(text, title, &record.title),
        }),
        None => inserts.push(format!(
            "<{q}>{}</{q}>",
            escape_text(&record.title),
            q = dc_name(opf, "title")
        )),
    }

    // Creators, paired by position.
    let default_form = default_file_as_form(opf);
    for (i, author) in record.authors.iter().enumerate() {
        match opf.creators.get(i) {
            Some(creator) => {
                let form = match (
                    creator.file_as_attr().is_some(),
                    creator.file_as_meta().is_some(),
                ) {
                    (true, true) => FileAsForm::Both,
                    (true, false) => FileAsForm::Attribute,
                    (false, true) => FileAsForm::Meta,
                    (false, false) => default_form,
                };
                let (id, attrs) =
                    creator_attributes(creator, form, &opf_prefix, &author.sort, &mut next_id);
                if matches!(form, FileAsForm::Attribute | FileAsForm::Both)
                    && opf.opf_prefix.is_none()
                {
                    needs_opf_prefix = true;
                }
                edits.push(Edit {
                    range: creator.range.clone(),
                    text: format!(
                        "<{q}{attrs}>{}</{q}>",
                        escape_text(&author.name),
                        q = creator.qname
                    ),
                });
                match (creator.file_as_meta(), form) {
                    (Some(meta), _) => edits.push(Edit {
                        range: meta.range.clone(),
                        text: replace_text(text, meta, &author.sort),
                    }),
                    (None, FileAsForm::Meta) => {
                        // Insert the refinement right after the creator, in
                        // the creator's indentation.
                        edits.push(Edit {
                            range: creator.range.end..creator.range.end,
                            text: format!(
                                "{}{}",
                                indent_before(text, creator.range.start),
                                file_as_meta(&id, &author.sort)
                            ),
                        });
                    }
                    _ => {}
                }
            }
            None => {
                let q = dc_name(opf, "creator");
                let sort_attr = escape_attr(&author.sort);
                match default_form {
                    FileAsForm::Attribute | FileAsForm::Both => {
                        if opf.opf_prefix.is_none() {
                            needs_opf_prefix = true;
                        }
                        inserts.push(format!(
                            r#"<{q} {opf_prefix}:file-as="{sort_attr}" {opf_prefix}:role="aut">{}</{q}>"#,
                            escape_text(&author.name)
                        ));
                    }
                    FileAsForm::Meta => {
                        let id = next_id.make("creator");
                        inserts.push(format!(
                            r#"<{q} id="{id}">{}</{q}>"#,
                            escape_text(&author.name)
                        ));
                        inserts.push(file_as_meta(&id, &author.sort));
                    }
                }
            }
        }
    }
    for creator in opf.creators.iter().skip(record.authors.len()) {
        edits.push(remove(text, &creator.range));
        if let Some(meta) = creator.file_as_meta() {
            edits.push(remove(text, &meta.range));
        }
    }

    // Publisher and description.
    for (element, value, name) in [
        (&opf.publisher, &record.publisher, "publisher"),
        (&opf.description, &record.description, "description"),
    ] {
        match (element, value) {
            (Some(e), Some(v)) => edits.push(Edit {
                range: e.range.clone(),
                text: replace_text(text, e, v),
            }),
            (Some(e), None) => edits.push(remove(text, &e.range)),
            (None, Some(v)) => inserts.push(format!(
                "<{q}>{}</{q}>",
                escape_text(v),
                q = dc_name(opf, name)
            )),
            (None, None) => {}
        }
    }

    // Series, in the form the file has, else the Calibre form.
    match (&opf.series, &record.series) {
        (Some(series), Some(new)) => match &series.form {
            SeriesForm::Calibre { name, index } => {
                edits.push(Edit {
                    range: name.range.clone(),
                    text: calibre_series_meta(&new.name),
                });
                match (index, new.number) {
                    (Some(index), Some(n)) => edits.push(Edit {
                        range: index.range.clone(),
                        text: calibre_index_meta(n),
                    }),
                    (Some(index), None) => edits.push(remove(text, &index.range)),
                    (None, Some(n)) => edits.push(Edit {
                        range: name.range.end..name.range.end,
                        text: format!(
                            "{}{}",
                            indent_before(text, name.range.start),
                            calibre_index_meta(n)
                        ),
                    }),
                    (None, None) => {}
                }
            }
            SeriesForm::Collection {
                collection,
                id,
                group_position,
                ..
            } => {
                let mut collection_text = replace_text(text, collection, &new.name);
                match (group_position, new.number) {
                    (Some(pos), Some(n)) => edits.push(Edit {
                        range: pos.range.clone(),
                        text: replace_text(text, pos, &format_series_number(n)),
                    }),
                    (Some(pos), None) => edits.push(remove(text, &pos.range)),
                    (None, Some(n)) => {
                        // The refinement points at the collection's id. A
                        // collection without one is rewritten with a new id.
                        let id = match id {
                            Some(id) => id.clone(),
                            None => {
                                let id = next_id.make("collection");
                                collection_text = format!(
                                    r#"<{q} property="belongs-to-collection" id="{id}">{}</{q}>"#,
                                    escape_text(&new.name),
                                    q = collection.qname
                                );
                                id
                            }
                        };
                        edits.push(Edit {
                            range: collection.range.end..collection.range.end,
                            text: format!(
                                "{}{}",
                                indent_before(text, collection.range.start),
                                group_position_meta(&id, n)
                            ),
                        });
                    }
                    (None, None) => {}
                }
                edits.push(Edit {
                    range: collection.range.clone(),
                    text: collection_text,
                });
            }
        },
        (Some(series), None) => match &series.form {
            SeriesForm::Calibre { name, index } => {
                edits.push(remove(text, &name.range));
                if let Some(index) = index {
                    edits.push(remove(text, &index.range));
                }
            }
            SeriesForm::Collection {
                collection,
                collection_type,
                group_position,
                ..
            } => {
                edits.push(remove(text, &collection.range));
                for e in [collection_type, group_position].into_iter().flatten() {
                    edits.push(remove(text, &e.range));
                }
            }
        },
        (None, Some(new)) => {
            inserts.push(calibre_series_meta(&new.name));
            if let Some(n) = new.number {
                inserts.push(calibre_index_meta(n));
            }
        }
        (None, None) => {}
    }

    for insert in inserts {
        edits.push(Edit {
            range: opf.insert_at..opf.insert_at,
            text: format!("{}{}", opf.indent, insert),
        });
    }

    if needs_opf_prefix {
        edits.push(Edit {
            range: opf.package_tag_end..opf.package_tag_end,
            text: format!(r#" xmlns:{opf_prefix}="{NS_OPF}""#),
        });
    }

    apply(text, edits)
}

/// Applies the edits in text order. Edits do not overlap; inserts at one
/// offset keep the order they were added in.
fn apply(text: &str, mut edits: Vec<Edit>) -> String {
    edits.sort_by_key(|e| (e.range.start, e.range.end));
    let mut out = String::with_capacity(text.len() + 256);
    let mut pos = 0;
    for edit in edits {
        assert!(edit.range.start >= pos, "overlapping OPF edits");
        out.push_str(&text[pos..edit.range.start]);
        out.push_str(&edit.text);
        pos = edit.range.end;
    }
    out.push_str(&text[pos..]);
    out
}

/// An edit that removes an element and the whitespace before it.
fn remove(text: &str, range: &Range<usize>) -> Edit {
    let start = text[..range.start].trim_end().len();
    Edit {
        range: start..range.end,
        text: String::new(),
    }
}

/// The element with its text content replaced and its tags kept as
/// written. A self-closing element becomes an open and close pair.
fn replace_text(text: &str, element: &Element, value: &str) -> String {
    let source = &text[element.range.clone()];
    let escaped = escape_text(value);
    if source.ends_with("/>") {
        let start_tag = source.strip_suffix("/>").unwrap_or(source);
        return format!("{}>{escaped}</{}>", start_tag.trim_end(), element.qname);
    }
    let open_end = source.find('>').map(|i| i + 1).unwrap_or(source.len());
    let close_start = source.rfind("</").unwrap_or(source.len());
    format!("{}{escaped}{}", &source[..open_end], &source[close_start..])
}

/// The creator's attribute text with the sort name in the chosen form and
/// an id when the meta form needs one. Returns the id and the text.
fn creator_attributes(
    creator: &Creator,
    form: FileAsForm,
    opf_prefix: &str,
    sort: &str,
    ids: &mut IdMaker,
) -> (String, String) {
    let sort_attr = escape_attr(sort);
    let keeps_attr = matches!(form, FileAsForm::Attribute | FileAsForm::Both);
    let mut attrs: Vec<String> = Vec::new();
    for attr in &creator.attributes {
        let name = &attr.name;
        match (name.ends_with(":file-as"), keeps_attr) {
            (true, true) => attrs.push(format!(r#"{name}="{sort_attr}""#)),
            (true, false) => {}
            (false, _) => attrs.push(format!(r#"{name}="{}""#, escape_attr(&attr.value))),
        }
    }
    if keeps_attr && creator.file_as_attr().is_none() {
        attrs.push(format!(r#"{opf_prefix}:file-as="{sort_attr}""#));
    }
    let id = match &creator.id {
        Some(id) => id.id.clone(),
        None if form == FileAsForm::Meta => {
            let id = ids.make("creator");
            attrs.insert(0, format!(r#"id="{id}""#));
            id
        }
        None => String::new(),
    };
    let text = attrs.iter().map(|a| format!(" {a}")).collect::<String>();
    (id, text)
}

/// The sort name form for a creator the file gives none for: the form the
/// file's other creators use, else the form of the package version.
fn default_file_as_form(opf: &Opf) -> FileAsForm {
    for c in &opf.creators {
        match (c.file_as_attr().is_some(), c.file_as_meta().is_some()) {
            (true, true) => return FileAsForm::Both,
            (true, false) => return FileAsForm::Attribute,
            (false, true) => return FileAsForm::Meta,
            (false, false) => {}
        }
    }
    match opf.version {
        Version::Epub2 => FileAsForm::Attribute,
        Version::Epub3 => FileAsForm::Meta,
    }
}

fn dc_name(opf: &Opf, local: &str) -> String {
    match &opf.dc_prefix {
        Some(p) => format!("{p}:{local}"),
        None => local.to_string(),
    }
}

fn file_as_meta(id: &str, sort: &str) -> String {
    format!(
        r##"<meta refines="#{id}" property="file-as">{}</meta>"##,
        escape_text(sort)
    )
}

fn group_position_meta(id: &str, n: f64) -> String {
    format!(
        r##"<meta refines="#{id}" property="group-position">{}</meta>"##,
        format_series_number(n)
    )
}

fn calibre_series_meta(name: &str) -> String {
    format!(
        r#"<meta name="calibre:series" content="{}"/>"#,
        escape_attr(name)
    )
}

fn calibre_index_meta(n: f64) -> String {
    format!(
        r#"<meta name="calibre:series_index" content="{}"/>"#,
        format_series_number(n)
    )
}

/// Makes ids that no element in the text already uses.
struct IdMaker<'a> {
    text: &'a str,
    n: usize,
}

impl<'a> IdMaker<'a> {
    fn new(text: &'a str) -> Self {
        IdMaker { text, n: 0 }
    }

    fn make(&mut self, stem: &str) -> String {
        loop {
            self.n += 1;
            let id = format!("{stem}{}", self.n);
            if !self.text.contains(&format!(r#"id="{id}""#)) {
                return id;
            }
        }
    }
}

pub fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

pub fn escape_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}
