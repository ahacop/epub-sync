use super::*;
use crate::fixtures as common;
use crate::metadata::Stats;
use crate::opf;

fn parse(text: &str) -> Opf {
    opf::parse(common::OPF_PATH, text.to_string()).unwrap()
}

/// Checks that an element's range slices exactly `expected` out of the text.
fn assert_slice(opf: &Opf, element: &Element, expected: &str) {
    assert_eq!(&opf.text[element.range.clone()], expected);
}

#[test]
fn reads_through_the_zip() {
    let dir = tempfile::tempdir().unwrap();
    let path = common::write_epub(dir.path(), "book.epub", common::EPUB2_OPF);
    let opf = opf::read(&path).unwrap();
    assert_eq!(opf.path, common::OPF_PATH);
    assert_eq!(opf.text, common::EPUB2_OPF);
    assert_eq!(opf.title.unwrap().value, "The Left Hand of Darkness");
}

#[test]
fn reads_epub2_from_calibre() {
    let opf = parse(common::EPUB2_OPF);
    assert_eq!(opf.version, Version::Epub2);
    assert_eq!(opf.opf_prefix.as_deref(), Some("opf"));
    assert_eq!(opf.dc_prefix.as_deref(), Some("dc"));

    let title = opf.title.as_ref().unwrap();
    assert_eq!(title.value, "The Left Hand of Darkness");
    assert_eq!(title.qname, "dc:title");
    assert_slice(
        &opf,
        title,
        "<dc:title>The Left Hand of Darkness</dc:title>",
    );

    assert_eq!(opf.creators.len(), 1);
    let creator = &opf.creators[0];
    assert_eq!(creator.name, "Ursula K. Le Guin");
    assert_eq!(creator.file_as_attr(), Some("Le Guin, Ursula K."));
    assert!(creator.id.is_none());
    assert_eq!(creator.sort(), Some("Le Guin, Ursula K."));
    assert_eq!(
        creator.attributes,
        vec![
            Attribute {
                name: "opf:file-as".into(),
                value: "Le Guin, Ursula K.".into()
            },
            Attribute {
                name: "opf:role".into(),
                value: "aut".into()
            },
        ]
    );
    assert_eq!(
        &opf.text[creator.range.clone()],
        r##"<dc:creator opf:file-as="Le Guin, Ursula K." opf:role="aut">Ursula K. Le Guin</dc:creator>"##
    );

    assert_slice(
        &opf,
        opf.publisher.as_ref().unwrap(),
        "<dc:publisher>Ace Books</dc:publisher>",
    );
    let description = opf.description.as_ref().unwrap();
    assert_eq!(description.value, "<p>A novel of Winter.</p>");
    assert_slice(
        &opf,
        description,
        "<dc:description>&lt;p&gt;A novel of Winter.&lt;/p&gt;</dc:description>",
    );
    assert_eq!(opf.language.as_deref(), Some("en"));

    let series = opf.series.as_ref().unwrap();
    assert_eq!(series.name(), "Hainish Cycle");
    assert_eq!(series.number(), Some(4.0));
    let (name, series_index) = match &series.form {
        SeriesForm::Calibre { name, index } => (name, index),
        other => panic!("expected the Calibre form, got {other:?}"),
    };
    assert_slice(
        &opf,
        name,
        r##"<meta name="calibre:series" content="Hainish Cycle"/>"##,
    );
    assert_slice(
        &opf,
        series_index.as_ref().unwrap(),
        r##"<meta name="calibre:series_index" content="4"/>"##,
    );

    assert_eq!(opf.cover_path.as_deref(), Some("OEBPS/cover.jpg"));
    assert_eq!(opf.insert_at, series_index.as_ref().unwrap().range.end);
    assert_eq!(opf.indent, "\n    ");
}

#[test]
fn reads_epub3_with_refinements_and_collection() {
    let opf = parse(common::EPUB3_OPF);
    assert_eq!(opf.version, Version::Epub3);
    assert_eq!(opf.opf_prefix, None);

    let creator = &opf.creators[0];
    let CreatorId { id, file_as_meta } = creator.id.as_ref().unwrap();
    assert_eq!(id, "creator01");
    assert!(creator.file_as_attr().is_none());
    let meta = file_as_meta.as_ref().unwrap();
    assert_eq!(meta.value, "Le Guin, Ursula K.");
    assert_slice(
        &opf,
        meta,
        r##"<meta refines="#creator01" property="file-as">Le Guin, Ursula K.</meta>"##,
    );
    assert_eq!(creator.sort(), Some("Le Guin, Ursula K."));

    let series = opf.series.as_ref().unwrap();
    assert_eq!(series.name(), "Earthsea Cycle");
    assert_eq!(series.number(), Some(1.0));
    let (collection, id, collection_type, group_position) = match &series.form {
        SeriesForm::Collection {
            collection,
            id,
            collection_type,
            group_position,
        } => (collection, id, collection_type, group_position),
        other => panic!("expected the collection form, got {other:?}"),
    };
    assert_eq!(id.as_deref(), Some("c01"));
    assert_slice(
        &opf,
        collection,
        r##"<meta property="belongs-to-collection" id="c01">Earthsea Cycle</meta>"##,
    );
    assert_slice(
        &opf,
        collection_type.as_ref().unwrap(),
        r##"<meta refines="#c01" property="collection-type">series</meta>"##,
    );
    assert_slice(
        &opf,
        group_position.as_ref().unwrap(),
        r##"<meta refines="#c01" property="group-position">1</meta>"##,
    );

    assert_eq!(opf.cover_path.as_deref(), Some("OEBPS/cover.jpg"));
    assert_eq!(opf.insert_at, group_position.as_ref().unwrap().range.end);
}

#[test]
fn reads_both_file_as_forms_on_one_creator() {
    let opf = parse(common::EPUB3_CALIBRE_OPF);
    let creator = &opf.creators[0];
    assert_eq!(creator.file_as_attr(), Some("Le Guin, Ursula K."));
    assert_eq!(creator.file_as_meta().unwrap().value, "Le Guin, Ursula K.");
    assert_eq!(opf.publisher.as_ref().unwrap().value, "Harper & Row");
    assert!(matches!(
        opf.series.as_ref().unwrap().form,
        SeriesForm::Calibre { .. }
    ));
    assert_eq!(opf.series.as_ref().unwrap().number(), Some(5.0));
}

#[test]
fn reads_a_description_in_default_namespace_form() {
    let opf = parse(common::DEFAULT_NS_DESCRIPTION_OPF);
    let description = opf.description.as_ref().unwrap();
    assert_eq!(description.value, "Dreams that change the world.");
    assert_eq!(description.qname, "description");
    assert_slice(
        &opf,
        description,
        r##"<description xmlns="http://purl.org/dc/elements/1.1/">Dreams that change the world.</description>"##,
    );
    assert!(opf.series.is_none());
    assert!(opf.publisher.is_none());
    assert!(opf.cover_path.is_none());
}

#[test]
fn picks_the_main_title() {
    let opf = parse(common::TWO_TITLES_OPF);
    let title = opf.title.as_ref().unwrap();
    assert_eq!(title.value, "The Tombs of Atuan");
    assert_slice(
        &opf,
        title,
        r##"<dc:title id="t2">The Tombs of Atuan</dc:title>"##,
    );
}

#[test]
fn reads_a_file_with_no_series_and_no_file_as() {
    let opf = parse(common::BARE_OPF);
    assert_eq!(opf.version, Version::Epub2);
    assert_eq!(opf.opf_prefix, None);
    let creator = &opf.creators[0];
    assert_eq!(creator.name, "Voltaire");
    assert_eq!(creator.sort(), None);
    assert!(creator.attributes.is_empty());
    assert!(opf.series.is_none());
    assert!(opf.publisher.is_none());
    assert!(opf.description.is_none());
    assert!(opf.word_count.is_none());
    assert!(opf.reading_ease.is_none());
    assert_eq!(opf.language.as_deref(), Some("fr"));
    assert_eq!(opf.insert_at, creator.range.end);
    assert_eq!(opf.indent, "\n    ");
}

#[test]
fn reads_the_word_count_and_reading_ease_from_standard_ebooks() {
    let opf = parse(common::STANDARD_EBOOKS_OPF);
    assert_eq!(opf.title.as_ref().unwrap().value, "Pride and Prejudice");
    assert_eq!(opf.creators[0].sort(), Some("Austen, Jane"));
    let word_count = opf.word_count.as_ref().unwrap();
    assert_eq!(word_count.value, "121970");
    assert_eq!(word_count.qname, "meta");
    assert_slice(
        &opf,
        word_count,
        r#"<meta property="schema:wordCount">121970</meta>"#,
    );
    let reading_ease = opf.reading_ease.as_ref().unwrap();
    assert_eq!(reading_ease.value, "60.95");
    assert_slice(
        &opf,
        reading_ease,
        r#"<meta property="schema:educationalLevel">60.95</meta>"#,
    );
    assert_eq!(
        Stats::from_opf(&opf),
        Stats {
            word_count: Some(121970),
            reading_ease: Some(60.95),
        }
    );
    assert!(opf.owned_ranges().contains(&word_count.range));
    assert!(opf.owned_ranges().contains(&reading_ease.range));
    assert_eq!(opf.indent, "\n\t\t");
}

#[test]
fn a_word_count_that_is_not_a_number_reads_as_none() {
    let text = common::STANDARD_EBOOKS_OPF.replace(">121970<", ">many<");
    let opf = parse(&text);
    assert_eq!(opf.word_count.as_ref().unwrap().value, "many");
    let stats = Stats::from_opf(&opf);
    assert!(stats.word_count.is_none());
    assert_eq!(stats.reading_ease, Some(60.95));
}

#[test]
fn lists_the_xhtml_spine_documents_without_the_nav() {
    assert_eq!(parse(common::EPUB2_OPF).spine, ["OEBPS/chapter1.xhtml"]);
    // The nav document is in the manifest and not in the spine here; a
    // spine that lists it still leaves it out.
    let with_nav = common::EPUB3_OPF.replace(
        r#"<itemref idref="ch1"/>"#,
        r#"<itemref idref="nav"/>
    <itemref idref="ch1"/>
    <itemref idref="cover"/>"#,
    );
    assert_eq!(parse(&with_nav).spine, ["OEBPS/chapter1.xhtml"]);
}

#[test]
fn owned_ranges_do_not_overlap_in_any_fixture() {
    for (name, text) in common::ALL_OPFS {
        let opf = parse(text);
        let ranges = opf.owned_ranges();
        for pair in ranges.windows(2) {
            assert!(
                pair[0].end <= pair[1].start,
                "{name}: {:?} overlaps {:?}",
                pair[0],
                pair[1]
            );
        }
        assert!(opf.title.is_some(), "{name}: no title");
    }
}

#[test]
fn accepts_a_doctype_in_the_container_and_the_opf() {
    // Some Pearson EPUBs put an XHTML DOCTYPE on their container.xml.
    let container = common::CONTAINER_XML.replacen(
        "?>\n",
        "?>\n<!DOCTYPE container PUBLIC \"-//W3C//DTD XHTML 1.1//EN\" \"http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd\">\n",
        1,
    );
    assert_eq!(opf::rootfile_path(&container).unwrap(), common::OPF_PATH);

    let with_doctype = common::EPUB2_OPF.replacen(
        "?>\n",
        "?>\n<!DOCTYPE package PUBLIC \"+//ISBN 0-9673008-1-9//DTD OEB 1.2 Package//EN\" \"http://openebook.org/dtds/oeb-1.2/oebpkg12.dtd\">\n",
        1,
    );
    let opf = parse(&with_doctype);
    assert_eq!(
        opf.title.as_ref().unwrap().value,
        "The Left Hand of Darkness"
    );
    assert_slice(
        &opf,
        opf.title.as_ref().unwrap(),
        "<dc:title>The Left Hand of Darkness</dc:title>",
    );
}

#[test]
fn joins_hrefs_to_the_opf_folder() {
    assert_eq!(
        join_path("OEBPS/content.opf", "images/cover.jpg"),
        "OEBPS/images/cover.jpg"
    );
    assert_eq!(join_path("content.opf", "cover.jpg"), "cover.jpg");
    assert_eq!(join_path("a/b/content.opf", "../cover.jpg"), "a/cover.jpg");
}

#[test]
fn undoes_percent_encoding_in_hrefs() {
    assert_eq!(
        join_path("OEBPS/content.opf", "Text/Other%2001.xhtml"),
        "OEBPS/Text/Other 01.xhtml"
    );
    assert_eq!(join_path("content.opf", "caf%C3%A9.xhtml"), "café.xhtml");
    assert_eq!(join_path("content.opf", "100%.xhtml"), "100%.xhtml");
    assert_eq!(join_path("content.opf", "a%2Fb.xhtml"), "a/b.xhtml");
}

#[test]
fn declares_an_attribute_prefix_no_element_declares() {
    let text = common::EPUB2_OPF
        .replace("opf:file-as", "ns0:file-as")
        .replace("opf:role", "ns1:role");
    let opf = parse(&text);
    assert_eq!(opf.opf_prefix.as_deref(), Some("opf"));
    assert!(
        opf.text.contains(
            r#"version="2.0" xmlns:ns0="http://www.idpf.org/2007/opf" xmlns:ns1="http://www.idpf.org/2007/opf">"#
        ),
        "{}",
        opf.text
    );
    let creator = &opf.creators[0];
    assert_eq!(creator.sort(), Some("Le Guin, Ursula K."));
    assert_eq!(creator.attributes[0].name, "ns0:file-as");
    assert_eq!(
        &opf.text[creator.range.clone()],
        r#"<dc:creator ns0:file-as="Le Guin, Ursula K." ns1:role="aut">Ursula K. Le Guin</dc:creator>"#
    );
    // The declared text parses again as it is.
    opf::parse(common::OPF_PATH, opf.text.clone()).unwrap();
}

#[test]
fn reports_an_xml_error_other_than_a_missing_prefix() {
    let text = common::EPUB2_OPF.replace("</dc:title>", "</dc:titel>");
    let err = opf::parse(common::OPF_PATH, text).unwrap_err();
    assert!(format!("{err:#}").starts_with("parse the OPF: "), "{err:#}");
}
