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
    /// Every attribute as written, in file order.
    pub attributes: Vec<Attribute>,
    /// The id, and the `file-as` refinement that points at it.
    pub id: Option<CreatorId>,
}

/// One attribute of an element, with its name as written in the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    /// The qualified name as written, such as `opf:file-as`.
    pub name: String,
    pub value: String,
}

/// A creator's id and the refinement that points at it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatorId {
    pub id: String,
    /// The `meta refines property="file-as"` element, the EPUB 3 form.
    pub file_as_meta: Option<Element>,
}

impl Creator {
    /// The `opf:file-as` attribute value, the EPUB 2 form.
    pub fn file_as_attr(&self) -> Option<&str> {
        self.attributes
            .iter()
            .find(|a| a.name.ends_with(":file-as"))
            .map(|a| a.value.as_str())
    }

    /// The `file-as` refinement, the EPUB 3 form.
    pub fn file_as_meta(&self) -> Option<&Element> {
        self.id.as_ref()?.file_as_meta.as_ref()
    }

    /// The sort name the file gives, from the attribute else the refinement.
    pub fn sort(&self) -> Option<&str> {
        self.file_as_attr()
            .or(self.file_as_meta().map(|m| m.value.as_str()))
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
    /// `meta property="belongs-to-collection"` with its refinements. The
    /// refinements point at the id, so a collection without one has none.
    Collection {
        collection: Element,
        id: Option<String>,
        collection_type: Option<Element>,
        group_position: Option<Element>,
    },
}

/// The series the file has. The name and the number are read out of the
/// elements in `form`.
#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    pub form: SeriesForm,
}

impl Series {
    pub fn name(&self) -> &str {
        match &self.form {
            SeriesForm::Calibre { name, .. } => &name.value,
            SeriesForm::Collection { collection, .. } => &collection.value,
        }
    }

    /// The number, when the file has one and it parses.
    pub fn number(&self) -> Option<f64> {
        let element = match &self.form {
            SeriesForm::Calibre { index, .. } => index.as_ref(),
            SeriesForm::Collection { group_position, .. } => group_position.as_ref(),
        };
        element.and_then(|e| e.value.trim().parse().ok())
    }
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
    /// The `meta property="schema:wordCount"` element, the form Standard
    /// Ebooks writes. Import measures a file that has none and writes it.
    pub word_count: Option<Element>,
    /// The `meta property="schema:educationalLevel"` element, which Standard
    /// Ebooks uses for the Flesch reading ease score despite the name: 0 to
    /// 100, higher is easier. Import writes it next to the word count.
    pub reading_ease: Option<Element>,
    /// Paths inside the zip of the XHTML spine documents in reading order,
    /// without the nav document. Measuring reads them.
    pub spine: Vec<String>,
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
pub(crate) fn parse_xml(text: &str) -> Result<Document<'_>> {
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

    let creators: Vec<Creator> = dc_children(&metadata, "creator")
        .map(|node| Creator {
            name: node_text(&node),
            range: node.range(),
            qname: qname(&node, &text),
            attributes: node.attributes().map(|a| attribute(&a, &text)).collect(),
            id: node.attribute("id").map(|id| CreatorId {
                id: id.to_string(),
                file_as_meta: refinement(&metadata, id, "file-as", &text),
            }),
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
    let word_count = meta_property(&metadata, "schema:wordCount", &text);
    let reading_ease = meta_property(&metadata, "schema:educationalLevel", &text);
    let cover_path = cover_path(&package, &metadata, path);
    let spine = spine_paths(&package, path);

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
    // Inserts go after the last owned element. A file with no owned
    // element takes them just before the metadata close tag.
    let owned = owned_ranges(
        title.as_ref(),
        &creators,
        publisher.as_ref(),
        description.as_ref(),
        series.as_ref(),
        [word_count.as_ref(), reading_ease.as_ref()],
    );
    let insert_at = owned.last().map(|r| r.end).unwrap_or_else(|| {
        text[..metadata.range().end]
            .rfind("</")
            .unwrap_or(metadata.range().end)
    });
    let indent_from = owned.first().map(|r| r.start).unwrap_or(insert_at);
    let indent = indent_before(&text, indent_from);

    Ok(Opf {
        path: path.to_string(),
        text,
        version,
        title,
        creators,
        publisher,
        description,
        language,
        series,
        word_count,
        reading_ease,
        spine,
        cover_path,
        insert_at,
        indent,
        opf_prefix,
        dc_prefix,
        package_tag_end,
    })
}

impl Opf {
    /// The byte ranges of every element the app owns, in text order.
    pub fn owned_ranges(&self) -> Vec<Range<usize>> {
        owned_ranges(
            self.title.as_ref(),
            &self.creators,
            self.publisher.as_ref(),
            self.description.as_ref(),
            self.series.as_ref(),
            [self.word_count.as_ref(), self.reading_ease.as_ref()],
        )
    }
}

/// The byte ranges of the given owned elements, in text order.
fn owned_ranges(
    title: Option<&Element>,
    creators: &[Creator],
    publisher: Option<&Element>,
    description: Option<&Element>,
    series: Option<&Series>,
    stats: [Option<&Element>; 2],
) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    if let Some(t) = title {
        ranges.push(t.range.clone());
    }
    for c in creators {
        ranges.push(c.range.clone());
        if let Some(m) = c.file_as_meta() {
            ranges.push(m.range.clone());
        }
    }
    for e in [publisher, description].into_iter().flatten() {
        ranges.push(e.range.clone());
    }
    if let Some(s) = series {
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
    ranges.extend(stats.into_iter().flatten().map(|e| e.range.clone()));
    ranges.sort_by_key(|r| r.start);
    ranges
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

/// The attribute with its name as written, prefix and all. roxmltree
/// gives the local name and the namespace, so the name comes from the
/// attribute's text.
fn attribute(attr: &roxmltree::Attribute, text: &str) -> Attribute {
    let raw = &text[attr.range()];
    let name = raw.split('=').next().unwrap_or(raw).trim_end();
    Attribute {
        name: name.to_string(),
        value: attr.value().to_string(),
    }
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
    let id = pick.attribute("id").map(str::to_string);
    let collection_type = id
        .as_deref()
        .and_then(|id| refinement(metadata, id, "collection-type", text));
    let group_position = id
        .as_deref()
        .and_then(|id| refinement(metadata, id, "group-position", text));
    let collection = element(pick, text);
    Some(Series {
        form: SeriesForm::Collection {
            collection,
            id,
            collection_type,
            group_position,
        },
    })
}

/// The first `meta property="..."` that refines nothing, when there is one.
fn meta_property(metadata: &Node, property: &str, text: &str) -> Option<Element> {
    metas(metadata)
        .find(|m| m.attribute("property") == Some(property) && m.attribute("refines").is_none())
        .map(|m| element(&m, text))
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

/// The zip paths of the spine documents in reading order: every `itemref`
/// whose manifest item is XHTML, without the item that has the `nav`
/// property and without the blank page kepubify puts first in a converted
/// file, which is not book text.
fn spine_paths(package: &Node, opf_path: &str) -> Vec<String> {
    let child = |name: &str| {
        package
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == name)
    };
    let (Some(manifest), Some(spine)) = (child("manifest"), child("spine")) else {
        return Vec::new();
    };
    let item = |id: &str| {
        manifest.children().find(|n| {
            n.is_element() && n.tag_name().name() == "item" && n.attribute("id") == Some(id)
        })
    };
    spine
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "itemref")
        .filter_map(|r| item(r.attribute("idref")?))
        .filter(|i| i.attribute("media-type") == Some("application/xhtml+xml"))
        .filter(|i| i.attribute("id") != Some("kepubify-titlepage-dummy"))
        .filter(|i| {
            !i.attribute("properties")
                .is_some_and(|p| p.split_whitespace().any(|w| w == "nav"))
        })
        .filter_map(|i| i.attribute("href"))
        .map(|href| join_path(opf_path, href))
        .collect()
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
