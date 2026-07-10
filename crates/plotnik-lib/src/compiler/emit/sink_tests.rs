use crate::compiler::emit::sink::{Sink, Style, indentation};
use crate::core::Colors;

#[test]
fn one_emission_renders_plain_ansi_and_tags() {
    let mut sink = Sink::new();
    sink.styled(Style::Dim, "type");
    sink.push(" ");
    sink.set_style(Style::Blue);
    sink.tagged("name", |sink| sink.push("Q"));
    sink.reset_style();

    assert_eq!(sink.plain(), "type Q");
    assert_eq!(sink.tags()[0].start, 5);
    assert_eq!(sink.tags()[0].end, 6);
    assert_eq!(sink.tags()[0].tag, "name");
    assert_eq!(sink.render(Colors::OFF), "type Q");
    assert_eq!(
        sink.render(Colors::ON),
        "\x1b[2mtype\x1b[0m \x1b[34mQ\x1b[0m"
    );
}

#[test]
fn append_offsets_tags_and_styles() {
    let mut child = Sink::new();
    child.set_style(Style::Green);
    child.tagged(7, |sink| sink.push("value"));
    child.reset_style();

    let mut parent = Sink::new();
    parent.push("key: ");
    parent.append(child);

    assert_eq!(parent.tags()[0].start, 5);
    assert_eq!(parent.tags()[0].end, 10);
    assert_eq!(parent.render(Colors::ON), "key: \x1b[32mvalue\x1b[0m");
}

#[test]
fn line_indentation_leaves_blank_lines_empty() {
    let mut sink = Sink::<()>::new();
    sink.line("outer");
    sink.indented(|sink| sink.lines("first\n\nsecond\n"));

    assert_eq!(sink.plain(), "outer\n    first\n\n    second\n");
    assert_eq!(indentation(2), "        ");
}
