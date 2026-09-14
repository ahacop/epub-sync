//! Reads the OPF of an EPUB 2 or EPUB 3 file and returns the metadata
//! fields with the byte range of each element in the OPF text.
//!
//! Import uses the fields. Edit uses the ranges to splice new text in
//! place, so the module matches elements by namespace URI and records where
//! each owned element sits in the text.

use std::io::Read;
use std::ops::Range;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use roxmltree::{Document, Node, ParsingOptions};

pub const NS_OPF: &str = "http://www.idpf.org/2007/opf";
pub const NS_DC: &str = "http://purl.org/dc/elements/1.1/";
const NS_CONTAINER: &str = "urn:oasis:names:tc:opendocument:xmlns:container";

/// The package version of the file. The app never changes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    Epub2,
    Epub3,
}

/// One element the app owns: its text value, its byte range in the OPF
/// text, and its qualified name as written in the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub value: String,
    pub range: Range<usize>,
    pub qname: String,
}

/// One `dc:creator` with both forms of its sort name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Creator {
    pub name: String,
    pub range: Range<usize>,
    pub qname: String,
    pub id: Option<String>,
    /// Every attribute of the element as written, in file order. The
    /// `opf:file-as` attribute is included here and flagged in `file_as_attr`.
    pub attributes: Vec<String>,
    /// The `opf:file-as` attribute value, the EPUB 2 form.
    pub file_as_attr: Option<String>,
    /// The `meta refines property="file-as"` element, the EPUB 3 form.
    pub file_as_meta: Option<Element>,
}

impl Creator {
    /// The sort name the file gives, from the attribute else the refinement.
    pub fn sort(&self) -> Option<&str> {
        self.file_as_attr
            .as_deref()
            .or(self.file_as_meta.as_ref().map(|m| m.value.as_str()))
    }
}

/// The series elements in the form the file has.
#[derive(Debug, Clone, PartialEq)]
pub enum SeriesForm {
    /// `meta name="calibre:series"` and `meta name="calibre:series_index"`.
    Calibre {
        name: Element,
        index: Option<Element>,
    },
    /// `meta property="belongs-to-collection"` with its refinements.
    Collection {
        collection: Element,
        id: String,
        collection_type: Option<Element>,
        group_position: Option<Element>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    pub name: String,
    pub number: Option<f64>,
    pub form: SeriesForm,
}

/// The parsed OPF: the text, the fields, and where each field sits.
#[derive(Debug, Clone)]
pub struct Opf {
    /// Path of the OPF inside the zip.
    pub path: String,
    pub text: String,
    pub version: Version,
    pub title: Option<Element>,
    pub creators: Vec<Creator>,
    pub publisher: Option<Element>,
    pub description: Option<Element>,
    pub language: Option<String>,
    pub series: Option<Series>,
    /// Path of the cover image inside the zip.
    pub cover_path: Option<String>,
    /// Byte offset after the last element the app owns. Inserts go here.
    pub insert_at: usize,
    /// The newline and indentation before an owned element, for inserts.
    pub indent: String,
    /// The prefix declared for the OPF namespace on `package` or `metadata`,
    /// when there is one. The EPUB 2 `file-as` attribute needs it.
    pub opf_prefix: Option<String>,
    /// The prefix of the Dublin Core namespace as the title uses it. `None`
    /// means the default namespace form.
    pub dc_prefix: Option<String>,
    /// Byte offset of the `>` that ends the `package` start tag.
    pub package_tag_end: usize,
}

/// Opens the EPUB, finds the OPF through `META-INF/container.xml`, and
/// parses it.
pub fn read(epub: &Path) -> Result<Opf> {
    let file = std::fs::File::open(epub).with_context(|| format!("open {}", epub.display()))?;
    let mut archive =
        zip::ZipArchive::new(file).with_context(|| format!("read {}", epub.display()))?;
    let container = read_entry(&mut archive, "META-INF/container.xml")?;
    let opf_path = rootfile_path(&container)?;
    let text = read_entry(&mut archive, &opf_path)?;
    parse(&opf_path, text)
}

fn read_entry<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<String> {
    let mut entry = archive
        .by_name(name)
        .with_context(|| format!("no {name} in the EPUB"))?;
    let mut text = String::new();
    entry
        .read_to_string(&mut text)
        .with_context(|| format!("read {name}"))?;
    Ok(text)
}

/// Parses an XML document that may start with a DOCTYPE. Some publishers put
/// a DOCTYPE line in `container.xml` and the OPF. roxmltree rejects DOCTYPEs
/// unless told otherwise, and the entity expansion it guards against is not
/// a concern for files the user places in their own library.
fn parse_xml(text: &str) -> Result<Document<'_>> {
    let options = ParsingOptions {
        allow_dtd: true,
        ..ParsingOptions::default()
    };
    Ok(Document::parse_with_options(text, options)?)
}

/// Returns the `full-path` of the first `rootfile` in `container.xml`.
pub fn rootfile_path(container: &str) -> Result<String> {
    let doc = parse_xml(container).context("parse META-INF/container.xml")?;
    doc.descendants()
        .find(|n| {
            n.is_element()
                && n.tag_name().name() == "rootfile"
                && n.tag_name().namespace() == Some(NS_CONTAINER)
        })
        .and_then(|n| n.attribute("full-path"))
        .map(str::to_string)
        .ok_or_else(|| anyhow!("META-INF/container.xml names no rootfile"))
}

/// Parses OPF text. `path` is the OPF path inside the zip and is used to
/// resolve the cover path.
pub fn parse(path: &str, text: String) -> Result<Opf> {
    let doc = parse_xml(&text).context("parse the OPF")?;
    let package = doc.root_element();
    if package.tag_name().name() != "package" {
        bail!(
            "the OPF root element is {}, not package",
            package.tag_name().name()
        );
    }
    let version = match package.attribute("version") {
        Some(v) if v.starts_with('2') => Version::Epub2,
        _ => Version::Epub3,
    };
    let metadata = package
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "metadata")
        .ok_or_else(|| anyhow!("the OPF has no metadata element"))?;

    let title = pick_title(&metadata, &text);

    let creators = dc_children(&metadata, "creator")
        .map(|node| {
            let id = node.attribute("id").map(str::to_string);
            let file_as_meta = id
                .as_deref()
                .and_then(|id| refinement(&metadata, id, "file-as", &text));
            Creator {
                name: node_text(&node),
                range: node.range(),
                qname: qname(&node, &text),
                id,
                attributes: node
                    .attributes()
                    .map(|a| text[a.range()].to_string())
                    .collect(),
                file_as_attr: node.attribute((NS_OPF, "file-as")).map(str::to_string),
                file_as_meta,
            }
        })
        .collect();

    let publisher = dc_children(&metadata, "publisher")
        .next()
        .map(|n| element(&n, &text));
    let description = dc_children(&metadata, "description")
        .next()
        .map(|n| element(&n, &text));
    let language = dc_children(&metadata, "language")
        .next()
        .map(|n| node_text(&n));
    let series = read_series(&metadata, &text);
    let cover_path = cover_path(&package, &metadata, path);

    let opf_prefix = [package, metadata]
        .iter()
        .flat_map(|n| n.namespaces())
        .find(|ns| ns.uri() == NS_OPF && ns.name().is_some())
        .and_then(|ns| ns.name().map(str::to_string));
    let dc_prefix = title
        .as_ref()
        .and_then(|t| t.qname.split_once(':').map(|(p, _)| p.to_string()));
    let package_tag_end = text[package.range().start..]
        .find('>')
        .map(|i| package.range().start + i)
        .ok_or_else(|| anyhow!("the package start tag has no end"))?;
    // Where an insert goes when the file has no owned element: just
    // before the metadata close tag.
    let metadata_close = text[..metadata.range().end]
        .rfind("</")
        .unwrap_or(metadata.range().end);

    let mut opf = Opf {
        path: path.to_string(),
        text,
        version,
        title,
        creators,
        publisher,
        description,
        language,
        series,
        cover_path,
        insert_at: metadata_close,
        indent: String::new(),
        opf_prefix,
        dc_prefix,
        package_tag_end,
    };
    let owned = opf.owned_ranges();
    if let Some(last) = owned.last() {
        opf.insert_at = last.end;
    }
    let indent_from = owned.first().map(|r| r.start).unwrap_or(opf.insert_at);
    opf.indent = indent_before(&opf.text, indent_from);
    Ok(opf)
}

impl Opf {
    /// The byte ranges of every element the app owns, in text order.
    pub fn owned_ranges(&self) -> Vec<Range<usize>> {
        let mut ranges = Vec::new();
        if let Some(t) = &self.title {
            ranges.push(t.range.clone());
        }
        for c in &self.creators {
            ranges.push(c.range.clone());
            if let Some(m) = &c.file_as_meta {
                ranges.push(m.range.clone());
            }
        }
        for e in [&self.publisher, &self.description].into_iter().flatten() {
            ranges.push(e.range.clone());
        }
        if let Some(s) = &self.series {
            match &s.form {
                SeriesForm::Calibre { name, index } => {
                    ranges.push(name.range.clone());
                    ranges.extend(index.iter().map(|e| e.range.clone()));
                }
                SeriesForm::Collection {
                    collection,
                    collection_type,
                    group_position,
                    ..
                } => {
                    ranges.push(collection.range.clone());
                    ranges.extend(collection_type.iter().map(|e| e.range.clone()));
                    ranges.extend(group_position.iter().map(|e| e.range.clone()));
                }
            }
        }
        ranges.sort_by_key(|r| r.start);
        ranges
    }
}

fn dc_children<'a, 'input>(
    metadata: &Node<'a, 'input>,
    name: &'static str,
) -> impl Iterator<Item = Node<'a, 'input>> {
    metadata.children().filter(move |n| {
        n.is_element() && n.tag_name().namespace() == Some(NS_DC) && n.tag_name().name() == name
    })
}

fn metas<'a, 'input>(metadata: &Node<'a, 'input>) -> impl Iterator<Item = Node<'a, 'input>> {
    metadata.children().filter(|n| {
        n.is_element()
            && n.tag_name().name() == "meta"
            && n.tag_name().namespace().is_none_or(|ns| ns == NS_OPF)
    })
}

fn node_text(node: &Node) -> String {
    node.text().unwrap_or("").trim().to_string()
}

fn qname(node: &Node, text: &str) -> String {
    let start = node.range().start + 1;
    let rest = &text[start..];
    let end = rest
        .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
        .unwrap_or(rest.len());
    rest[..end].to_string()
}

fn element(node: &Node, text: &str) -> Element {
    Element {
        value: node_text(node),
        range: node.range(),
        qname: qname(node, text),
    }
}

/// The `meta refines="#id" property="..."` child of `metadata`, when there is one.
fn refinement(metadata: &Node, id: &str, property: &str, text: &str) -> Option<Element> {
    let target = format!("#{id}");
    metas(metadata)
        .find(|m| {
            m.attribute("refines") == Some(target.as_str())
                && m.attribute("property") == Some(property)
        })
        .map(|m| element(&m, text))
}

/// The main title: the one refined with `title-type` "main", else the first.
fn pick_title(metadata: &Node, text: &str) -> Option<Element> {
    let titles: Vec<Node> = dc_children(metadata, "title").collect();
    let main = titles.iter().find(|t| {
        t.attribute("id")
            .and_then(|id| refinement(metadata, id, "title-type", text))
            .is_some_and(|r| r.value == "main")
    });
    main.or(titles.first()).map(|n| element(n, text))
}

/// The series in the Calibre form, else the EPUB 3 collection form.
fn read_series(metadata: &Node, text: &str) -> Option<Series> {
    let calibre_name = metas(metadata).find(|m| m.attribute("name") == Some("calibre:series"));
    if let Some(name_node) = calibre_name {
        let index_node =
            metas(metadata).find(|m| m.attribute("name") == Some("calibre:series_index"));
        let name = meta_content(&name_node, text);
        let index = index_node.map(|n| meta_content(&n, text));
        return Some(Series {
            name: name.value.clone(),
            number: index.as_ref().and_then(|i| i.value.trim().parse().ok()),
            form: SeriesForm::Calibre { name, index },
        });
    }

    let collections: Vec<Node> = metas(metadata)
        .filter(|m| m.attribute("property") == Some("belongs-to-collection"))
        .collect();
    let pick = collections
        .iter()
        .find(|c| {
            c.attribute("id")
                .and_then(|id| refinement(metadata, id, "collection-type", text))
                .is_some_and(|t| t.value == "series")
        })
        .or(collections.first())?;
    let id = pick.attribute("id").unwrap_or("").to_string();
    let collection_type = refinement(metadata, &id, "collection-type", text);
    let group_position = refinement(metadata, &id, "group-position", text);
    let collection = element(pick, text);
    Some(Series {
        name: collection.value.clone(),
        number: group_position
            .as_ref()
            .and_then(|g| g.value.trim().parse().ok()),
        form: SeriesForm::Collection {
            collection,
            id,
            collection_type,
            group_position,
        },
    })
}

fn meta_content(node: &Node, text: &str) -> Element {
    Element {
        value: node.attribute("content").unwrap_or("").to_string(),
        range: node.range(),
        qname: qname(node, text),
    }
}

/// The cover image path: the manifest item with the `cover-image` property,
/// else the item named by `meta name="cover"`.
fn cover_path(package: &Node, metadata: &Node, opf_path: &str) -> Option<String> {
    let manifest = package
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "manifest")?;
    let items = || {
        manifest
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "item")
    };
    let by_property = items().find(|i| {
        i.attribute("properties")
            .is_some_and(|p| p.split_whitespace().any(|w| w == "cover-image"))
    });
    let item = by_property.or_else(|| {
        let id = metas(metadata)
            .find(|m| m.attribute("name") == Some("cover"))
            .and_then(|m| m.attribute("content"))?;
        items().find(|i| i.attribute("id") == Some(id))
    })?;
    let href = item.attribute("href")?;
    Some(join_path(opf_path, href))
}

/// Joins an href to the folder of the OPF, with `..` segments resolved.
fn join_path(opf_path: &str, href: &str) -> String {
    let mut parts: Vec<&str> = match opf_path.rsplit_once('/') {
        Some((dir, _)) => dir.split('/').collect(),
        None => Vec::new(),
    };
    for seg in href.split('/') {
        match seg {
            "." | "" => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// The newline and the spaces or tabs between it and `at`. When `at` does
/// not follow a newline and indentation, returns a newline and two spaces.
pub(crate) fn indent_before(text: &str, at: usize) -> String {
    let before = &text[..at];
    let ws_start = before.trim_end_matches([' ', '\t']).len();
    if before[..ws_start].ends_with('\n') {
        format!("\n{}", &before[ws_start..])
    } else {
        "\n  ".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_hrefs_to_the_opf_folder() {
        assert_eq!(
            join_path("OEBPS/content.opf", "images/cover.jpg"),
            "OEBPS/images/cover.jpg"
        );
        assert_eq!(join_path("content.opf", "cover.jpg"), "cover.jpg");
        assert_eq!(join_path("a/b/content.opf", "../cover.jpg"), "a/cover.jpg");
    }
}
