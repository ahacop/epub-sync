//! Converts a book's description to the items the markdown widget draws.
//! Many EPUBs hold HTML in `dc:description`: Calibre and Standard Ebooks
//! both write `<p>` tags there.

use iced::widget::markdown;

/// Converts the HTML to Markdown and parses it. Text with no tags comes
/// back as one paragraph, with Markdown characters shown as written.
pub fn parse(html: &str) -> Vec<markdown::Item> {
    // htmd fails only when its writer fails, which a String does not.
    let text = htmd::convert(html).unwrap_or_else(|_| html.to_string());
    markdown::parse(&text).collect()
}

#[cfg(test)]
mod tests {
    use iced::Theme;
    use iced::font::{Style, Weight};
    use iced::widget::markdown::{self, Item};

    use super::parse;

    /// The spans of a paragraph item as (text, bold, italic).
    fn spans(item: &Item) -> Vec<(String, bool, bool)> {
        let Item::Paragraph(text) = item else {
            panic!("not a paragraph: {item:?}");
        };
        text.spans(markdown::Style::from(&Theme::Light))
            .iter()
            .map(|s| {
                let font = s.font.unwrap_or_default();
                (
                    s.text.to_string(),
                    font.weight == Weight::Bold,
                    font.style == Style::Italic,
                )
            })
            .collect()
    }

    fn plain(item: &Item) -> String {
        spans(item).into_iter().map(|(t, _, _)| t).collect()
    }

    #[test]
    fn two_p_elements_become_two_paragraphs() {
        let items = parse("<p>One.</p><p>Two.</p>");
        assert_eq!(items.len(), 2);
        assert_eq!(plain(&items[0]), "One.");
        assert_eq!(plain(&items[1]), "Two.");
    }

    #[test]
    fn b_and_i_become_bold_and_italic() {
        let items = parse("<p>a <b>bold</b> and <i>italic</i> word</p>");
        assert_eq!(items.len(), 1);
        let spans = spans(&items[0]);
        assert!(
            spans.contains(&("bold".to_string(), true, false)),
            "{spans:?}"
        );
        assert!(
            spans.contains(&("italic".to_string(), false, true)),
            "{spans:?}"
        );
    }

    #[test]
    fn br_becomes_a_line_break() {
        let items = parse("<p>line one<br>line two</p>");
        assert_eq!(items.len(), 1);
        assert_eq!(plain(&items[0]), "line one\nline two");
    }

    #[test]
    fn plain_text_keeps_its_star() {
        let items = parse("A note on * stars");
        assert_eq!(items.len(), 1);
        assert_eq!(plain(&items[0]), "A note on * stars");
    }
}
