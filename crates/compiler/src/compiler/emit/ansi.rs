//! ANSI presentation for deterministic styled text.

use crate::core::Colors;

use super::sink::Style;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StyleChange {
    Set(Style),
    Reset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct StyleEvent {
    pub(super) offset: usize,
    pub(super) change: StyleChange,
}

pub(super) fn render(text: &str, styles: &[StyleEvent], colors: Colors) -> String {
    let mut output =
        String::with_capacity(text.len() + styles.len().saturating_mul(colors.reset.len()));
    let mut cursor = 0;
    for event in styles {
        output.push_str(
            text.get(cursor..event.offset)
                .expect("style offsets are emitted on UTF-8 boundaries in order"),
        );
        output.push_str(match event.change {
            StyleChange::Set(Style::Blue) => colors.blue,
            StyleChange::Set(Style::Green) => colors.green,
            StyleChange::Set(Style::Dim) => colors.dim,
            StyleChange::Reset => colors.reset,
        });
        cursor = event.offset;
    }
    output.push_str(
        text.get(cursor..)
            .expect("last style offset is a UTF-8 boundary"),
    );
    output
}
