use jones_syntax::Highlighter;
use jones_theme as theme;
use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};

pub fn render_markdown(input: &str) -> Text<'static> {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_HEADING_ATTRIBUTES
        | Options::ENABLE_FOOTNOTES;

    let parser = Parser::new_ext(input, options);

    let mut renderer = MarkdownRenderer::new();
    renderer.process(parser);
    renderer.finish()
}

// ── Bullet styles per nesting depth ──────────────────────────────────

const BULLET_CHARS: &[char] = &['\u{2022}', '\u{25E6}', '\u{25AA}']; // •, ◦, ▪

fn bullet_for_depth(depth: usize) -> char {
    BULLET_CHARS[depth % BULLET_CHARS.len()]
}

// ── Renderer state machine ────────────────────────────────────────────

struct MarkdownRenderer {
    /// Completed lines ready for output.
    lines: Vec<Line<'static>>,
    /// Spans being accumulated for the current line.
    current_spans: Vec<Span<'static>>,
    /// Stack of styles for nesting (bold inside italic, etc.).
    style_stack: Vec<Style>,
    /// List nesting: each entry is `Some(counter)` for ordered lists or
    /// `None` for unordered lists.
    list_stack: Vec<Option<u64>>,
    /// Whether we are inside a code block.
    in_code_block: bool,
    /// Language label for the current code block (empty if none).
    code_block_lang: String,
    /// Blockquote nesting depth.
    blockquote_depth: usize,

    // ── Table state ──────────────────────────────────────────────────
    /// Column alignments for the current table.
    table_alignments: Vec<Alignment>,
    /// Whether we are inside a table header row.
    in_table_head: bool,
    /// Whether we are inside a table (head or body).
    in_table: bool,
    /// Cells collected for the current row (each cell is a list of spans).
    table_row_cells: Vec<Vec<Span<'static>>>,
    /// All completed rows (header + body).  Each row is a Vec of cells,
    /// each cell a Vec<Span>.
    table_rows: Vec<Vec<Vec<Span<'static>>>>,
    /// Index of the header row inside `table_rows` (always 0 when present).
    table_header_row_count: usize,

    // ── Image state ──────────────────────────────────────────────────
    /// When we are inside an Image tag, collect text spans for the alt text.
    in_image: bool,
    image_alt_buf: String,

    // ── Footnote state ───────────────────────────────────────────────
    in_footnote_def: bool,
    footnote_label: String,
}

impl MarkdownRenderer {
    fn new() -> Self {
        Self {
            lines: Vec::new(),
            current_spans: Vec::new(),
            style_stack: vec![Style::default().fg(theme::text_primary())],
            list_stack: Vec::new(),
            in_code_block: false,
            code_block_lang: String::new(),
            blockquote_depth: 0,

            table_alignments: Vec::new(),
            in_table_head: false,
            in_table: false,
            table_row_cells: Vec::new(),
            table_rows: Vec::new(),
            table_header_row_count: 0,

            in_image: false,
            image_alt_buf: String::new(),

            in_footnote_def: false,
            footnote_label: String::new(),
        }
    }

    /// Current effective style (top of stack).
    fn current_style(&self) -> Style {
        self.style_stack.last().copied().unwrap_or_default()
    }

    /// Push a style modifier on top of the current style.
    fn push_style(&mut self, modifier: Style) {
        let base = self.current_style();
        let merged = base.patch(modifier);
        self.style_stack.push(merged);
    }

    fn pop_style(&mut self) {
        if self.style_stack.len() > 1 {
            self.style_stack.pop();
        }
    }

    /// Flush current_spans into a completed Line, prepending blockquote
    /// indicators if nested inside blockquotes.
    fn finish_line(&mut self) {
        let spans = std::mem::take(&mut self.current_spans);
        let mut line = Line::from(spans);
        if self.blockquote_depth > 0 {
            let prefix: String = "\u{2502} ".repeat(self.blockquote_depth);
            let mut new_spans = vec![Span::styled(
                prefix,
                Style::default().fg(theme::blockquote()),
            )];
            // Apply subtle background tint to blockquote content spans
            for span in &mut line.spans {
                span.style = span.style.bg(theme::bg_surface());
            }
            new_spans.extend(line.spans);
            line = Line::from(new_spans);
        }
        self.lines.push(line);
    }

    /// Process all parser events.
    fn process<'a>(&mut self, parser: Parser<'a>) {
        for event in parser {
            self.handle_event(event);
        }
        if !self.current_spans.is_empty() {
            self.finish_line();
        }
    }

    fn handle_event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.handle_start(tag),
            Event::End(tag_end) => self.handle_end(tag_end),

            Event::Text(text) => {
                // Image alt-text collection
                if self.in_image {
                    self.image_alt_buf.push_str(&text);
                    return;
                }

                // Table cell text collection
                if self.in_table {
                    let style = self.current_style();
                    self.current_spans
                        .push(Span::styled(text.to_string(), style));
                    return;
                }

                if self.in_code_block {
                    // Render each line of code block text with indentation
                    for (i, code_line) in text.lines().enumerate() {
                        if i > 0 {
                            self.finish_line();
                        }
                        self.push_code_line(code_line);
                    }
                } else {
                    let style = self.current_style();
                    self.current_spans
                        .push(Span::styled(text.to_string(), style));
                }
            }

            Event::Code(code) => {
                if self.in_image {
                    self.image_alt_buf.push_str(&code);
                    return;
                }
                let style = Style::default().fg(theme::code_fg()).bg(theme::code_bg());
                self.current_spans
                    .push(Span::styled(format!("`{code}`"), style));
            }

            Event::SoftBreak => {
                self.current_spans
                    .push(Span::styled(" ", self.current_style()));
            }

            Event::HardBreak => {
                self.finish_line();
            }

            Event::Rule => {
                if !self.current_spans.is_empty() {
                    self.finish_line();
                }
                let rule_str = "\u{2500}".repeat(80);
                self.lines.push(Line::from(Span::styled(
                    rule_str,
                    Style::default().fg(theme::rule()),
                )));
                self.lines.push(Line::default());
            }

            Event::TaskListMarker(checked) => {
                let marker = if checked { "[\u{2713}] " } else { "[ ] " };
                self.current_spans.push(Span::styled(
                    marker.to_string(),
                    Style::default().fg(theme::task_marker()),
                ));
            }

            Event::FootnoteReference(label) => {
                let display = format!("[^{label}]");
                self.current_spans.push(Span::styled(
                    display,
                    Style::default()
                        .fg(theme::footnote_ref())
                        .add_modifier(Modifier::BOLD),
                ));
            }

            _ => {}
        }
    }

    fn handle_start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Heading { level, .. } => {
                let color = heading_color(level);
                self.push_style(Style::default().fg(color).add_modifier(Modifier::BOLD));
            }
            Tag::Emphasis => {
                self.push_style(Style::default().add_modifier(Modifier::ITALIC));
            }
            Tag::Strong => {
                self.push_style(Style::default().add_modifier(Modifier::BOLD));
            }
            Tag::Strikethrough => {
                self.push_style(Style::default().add_modifier(Modifier::CROSSED_OUT));
            }
            Tag::Link { .. } => {
                self.push_style(
                    Style::default()
                        .fg(theme::link())
                        .add_modifier(Modifier::UNDERLINED),
                );
            }
            Tag::Image { .. } => {
                self.in_image = true;
                self.image_alt_buf.clear();
            }
            Tag::CodeBlock(kind) => {
                self.in_code_block = true;
                if !self.current_spans.is_empty() {
                    self.finish_line();
                }
                // Track language and render top border with optional label
                let lang_str = if let CodeBlockKind::Fenced(ref lang) = kind {
                    lang.trim().to_string()
                } else {
                    String::new()
                };
                self.code_block_lang = lang_str.clone();
                let border_style = Style::default().fg(theme::code_lang_label());
                let top_border = if lang_str.is_empty() {
                    format!("\u{250C}{}", "\u{2500}".repeat(40))
                } else {
                    format!(
                        "\u{250C}\u{2500}\u{2500}\u{2500} [{lang_str}] \u{2500}\u{2500}\u{2500}"
                    )
                };
                self.lines
                    .push(Line::from(Span::styled(top_border, border_style)));
                self.push_style(
                    Style::default()
                        .fg(theme::code_block_fg())
                        .bg(theme::code_block_bg()),
                );
            }
            Tag::BlockQuote(_) => {
                self.blockquote_depth += 1;
                self.push_style(Style::default().fg(theme::text_secondary()));
            }
            Tag::List(start_number) => {
                self.list_stack.push(start_number);
            }
            Tag::Item => {
                let depth = self.list_stack.len().saturating_sub(1);
                let indent = "  ".repeat(depth);

                let marker = if let Some(ordered) = self.list_stack.last().copied().flatten() {
                    let m = format!("{indent}{ordered}. ");
                    if let Some(Some(n)) = self.list_stack.last_mut() {
                        *n += 1;
                    }
                    m
                } else {
                    let bullet = bullet_for_depth(depth);
                    format!("{indent}{bullet} ")
                };

                self.current_spans.push(Span::styled(
                    marker,
                    Style::default().fg(theme::list_bullet()),
                ));
            }
            Tag::FootnoteDefinition(label) => {
                self.in_footnote_def = true;
                self.footnote_label = label.to_string();
                if !self.current_spans.is_empty() {
                    self.finish_line();
                }
                let prefix = format!("[^{}]: ", self.footnote_label);
                self.current_spans.push(Span::styled(
                    prefix,
                    Style::default()
                        .fg(theme::footnote_def())
                        .add_modifier(Modifier::BOLD),
                ));
            }
            Tag::Table(alignments) => {
                self.in_table = true;
                self.table_alignments = alignments;
                self.table_rows.clear();
                self.table_header_row_count = 0;
                if !self.current_spans.is_empty() {
                    self.finish_line();
                }
            }
            Tag::TableHead => {
                self.in_table_head = true;
            }
            Tag::TableRow => {
                self.table_row_cells.clear();
            }
            Tag::TableCell => {
                // Start collecting spans for this cell
                self.current_spans.clear();
            }
            Tag::Paragraph => {}
            _ => {}
        }
    }

    fn push_code_line(&mut self, code_line: &str) {
        let base_style = self.current_style();
        self.current_spans.push(Span::styled("  ", base_style));

        for (style, content) in highlight_code_line(&self.code_block_lang, code_line) {
            self.current_spans
                .push(Span::styled(content, base_style.patch(style)));
        }
    }

    fn handle_end(&mut self, tag_end: TagEnd) {
        match tag_end {
            TagEnd::Heading(level) => {
                self.pop_style();
                self.finish_line();
                // H1/H2 get decorative underlines
                if level <= HeadingLevel::H2 {
                    let underline_char = if level == HeadingLevel::H1 {
                        '\u{2501}'
                    } else {
                        '\u{2500}'
                    };
                    let underline = underline_char.to_string().repeat(40);
                    let color = heading_color(level);
                    let dim_style = Style::default().fg(color).add_modifier(Modifier::DIM);
                    self.lines
                        .push(Line::from(Span::styled(underline, dim_style)));
                }
                self.lines.push(Line::default());
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                self.pop_style();
            }
            TagEnd::Image => {
                self.in_image = false;
                let alt = std::mem::take(&mut self.image_alt_buf);
                let display = if alt.is_empty() {
                    "[img]".to_string()
                } else {
                    format!("[img: {alt}]")
                };
                self.current_spans.push(Span::styled(
                    display,
                    Style::default()
                        .fg(theme::image_tag())
                        .add_modifier(Modifier::ITALIC),
                ));
            }
            TagEnd::CodeBlock => {
                self.in_code_block = false;
                self.pop_style();
                if !self.current_spans.is_empty() {
                    self.finish_line();
                }
                // Bottom border for code block
                let border_style = Style::default().fg(theme::code_lang_label());
                let bottom_border = format!("\u{2514}{}", "\u{2500}".repeat(40));
                self.lines
                    .push(Line::from(Span::styled(bottom_border, border_style)));
                self.code_block_lang.clear();
                self.lines.push(Line::default());
            }
            TagEnd::BlockQuote(_) => {
                self.blockquote_depth = self.blockquote_depth.saturating_sub(1);
                self.pop_style();
                if !self.current_spans.is_empty() {
                    self.finish_line();
                }
            }
            TagEnd::List(_) => {
                self.list_stack.pop();
                if self.list_stack.is_empty() && !self.current_spans.is_empty() {
                    self.finish_line();
                }
            }
            TagEnd::Item => {
                self.finish_line();
            }
            TagEnd::FootnoteDefinition => {
                self.in_footnote_def = false;
                self.footnote_label.clear();
                if !self.current_spans.is_empty() {
                    self.finish_line();
                }
                self.lines.push(Line::default());
            }
            TagEnd::TableCell => {
                // Collect spans accumulated for this cell
                let cell_spans = std::mem::take(&mut self.current_spans);
                self.table_row_cells.push(cell_spans);
            }
            TagEnd::TableHead => {
                // The header row is done
                let row = std::mem::take(&mut self.table_row_cells);
                self.table_rows.push(row);
                self.table_header_row_count = 1;
                self.in_table_head = false;
            }
            TagEnd::TableRow => {
                let row = std::mem::take(&mut self.table_row_cells);
                self.table_rows.push(row);
            }
            TagEnd::Table => {
                self.flush_table();
                self.in_table = false;
                self.table_alignments.clear();
                self.table_rows.clear();
                self.table_header_row_count = 0;
            }
            TagEnd::Paragraph => {
                if self.in_footnote_def {
                    // Inside a footnote definition: don't add extra blank line
                    if !self.current_spans.is_empty() {
                        self.finish_line();
                    }
                } else {
                    self.finish_line();
                    self.lines.push(Line::default());
                }
            }
            _ => {}
        }
    }

    // ── Table flushing ───────────────────────────────────────────────

    /// Render all collected table rows as pipe-separated output with
    /// box-drawing separator under the header.
    fn flush_table(&mut self) {
        if self.table_rows.is_empty() {
            return;
        }

        let num_cols = self.table_rows.iter().map(|r| r.len()).max().unwrap_or(0);

        if num_cols == 0 {
            return;
        }

        // Compute column widths from plain-text content.
        let mut col_widths = vec![0usize; num_cols];
        for row in &self.table_rows {
            for (ci, cell) in row.iter().enumerate() {
                let text_len: usize = cell.iter().map(|s| s.content.len()).sum();
                col_widths[ci] = col_widths[ci].max(text_len);
            }
        }

        // Ensure a minimum width of 3 per column.
        for w in &mut col_widths {
            *w = (*w).max(3);
        }

        let border_style = Style::default().fg(theme::table_border());
        let header_style = Style::default()
            .fg(theme::table_header())
            .add_modifier(Modifier::BOLD);

        for (ri, row) in self.table_rows.iter().enumerate() {
            let is_header = ri < self.table_header_row_count;
            let mut spans: Vec<Span<'static>> = Vec::new();

            spans.push(Span::styled("\u{2502} ", border_style));

            for (ci, &col_w) in col_widths.iter().enumerate().take(num_cols) {
                let cell_spans = row.get(ci);
                let text_len: usize = cell_spans
                    .map(|cs| cs.iter().map(|s| s.content.len()).sum())
                    .unwrap_or(0);
                let pad = col_w.saturating_sub(text_len);

                let alignment = self
                    .table_alignments
                    .get(ci)
                    .copied()
                    .unwrap_or(Alignment::None);

                let (pad_left, pad_right) = match alignment {
                    Alignment::Right => (pad, 0),
                    Alignment::Center => {
                        let left = pad / 2;
                        (left, pad - left)
                    }
                    // Left and None: content first, then pad
                    _ => (0, pad),
                };

                if pad_left > 0 {
                    spans.push(Span::styled(" ".repeat(pad_left), Style::default()));
                }

                if let Some(cell) = cell_spans {
                    for s in cell {
                        let mut style = s.style;
                        if is_header {
                            style = style.patch(header_style);
                        }
                        spans.push(Span::styled(s.content.clone(), style));
                    }
                }

                if pad_right > 0 {
                    spans.push(Span::styled(" ".repeat(pad_right), Style::default()));
                }

                spans.push(Span::styled(" \u{2502} ", border_style));
            }

            self.lines.push(Line::from(spans));

            // After header row, draw separator
            if is_header {
                let mut sep_parts = Vec::new();
                sep_parts.push(Span::styled("\u{251C}", border_style));
                for (ci, &w) in col_widths.iter().enumerate() {
                    let fill = "\u{2500}".repeat(w + 2); // +2 for padding
                    sep_parts.push(Span::styled(fill, border_style));
                    if ci < num_cols - 1 {
                        sep_parts.push(Span::styled("\u{253C}", border_style));
                    }
                }
                sep_parts.push(Span::styled("\u{2524}", border_style));
                self.lines.push(Line::from(sep_parts));
            }
        }

        self.lines.push(Line::default());
    }

    fn finish(self) -> Text<'static> {
        Text::from(self.lines)
    }
}

fn highlight_code_line(lang: &str, code_line: &str) -> Vec<(Style, String)> {
    match code_block_highlighter(lang) {
        CodeBlockHighlighter::Jones(highlighter) => highlighter.highlight_line(code_line),
        CodeBlockHighlighter::Plain => vec![(
            Style::default().fg(theme::code_block_fg()),
            code_line.to_string(),
        )],
    }
}

enum CodeBlockHighlighter {
    Jones(Highlighter),
    Plain,
}

fn code_block_highlighter(lang: &str) -> CodeBlockHighlighter {
    let lang = lang
        .split(|c: char| c.is_whitespace() || c == ',' || c == '{')
        .next()
        .unwrap_or("")
        .trim_start_matches('.')
        .to_ascii_lowercase();

    match lang.as_str() {
        "md" | "markdown" => CodeBlockHighlighter::Jones(Highlighter::Markdown),
        "json" => CodeBlockHighlighter::Jones(Highlighter::Json),
        "toml" => CodeBlockHighlighter::Jones(Highlighter::Toml),
        "rs" | "rust" => CodeBlockHighlighter::Jones(Highlighter::Rust),
        "py" | "python" | "python3" => CodeBlockHighlighter::Jones(Highlighter::Python),
        "bash" | "sh" | "shell" | "zsh" => CodeBlockHighlighter::Jones(Highlighter::Shell),
        _ => CodeBlockHighlighter::Plain,
    }
}

/// Map a pulldown_cmark HeadingLevel to a theme color — full H1-H6 support.
fn heading_color(level: HeadingLevel) -> ratatui::style::Color {
    match level {
        HeadingLevel::H1 => theme::heading_h1(),
        HeadingLevel::H2 => theme::heading_h2(),
        HeadingLevel::H3 => theme::heading_h3(),
        HeadingLevel::H4 => theme::heading_h4(),
        HeadingLevel::H5 => theme::heading_h5(),
        HeadingLevel::H6 => theme::heading_h6(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Collect all text content from a rendered Text.
    fn collect_text(text: &Text<'_>) -> String {
        text.lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect()
    }

    #[test]
    fn renders_heading() {
        let text = render_markdown("# Hello");
        assert!(!text.lines.is_empty());
        let first_line_text: String = text.lines[0]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(first_line_text.contains("Hello"));
    }

    #[test]
    fn renders_bullet_list() {
        let text = render_markdown("- one\n- two\n- three");
        let all_text = collect_text(&text);
        assert!(all_text.contains("one"));
        assert!(all_text.contains("two"));
        assert!(all_text.contains("three"));
        // Should use unicode bullet
        assert!(all_text.contains('\u{2022}'));
    }

    #[test]
    fn renders_inline_code() {
        let text = render_markdown("use `foo` here");
        let all_text = collect_text(&text);
        assert!(all_text.contains("`foo`"));
    }

    #[test]
    fn renders_horizontal_rule() {
        let text = render_markdown("above\n\n---\n\nbelow");
        let all_text = collect_text(&text);
        assert!(all_text.contains('\u{2500}'));
    }

    #[test]
    fn renders_blockquote_with_border() {
        let text = render_markdown("> quoted text");
        let all_text = collect_text(&text);
        assert!(all_text.contains('\u{2502}'));
        assert!(all_text.contains("quoted text"));
    }

    #[test]
    fn renders_ordered_list_items() {
        let text = render_markdown("1. one\n2. two");
        let all_text = collect_text(&text);
        assert!(all_text.contains("1. one"));
        assert!(all_text.contains("2. two"));
    }

    #[test]
    fn renders_task_list_marker() {
        let text = render_markdown("- [x] done\n- [ ] todo");
        let all_text = collect_text(&text);
        assert!(all_text.contains('\u{2713}'));
        assert!(all_text.contains("done"));
    }

    #[test]
    fn renders_all_heading_levels() {
        let text = render_markdown("# H1\n## H2\n### H3\n#### H4\n##### H5\n###### H6");
        let all_text = collect_text(&text);
        for h in ["H1", "H2", "H3", "H4", "H5", "H6"] {
            assert!(all_text.contains(h));
        }
    }

    // ── New feature tests ────────────────────────────────────────────

    #[test]
    fn renders_table_with_header_and_body() {
        let md = "| Name | Age |\n|------|-----|\n| Alice | 30 |\n| Bob | 25 |";
        let text = render_markdown(md);
        let all_text = collect_text(&text);
        assert!(all_text.contains("Name"));
        assert!(all_text.contains("Age"));
        assert!(all_text.contains("Alice"));
        assert!(all_text.contains("Bob"));
        // Should contain box-drawing border characters
        assert!(all_text.contains('\u{2502}')); // │
        assert!(all_text.contains('\u{2500}')); // ─
    }

    #[test]
    fn renders_table_header_separator() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |";
        let text = render_markdown(md);
        let all_text = collect_text(&text);
        // Separator uses ├ and ┤
        assert!(all_text.contains('\u{251C}')); // ├
        assert!(all_text.contains('\u{2524}')); // ┤
    }

    #[test]
    fn renders_image_alt_text() {
        let md = "![my cool image](http://example.com/img.png)";
        let text = render_markdown(md);
        let all_text = collect_text(&text);
        assert!(all_text.contains("[img: my cool image]"));
    }

    #[test]
    fn renders_image_without_alt() {
        let md = "![](http://example.com/img.png)";
        let text = render_markdown(md);
        let all_text = collect_text(&text);
        assert!(all_text.contains("[img]"));
    }

    #[test]
    fn renders_nested_blockquotes() {
        let md = "> level 1\n>> level 2\n>>> level 3";
        let text = render_markdown(md);
        let all_text = collect_text(&text);
        assert!(all_text.contains("level 1"));
        assert!(all_text.contains("level 2"));
        assert!(all_text.contains("level 3"));
        // Should have multiple │ indicators for deeper levels
        // Check for at least one line with triple depth indicator
        let has_deep = text.lines.iter().any(|line| {
            let line_text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            line_text.contains("\u{2502} \u{2502} \u{2502} ")
        });
        assert!(has_deep, "Should have triple-depth blockquote indicator");
    }

    #[test]
    fn renders_code_block_language_label() {
        let md = "```rust\nfn main() {}\n```";
        let text = render_markdown(md);
        let all_text = collect_text(&text);
        assert!(all_text.contains("[rust]"));
        assert!(all_text.contains("fn main()"));
    }

    #[test]
    fn renders_code_block_borders() {
        let md = "```rust\nfn main() {}\n```";
        let text = render_markdown(md);
        let all_text = collect_text(&text);
        assert!(all_text.contains('┌'));
        assert!(all_text.contains('└'));
    }

    #[test]
    fn renders_code_block_no_label_when_no_lang() {
        let md = "```\nplain code\n```";
        let text = render_markdown(md);
        let all_text = collect_text(&text);
        assert!(all_text.contains("plain code"));
        // No language label brackets (except code content)
        let has_label = text.lines.iter().any(|line| {
            line.spans.iter().any(|s| {
                let content = s.content.as_ref();
                content.starts_with('[') && content.ends_with(']') && content.len() > 2
            })
        });
        assert!(
            !has_label,
            "Should not have a language label for bare fenced blocks"
        );
    }

    #[test]
    fn highlights_fenced_python_code_block() {
        let md = "```python\ndef hello(name):\n    return f\"hi {name}\" # greet\n```";
        let text = render_markdown(md);
        let code_line = text
            .lines
            .iter()
            .find(|line| {
                let line_text: String = line
                    .spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect();
                line_text.contains("def hello")
            })
            .expect("python code line should render");

        let line_text: String = code_line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();

        assert_eq!(line_text, "  def hello(name):");
        assert!(
            code_line.spans.len() > 2,
            "python code should be split into highlighted spans"
        );
        assert!(code_line.spans.iter().any(|span| {
            span.content.as_ref() == "def" && span.style.fg != Some(theme::code_block_fg())
        }));
    }

    #[test]
    fn highlights_fenced_bash_code_block() {
        let md = "```bash\nif [ -n \"$NAME\" ]; then\n  echo \"hi $NAME\" # greet\nfi\n```";
        let text = render_markdown(md);
        let code_line = text
            .lines
            .iter()
            .find(|line| {
                line.spans
                    .iter()
                    .any(|span| span.content.as_ref().contains("$NAME"))
            })
            .expect("bash code line should render");

        let line_text: String = code_line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();

        assert_eq!(line_text, "  if [ -n \"$NAME\" ]; then");
        assert!(
            code_line.spans.len() > 2,
            "bash code should be split into highlighted spans"
        );
        assert!(code_line.spans.iter().any(|span| {
            span.content.as_ref() == "if" && span.style.fg != Some(theme::code_block_fg())
        }));
    }

    #[test]
    fn renders_nested_list_with_different_bullets() {
        let md = "- outer\n  - middle\n    - inner";
        let text = render_markdown(md);
        let all_text = collect_text(&text);
        assert!(all_text.contains("outer"));
        assert!(all_text.contains("middle"));
        assert!(all_text.contains("inner"));
        // Depth 0: • , depth 1: ◦ , depth 2: ▪
        assert!(all_text.contains('\u{2022}')); // •
        assert!(all_text.contains('\u{25E6}')); // ◦
        assert!(all_text.contains('\u{25AA}')); // ▪
    }

    #[test]
    fn renders_footnote_reference() {
        let md = "Text with a footnote[^1].\n\n[^1]: This is the footnote.";
        let text = render_markdown(md);
        let all_text = collect_text(&text);
        assert!(all_text.contains("[^1]"));
    }

    #[test]
    fn renders_footnote_definition() {
        let md = "Some text[^note].\n\n[^note]: The footnote content.";
        let text = render_markdown(md);
        let all_text = collect_text(&text);
        // Should contain the reference marker
        assert!(all_text.contains("[^note]"));
    }
}
