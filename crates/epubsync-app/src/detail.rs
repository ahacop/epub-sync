//! The sidebar: the selected book's details, its description, and its
//! progress on each device.

use std::collections::BTreeMap;

use epubsync_core::library::{Book, ProgressRow, book_file_name};
use epubsync_core::metadata::format_series_number;
use iced::widget::{
    button, column, container, markdown, progress_bar, row, scrollable, space, text,
};
use iced::{Center, Color, Element, Fill, padding};

use crate::theme::{self, MONO, SANS_MEDIUM, SERIF, SERIF_MEDIUM};
use crate::{Message, Open, Selected, format, table};

const WIDTH: f32 = 360.0;
/// The side padding of the header, the body, and the footer.
const INSET: f32 = 20.0;

/// The link color in the description. The markdown widget takes its
/// colors before it is drawn, when the mode is not known, so this one
/// color reads on both grounds.
const LINK: Color = Color::from_rgb8(0x4A, 0x8F, 0xA0);

/// The sidebar: a header, a scrollable body, and a footer, with a 1 px
/// line on its left.
pub fn view<'a>(open: &'a Open, book: &'a Book, selected: &'a Selected) -> Element<'a, Message> {
    let pane = column![
        header(book.id),
        theme::hline(),
        scrollable(body(book, selected, &open.progress)).height(Fill),
        theme::hline(),
        footer(open, book),
    ];
    row![
        theme::vline(),
        container(pane)
            .width(WIDTH)
            .height(Fill)
            .style(theme::ground(|c| c.window)),
    ]
    .into()
}

/// "Book 13" and the close button. The label starts at the body's left
/// inset, and the close button's glyph ends at the body's right inset.
fn header<'a>(id: i64) -> Element<'a, Message> {
    let close = button(container(text("×").size(15)).center(22))
        .on_press(Message::Close)
        .padding(0)
        .style(theme::close);
    row![
        theme::label(format!("Book {id}")).style(theme::text_color(|c| c.muted)),
        space().width(Fill),
        close,
    ]
    .align_y(Center)
    .height(theme::HEADER)
    .padding(padding::left(INSET).right(INSET - 6.0))
    .into()
}

fn body<'a>(
    book: &'a Book,
    selected: &'a Selected,
    progress: &'a BTreeMap<i64, Vec<ProgressRow>>,
) -> Element<'a, Message> {
    let m = &book.metadata;

    let title = text(&m.title).size(26).font(SERIF_MEDIUM).line_height(1.15);
    let mut byline = row![
        text(format::authors(&m.authors))
            .size(14)
            .style(theme::text_color(|c| c.ink_2))
    ]
    .spacing(4);
    if let Some(series) = &m.series {
        byline = byline.push(
            text(format!("· {}", format::series_tag(series)))
                .size(14)
                .style(theme::text_color(|c| c.muted)),
        );
    }

    let mut meta = column![].spacing(6);
    if let Some(publisher) = &m.publisher {
        meta = meta.push(field("Publisher", text(publisher).size(12.5)));
    }
    if let Some(series) = &m.series {
        let value = match series.number {
            Some(n) => format!("{}, book {}", series.name, format_series_number(n)),
            None => series.name.clone(),
        };
        meta = meta.push(field("Series", text(value).size(12.5)));
    }
    if let Some(words) = book.stats.word_count {
        let value = format!("{} words", format::thousands(words));
        meta = meta.push(field("Length", text(value).size(12.5)));
    }
    if let Some(score) = book.stats.reading_ease {
        meta = meta.push(field("Ease", text(format::reading_ease(score)).size(12.5)));
    }
    meta = meta.push(field(
        "File",
        text(book_file_name(book.id)).font(MONO).size(11.5),
    ));

    let description: Element<'a, Message> = if selected.description.is_empty() {
        text("No description in the file.")
            .size(13)
            .style(theme::text_color(|c| c.faint))
            .into()
    } else {
        markdown::view(&selected.description, description_settings())
            .map(|_uri| Message::LinkClicked)
    };

    let mut devices = column![
        text("ON DEVICE")
            .size(11.5)
            .font(SANS_MEDIUM)
            .style(theme::text_color(|c| c.muted))
    ]
    .spacing(8);
    let rows = progress.get(&book.id).map(Vec::as_slice).unwrap_or(&[]);
    if rows.is_empty() {
        devices = devices.push(
            text("Not yet sent to a device.")
                .size(12.5)
                .style(theme::text_color(|c| c.faint)),
        );
    }
    for (i, p) in rows.iter().enumerate() {
        if i > 0 {
            devices = devices.push(theme::hline());
        }
        devices = devices.push(device(p));
    }

    column![
        column![title, byline.wrap()].spacing(6),
        theme::hline(),
        meta,
        theme::hline(),
        description,
        theme::hline(),
        devices,
    ]
    .spacing(14)
    .padding(padding::top(18).bottom(24).left(INSET).right(INSET))
    .into()
}

/// A label and its value on one line.
fn field<'a>(label: &'a str, value: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    row![
        text(label)
            .size(12.5)
            .width(72)
            .style(theme::text_color(|c| c.muted)),
        value.into(),
    ]
    .spacing(14)
    .into()
}

/// One device's progress: the serial and the day, a full-width bar, then
/// the percent and the status chip.
fn device(p: &ProgressRow) -> Element<'_, Message> {
    let when = match table::day_of(p) {
        Some(day) => format!("read {}", format::day(day)),
        None => "not opened".to_string(),
    };
    column![
        row![
            text(&p.device_serial)
                .font(MONO)
                .size(11.5)
                .style(theme::text_color(|c| c.ink_2)),
            space().width(Fill),
            text(when).size(12.5).style(theme::text_color(|c| c.muted)),
        ]
        .align_y(Center),
        progress_bar(0.0..=100.0, p.percent as f32)
            .length(Fill)
            .girth(5)
            .style(theme::bar(p.status)),
        row![
            text(format!("{}%", p.percent))
                .size(12.5)
                .style(theme::text_color(|c| c.ink_2)),
            space().width(Fill),
            table::chip(p.status),
        ]
        .align_y(Center),
    ]
    .spacing(6)
    .into()
}

/// The description in the serif face at 15.5 px.
fn description_settings() -> markdown::Settings {
    let style = markdown::Style {
        font: SERIF,
        link_color: LINK,
        ..markdown::Style::from_palette(iced::theme::Palette::LIGHT)
    };
    markdown::Settings::with_text_size(15.5, style)
}

/// The full file path on one line, clipped.
fn footer<'a>(open: &'a Open, book: &'a Book) -> Element<'a, Message> {
    let path = open.folder.join(book_file_name(book.id));
    container(
        text(path.display().to_string())
            .font(MONO)
            .size(11)
            .wrapping(text::Wrapping::None)
            .style(theme::text_color(|c| c.muted)),
    )
    .width(Fill)
    .padding([8.0, INSET])
    .clip(true)
    .into()
}
