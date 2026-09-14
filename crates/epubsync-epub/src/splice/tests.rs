use super::splice;
use crate::fixtures as common;
use crate::metadata::{Author, Metadata, Series, Stats};
use crate::opf::{self, Opf, SeriesForm};

fn parse(text: &str) -> Opf {
    opf::parse(common::OPF_PATH, text.to_string()).unwrap()
}

fn record(opf: &Opf) -> Metadata {
    Metadata::from_opf(opf, |name| name.to_string())
}

/// The text outside the owned ranges, from the metadata element on. The
/// package start tag is left out because splice may add a namespace to it.
fn gaps(opf: &Opf) -> Vec<String> {
    let start = opf.text.find("<metadata").unwrap();
    let mut out = Vec::new();
    let mut pos = start;
    for range in opf.owned_ranges() {
        out.push(opf.text[pos..range.start].to_string());
        pos = range.end;
    }
    out.push(opf.text[pos..].to_string());
    out
}

/// A record of the same shape as the file's, with every value changed.
fn changed(opf: &Opf) -> Metadata {
    let mut m = record(opf);
    m.title = format!("{} (Revised)", m.title);
    for a in &mut m.authors {
        a.name = format!("{} II", a.name);
        a.sort = format!("{} II", a.sort);
    }
    if let Some(s) = &mut m.series {
        s.name = format!("{} & More", s.name);
        s.number = s.number.map(|n| n + 1.5);
    }
    m.publisher = m.publisher.map(|p| format!("{p} <Press>"));
    m.description = m.description.map(|d| format!("{d} Now with more."));
    m
}

#[test]
fn leaves_every_byte_outside_the_owned_ranges_unchanged() {
    for (name, text) in common::ALL_OPFS {
        let before = parse(text);
        let new = changed(&before);
        let spliced = splice(&before, &new, &Stats::from_opf(&before));
        let after = opf::parse(common::OPF_PATH, spliced.clone())
            .unwrap_or_else(|e| panic!("{name}: {e}\n{spliced}"));
        if before.owned_ranges().len() == after.owned_ranges().len() {
            assert_eq!(gaps(&before), gaps(&after), "{name}:\n{spliced}");
        } else {
            // An inserted element adds one gap of indentation. The bytes
            // outside the owned ranges are otherwise the same.
            let squash = |g: Vec<String>| g.concat().split_whitespace().collect::<String>();
            assert_eq!(
                squash(gaps(&before)),
                squash(gaps(&after)),
                "{name}:\n{spliced}"
            );
        }
        assert_eq!(record(&after), new, "{name}:\n{spliced}");
    }
}

#[test]
fn writes_the_calibre_series_form_in_place() {
    let before = parse(common::EPUB2_OPF);
    let mut new = record(&before);
    new.series = Some(Series {
        name: "Hainish \"Cycle\"".into(),
        number: Some(4.5),
    });
    let spliced = splice(&before, &new, &Stats::from_opf(&before));
    assert!(
        spliced.contains(r#"<meta name="calibre:series" content="Hainish &quot;Cycle&quot;"/>"#)
    );
    assert!(spliced.contains(r#"<meta name="calibre:series_index" content="4.5"/>"#));
    assert!(!spliced.contains("belongs-to-collection"));
    let after = parse(&spliced);
    assert_eq!(after.series.as_ref().unwrap().number(), Some(4.5));
}

#[test]
fn writes_the_collection_series_form_in_place() {
    let before = parse(common::EPUB3_OPF);
    let mut new = record(&before);
    new.series = Some(Series {
        name: "Earthsea".into(),
        number: Some(2.0),
    });
    let spliced = splice(&before, &new, &Stats::from_opf(&before));
    assert!(
        spliced.contains(r##"<meta property="belongs-to-collection" id="c01">Earthsea</meta>"##)
    );
    assert!(spliced.contains(r##"<meta refines="#c01" property="collection-type">series</meta>"##));
    assert!(spliced.contains(r##"<meta refines="#c01" property="group-position">2</meta>"##));
    assert!(!spliced.contains("calibre:series"));
    assert_eq!(record(&parse(&spliced)), new);
}

#[test]
fn gives_a_collection_without_an_id_one_before_refining_it() {
    let text = common::EPUB3_OPF
        .replace(
            r##"<meta property="belongs-to-collection" id="c01">"##,
            r##"<meta property="belongs-to-collection">"##,
        )
        .lines()
        .filter(|line| !line.contains(r##"refines="#c01""##))
        .collect::<Vec<_>>()
        .join("\n");
    let before = parse(&text);
    assert!(matches!(
        before.series.as_ref().unwrap().form,
        SeriesForm::Collection { id: None, .. }
    ));
    let mut new = record(&before);
    new.series = Some(Series {
        name: "Earthsea".into(),
        number: Some(2.0),
    });
    let spliced = splice(&before, &new, &Stats::from_opf(&before));
    assert!(!spliced.contains(r##"refines="#""##), "{spliced}");
    assert!(
        spliced.contains(
            r##"<meta property="belongs-to-collection" id="collection1">Earthsea</meta>"##
        ),
        "{spliced}"
    );
    assert!(
        spliced.contains(r##"<meta refines="#collection1" property="group-position">2</meta>"##),
        "{spliced}"
    );
    assert_eq!(record(&parse(&spliced)), new);
}

#[test]
fn removes_the_series_when_the_record_has_none() {
    for text in [common::EPUB2_OPF, common::EPUB3_OPF] {
        let before = parse(text);
        let mut new = record(&before);
        new.series = None;
        let spliced = splice(&before, &new, &Stats::from_opf(&before));
        assert!(!spliced.contains("calibre:series"), "{spliced}");
        assert!(!spliced.contains("belongs-to-collection"), "{spliced}");
        assert!(!spliced.contains("group-position"), "{spliced}");
        assert!(
            !spliced.contains("\n\n"),
            "blank line left behind:\n{spliced}"
        );
        assert_eq!(record(&parse(&spliced)), new);
    }
}

#[test]
fn writes_file_as_in_the_form_the_file_has() {
    // EPUB 2: the attribute.
    let before = parse(common::EPUB2_OPF);
    let mut new = record(&before);
    new.authors[0].sort = "Guin, Ursula".into();
    let spliced = splice(&before, &new, &Stats::from_opf(&before));
    assert!(spliced.contains(
        r#"<dc:creator opf:file-as="Guin, Ursula" opf:role="aut">Ursula K. Le Guin</dc:creator>"#
    ));
    assert!(!spliced.contains(r#"property="file-as""#));

    // EPUB 3: the refinement.
    let before = parse(common::EPUB3_OPF);
    let mut new = record(&before);
    new.authors[0].sort = "Guin, Ursula".into();
    let spliced = splice(&before, &new, &Stats::from_opf(&before));
    assert!(spliced.contains(r#"<dc:creator id="creator01">Ursula K. Le Guin</dc:creator>"#));
    assert!(
        spliced.contains(r##"<meta refines="#creator01" property="file-as">Guin, Ursula</meta>"##)
    );
    assert!(!spliced.contains("opf:file-as"));

    // Calibre EPUB 3: both, with the same value.
    let before = parse(common::EPUB3_CALIBRE_OPF);
    let mut new = record(&before);
    new.authors[0].sort = "Guin, Ursula".into();
    let spliced = splice(&before, &new, &Stats::from_opf(&before));
    assert!(spliced.contains(r#"<dc:creator id="id" opf:file-as="Guin, Ursula" opf:role="aut">Ursula K. Le Guin</dc:creator>"#));
    assert!(spliced.contains(r##"<meta refines="#id" property="file-as">Guin, Ursula</meta>"##));
    assert_eq!(parse(&spliced).creators[0].sort(), Some("Guin, Ursula"));
}

#[test]
fn inserts_missing_fields_into_a_bare_epub2_file() {
    let before = parse(common::BARE_OPF);
    let new = Metadata {
        title: "Candide".into(),
        authors: vec![Author {
            name: "Voltaire".into(),
            sort: "Voltaire".into(),
        }],
        series: Some(Series {
            name: "Contes".into(),
            number: Some(1.0),
        }),
        publisher: Some("Cramer".into()),
        description: Some("Tout est pour le mieux & co.".into()),
    };
    let spliced = splice(&before, &new, &Stats::from_opf(&before));
    assert!(spliced.contains(r#"<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="bookid" version="2.0" xmlns:opf="http://www.idpf.org/2007/opf">"#), "{spliced}");
    assert!(
        spliced.contains(r#"<dc:creator opf:file-as="Voltaire">Voltaire</dc:creator>"#),
        "{spliced}"
    );
    let expected_inserts = concat!(
        "<dc:creator opf:file-as=\"Voltaire\">Voltaire</dc:creator>\n",
        "    <dc:publisher>Cramer</dc:publisher>\n",
        "    <dc:description>Tout est pour le mieux &amp; co.</dc:description>\n",
        "    <meta name=\"calibre:series\" content=\"Contes\"/>\n",
        "    <meta name=\"calibre:series_index\" content=\"1\"/>\n",
        "    <dc:language>fr</dc:language>\n",
    );
    assert!(spliced.contains(expected_inserts), "{spliced}");
    assert_eq!(record(&parse(&spliced)), new);
}

#[test]
fn inserts_a_creator_id_when_epub3_needs_one() {
    let text = common::TWO_TITLES_OPF.replace(r#"<dc:creator id="a1">"#, "<dc:creator>");
    let before = parse(&text);
    assert!(before.creators[0].id.is_none());
    let mut new = record(&before);
    new.authors[0].sort = "Le Guin, Ursula K.".into();
    let spliced = splice(&before, &new, &Stats::from_opf(&before));
    assert!(
        spliced.contains(r#"<dc:creator id="creator1">Ursula K. Le Guin</dc:creator>"#),
        "{spliced}"
    );
    assert!(
        spliced.contains(
            r##"<meta refines="#creator1" property="file-as">Le Guin, Ursula K.</meta>"##
        ),
        "{spliced}"
    );
    assert!(!spliced.contains("xmlns:opf"), "{spliced}");
    assert_eq!(record(&parse(&spliced)), new);
}

#[test]
fn adds_and_removes_creators() {
    let before = parse(common::EPUB3_OPF);
    let mut new = record(&before);
    new.authors.push(Author {
        name: "Charles Vess".into(),
        sort: "Vess, Charles".into(),
    });
    let spliced = splice(&before, &new, &Stats::from_opf(&before));
    assert!(
        spliced.contains(r#"<dc:creator id="creator1">Charles Vess</dc:creator>"#),
        "{spliced}"
    );
    assert!(
        spliced.contains(r##"<meta refines="#creator1" property="file-as">Vess, Charles</meta>"##),
        "{spliced}"
    );
    let two = parse(&spliced);
    assert_eq!(record(&two), new);

    let mut one = record(&two);
    one.authors.truncate(1);
    let spliced = splice(&two, &one, &Stats::from_opf(&two));
    assert!(!spliced.contains("Vess"), "{spliced}");
    assert!(!spliced.contains("\n\n"), "{spliced}");
    assert_eq!(record(&parse(&spliced)), one);
}

#[test]
fn escapes_the_description_as_text() {
    let before = parse(common::EPUB2_OPF);
    let mut new = record(&before);
    new.description = Some("<p>Tom &amp; Jerry</p>".into());
    let spliced = splice(&before, &new, &Stats::from_opf(&before));
    assert!(
        spliced.contains("<dc:description>&lt;p&gt;Tom &amp;amp; Jerry&lt;/p&gt;</dc:description>")
    );
    assert_eq!(
        parse(&spliced).description.unwrap().value,
        "<p>Tom &amp; Jerry</p>"
    );
}

#[test]
fn keeps_the_main_title_and_the_other_title() {
    let before = parse(common::TWO_TITLES_OPF);
    let mut new = record(&before);
    new.title = "The Tombs".into();
    let spliced = splice(&before, &new, &Stats::from_opf(&before));
    assert!(spliced.contains(r#"<dc:title id="t1">Earthsea</dc:title>"#));
    assert!(spliced.contains(r#"<dc:title id="t2">The Tombs</dc:title>"#));
}

#[test]
fn replaces_the_word_count_and_reading_ease_in_place() {
    let before = parse(common::STANDARD_EBOOKS_OPF);
    let new = record(&before);
    let stats = Stats {
        word_count: Some(5),
        reading_ease: Some(70.0),
    };
    let spliced = splice(&before, &new, &stats);
    assert!(
        spliced.contains("\t\t<meta property=\"schema:wordCount\">5</meta>\n"),
        "{spliced}"
    );
    assert!(
        spliced.contains("\t\t<meta property=\"schema:educationalLevel\">70.00</meta>\n"),
        "{spliced}"
    );
    assert!(!spliced.contains("121970"), "{spliced}");
    assert!(!spliced.contains("60.95"), "{spliced}");
    let after = parse(&spliced);
    assert_eq!(Stats::from_opf(&after), stats);
    assert_eq!(record(&after), new);
}

#[test]
fn inserts_the_word_count_and_reading_ease_into_a_file_that_has_none() {
    for (text, expected) in [
        (
            common::BARE_OPF,
            concat!(
                "<dc:creator opf:file-as=\"Voltaire\">Voltaire</dc:creator>\n",
                "    <meta property=\"schema:wordCount\">13</meta>\n",
                "    <meta property=\"schema:educationalLevel\">83.50</meta>\n",
                "    <dc:language>fr</dc:language>\n",
            ),
        ),
        (
            common::EPUB2_OPF,
            concat!(
                "<meta name=\"calibre:series_index\" content=\"4\"/>\n",
                "    <meta property=\"schema:wordCount\">13</meta>\n",
                "    <meta property=\"schema:educationalLevel\">83.50</meta>\n",
                "    <meta name=\"cover\" content=\"cover\"/>\n",
            ),
        ),
    ] {
        let before = parse(text);
        let new = record(&before);
        let stats = Stats {
            word_count: Some(13),
            reading_ease: Some(83.5),
        };
        let spliced = splice(&before, &new, &stats);
        assert!(spliced.contains(expected), "{spliced}");
        let after = parse(&spliced);
        assert_eq!(Stats::from_opf(&after), stats);
        assert_eq!(record(&after), new);
    }

    // A book with no reading ease gets no educationalLevel element.
    let before = parse(common::BARE_OPF);
    let stats = Stats {
        word_count: Some(13),
        reading_ease: None,
    };
    let spliced = splice(&before, &record(&before), &stats);
    assert!(spliced.contains(r#"<meta property="schema:wordCount">13</meta>"#));
    assert!(!spliced.contains("educationalLevel"), "{spliced}");
    assert_eq!(Stats::from_opf(&parse(&spliced)), stats);
}
