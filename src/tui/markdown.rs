//! Markdown → styled ratatui lines for agent replies in the transcript.
//! Produces *logical* (unwrapped) lines; `ui::render_transcript` wraps them to
//! the terminal width with the same CJK-aware rules as plain text. Soft breaks
//! are kept as line breaks (chat replies use single newlines meaningfully), so
//! plain-text output renders exactly as before.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub(super) fn markdown_lines(text: &str) -> Vec<Line<'static>> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_TASKLISTS);
    let mut renderer = Renderer::default();
    for event in Parser::new_ext(text, opts) {
        renderer.on_event(event);
    }
    renderer.finish()
}

#[derive(Default)]
struct Renderer {
    lines: Vec<Line<'static>>,
    current: Vec<Span<'static>>,
    bold: u32,
    italic: u32,
    strike: u32,
    heading: bool,
    table_head: bool,
    code_block: bool,
    quote: u32,
    /// One entry per open list: `None` = bullet, `Some(n)` = next ordered index.
    lists: Vec<Option<u64>>,
    /// Open link: (url, span index where its text started).
    link: Option<(String, usize)>,
    /// Cell index within the table row being built.
    cell: usize,
}

impl Renderer {
    fn on_event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.on_start(tag),
            Event::End(tag) => self.on_end(tag),
            Event::Text(t) if self.code_block => {
                // Code block text arrives with embedded newlines; flushing even
                // empty parts preserves blank lines inside the block.
                for (i, part) in t.split('\n').enumerate() {
                    if i > 0 {
                        self.flush_line();
                    }
                    if !part.is_empty() {
                        let style = self.style();
                        self.current.push(Span::styled(part.to_string(), style));
                    }
                }
            }
            Event::Text(t) => {
                let style = self.style();
                self.current.push(Span::styled(t.into_string(), style));
            }
            Event::Code(t) => {
                let style = self.style().fg(Color::Yellow);
                self.current.push(Span::styled(t.into_string(), style));
            }
            Event::SoftBreak | Event::HardBreak => self.flush_if_content(),
            Event::Rule => {
                self.block_sep();
                self.current.push(Span::styled(
                    "─".repeat(24),
                    Style::new().fg(Color::DarkGray),
                ));
                self.flush_line();
            }
            Event::TaskListMarker(checked) => {
                let mark = if checked { "[x] " } else { "[ ] " };
                self.current
                    .push(Span::styled(mark, Style::new().fg(Color::DarkGray)));
            }
            // Raw HTML has no terminal rendering — show it verbatim rather
            // than lose content.
            Event::Html(t) | Event::InlineHtml(t) => {
                let style = self.style();
                self.current.push(Span::styled(t.into_string(), style));
            }
            _ => {}
        }
    }

    fn on_start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => self.block_sep(),
            Tag::Heading { .. } => {
                self.block_sep();
                self.heading = true;
            }
            Tag::BlockQuote(_) => {
                self.block_sep();
                self.quote += 1;
            }
            Tag::CodeBlock(_) => {
                self.block_sep();
                self.code_block = true;
            }
            Tag::List(start) => {
                if self.lists.is_empty() {
                    self.block_sep();
                }
                self.lists.push(start);
            }
            Tag::Item => {
                self.flush_if_content();
                let depth = self.lists.len().saturating_sub(1);
                let marker = match self.lists.last_mut() {
                    Some(Some(n)) => {
                        let m = format!("{n}. ");
                        *n += 1;
                        m
                    }
                    _ => "• ".to_string(),
                };
                self.current
                    .push(Span::raw(format!("{}{marker}", "  ".repeat(depth))));
            }
            Tag::Emphasis => self.italic += 1,
            Tag::Strong => self.bold += 1,
            Tag::Strikethrough => self.strike += 1,
            Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
                self.link = Some((dest_url.to_string(), self.current.len()));
            }
            Tag::Table(_) => self.block_sep(),
            Tag::TableHead => {
                self.table_head = true;
                self.cell = 0;
            }
            Tag::TableRow => self.cell = 0,
            Tag::TableCell => {
                if self.cell > 0 {
                    self.current
                        .push(Span::styled(" │ ", Style::new().fg(Color::DarkGray)));
                }
                self.cell += 1;
            }
            _ => {}
        }
    }

    fn on_end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::Item | TagEnd::TableRow => self.flush_if_content(),
            TagEnd::Heading(_) => {
                self.heading = false;
                self.flush_if_content();
            }
            TagEnd::BlockQuote(_) => {
                self.flush_if_content();
                self.quote = self.quote.saturating_sub(1);
            }
            TagEnd::CodeBlock => {
                self.flush_if_content();
                self.code_block = false;
            }
            TagEnd::List(_) => {
                self.lists.pop();
            }
            TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
            TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
            TagEnd::Strikethrough => self.strike = self.strike.saturating_sub(1),
            TagEnd::Link | TagEnd::Image => {
                if let Some((url, start)) = self.link.take() {
                    // Autolinks repeat the URL as their text — append the
                    // target only when it adds information.
                    let text: String = self.current[start..]
                        .iter()
                        .map(|s| s.content.as_ref())
                        .collect();
                    if !url.is_empty() && text != url {
                        self.current.push(Span::styled(
                            format!(" ({url})"),
                            Style::new().fg(Color::DarkGray),
                        ));
                    }
                }
            }
            TagEnd::TableHead => {
                self.table_head = false;
                self.flush_if_content();
            }
            _ => {}
        }
    }

    fn style(&self) -> Style {
        let mut s = Style::new();
        if self.code_block {
            s = s.fg(Color::Yellow);
        }
        if self.heading {
            s = s.fg(Color::Magenta).add_modifier(Modifier::BOLD);
        }
        if self.quote > 0 {
            s = s.fg(Color::DarkGray);
        }
        if self.link.is_some() {
            s = s.fg(Color::Blue).add_modifier(Modifier::UNDERLINED);
        }
        if self.bold > 0 || self.table_head {
            s = s.add_modifier(Modifier::BOLD);
        }
        if self.italic > 0 {
            s = s.add_modifier(Modifier::ITALIC);
        }
        if self.strike > 0 {
            s = s.add_modifier(Modifier::CROSSED_OUT);
        }
        s
    }

    /// Blank separator before a new block — only between blocks, never leading,
    /// and never while a line is being built (a list bullet awaiting its text).
    fn block_sep(&mut self) {
        if !self.current.is_empty() {
            return;
        }
        if self.lines.last().is_some_and(|l| !l.spans.is_empty()) {
            self.lines.push(Line::default());
        }
    }

    fn flush_if_content(&mut self) {
        if !self.current.is_empty() {
            self.flush_line();
        }
    }

    fn flush_line(&mut self) {
        let mut spans = std::mem::take(&mut self.current);
        if self.quote > 0 {
            spans.insert(0, Span::styled("▎ ", Style::new().fg(Color::DarkGray)));
        }
        self.lines.push(Line::from(spans));
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        self.flush_if_content();
        self.lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn plain_text_keeps_its_line_breaks() {
        let lines = markdown_lines("第一行\n第二行");
        assert_eq!(
            lines.iter().map(plain).collect::<Vec<_>>(),
            vec!["第一行", "第二行"]
        );
    }

    #[test]
    fn heading_is_bold_and_bullets_get_markers() {
        let lines = markdown_lines("# 标题\n\n- one\n- two\n\n1. first");
        let texts: Vec<String> = lines.iter().map(plain).collect();
        assert_eq!(texts[0], "标题");
        assert!(
            lines[0].spans[0]
                .style
                .add_modifier
                .contains(Modifier::BOLD),
            "heading rendered bold"
        );
        assert!(texts.contains(&"• one".to_string()), "{texts:?}");
        assert!(texts.contains(&"1. first".to_string()), "{texts:?}");
    }

    #[test]
    fn inline_styles_split_into_styled_spans() {
        let lines = markdown_lines("a **bold** and `code`");
        assert_eq!(lines.len(), 1);
        let spans = &lines[0].spans;
        let bold = spans.iter().find(|s| s.content == "bold").unwrap();
        assert!(bold.style.add_modifier.contains(Modifier::BOLD));
        let code = spans.iter().find(|s| s.content == "code").unwrap();
        assert_eq!(code.style.fg, Some(Color::Yellow));
    }

    #[test]
    fn code_block_preserves_lines_and_blank_lines() {
        let lines = markdown_lines("```\nlet a = 1;\n\nlet b = 2;\n```");
        let texts: Vec<String> = lines.iter().map(plain).collect();
        assert_eq!(texts, vec!["let a = 1;", "", "let b = 2;"]);
    }

    #[test]
    fn link_shows_target_unless_it_is_the_text() {
        let lines = markdown_lines("see [docs](https://example.com)");
        assert!(plain(&lines[0]).contains("docs (https://example.com)"));
        let lines = markdown_lines("see <https://example.com>");
        assert_eq!(plain(&lines[0]), "see https://example.com");
    }

    #[test]
    fn blockquote_lines_are_prefixed() {
        let lines = markdown_lines("> 引用内容");
        assert_eq!(plain(&lines[0]), "▎ 引用内容");
    }

    #[test]
    fn blocks_are_separated_by_one_blank_line() {
        let lines = markdown_lines("para one\n\npara two");
        let texts: Vec<String> = lines.iter().map(plain).collect();
        assert_eq!(texts, vec!["para one", "", "para two"]);
    }
}
