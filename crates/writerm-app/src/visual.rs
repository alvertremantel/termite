use jones_render::{RenderedDocument, RenderedLine};
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use unicode_width::UnicodeWidthChar;

#[derive(Debug, Clone)]
pub struct VisualDocument {
    pub rows: Vec<VisualRow>,
}

#[derive(Debug, Clone)]
pub struct VisualRow {
    spans: Vec<VisualSpan>,
    col_sources: Vec<usize>,
    boundaries: Vec<(usize, usize)>,
    source_start: usize,
    source_end: usize,
}

#[derive(Debug, Clone)]
struct VisualSpan {
    content: String,
    style: Style,
}

#[derive(Debug, Clone)]
struct Cell {
    ch: char,
    style: Style,
    source: Option<(usize, usize)>,
}

impl VisualDocument {
    pub fn from_source(input: &str, width: usize, style: Style) -> Self {
        let width = width.max(1);
        let mut rows = Vec::new();
        let mut char_start = 0usize;

        for line in input.split_inclusive('\n') {
            let content = line.trim_end_matches('\n').trim_end_matches('\r');
            let line_len = content.chars().count();
            let cells = content
                .chars()
                .enumerate()
                .map(|(idx, ch)| Cell {
                    ch,
                    style,
                    source: Some((char_start + idx, char_start + idx + 1)),
                })
                .collect::<Vec<_>>();
            rows.extend(wrap_cells(
                cells,
                Some((char_start, char_start + line_len)),
                width,
            ));
            char_start += line.chars().count();
        }

        if input.is_empty() || input.ends_with('\n') {
            rows.push(VisualRow::blank(char_start));
        }

        Self { rows }
    }

    pub fn from_rendered(rendered: &RenderedDocument, width: usize) -> Self {
        let width = width.max(1);
        let mut rows = Vec::new();
        for line in &rendered.lines {
            rows.extend(wrap_cells(
                rendered_line_cells(line),
                line.source
                    .as_ref()
                    .map(|source| (source.char_start, source.char_end)),
                width,
            ));
        }
        if rows.is_empty() {
            rows.push(VisualRow::blank(0));
        }
        Self { rows }
    }

    pub fn to_text(&self, scroll: usize, height: usize) -> Text<'static> {
        Text::from(
            self.rows
                .iter()
                .skip(scroll)
                .take(height)
                .map(VisualRow::to_line)
                .collect::<Vec<_>>(),
        )
    }

    pub fn display_to_source(&self, row: usize, col: usize) -> Option<usize> {
        self.rows.get(row).map(|row| row.source_at_col(col))
    }

    pub fn source_to_display(&self, char_pos: usize) -> Option<(usize, usize)> {
        let mut closest = None;
        for (row_idx, row) in self.rows.iter().enumerate() {
            if char_pos < row.source_start {
                continue;
            }
            if char_pos <= row.source_end {
                return Some((row_idx, row.col_for_source(char_pos)));
            }
            closest = Some((row_idx, row.width()));
        }
        closest
    }
}

impl VisualRow {
    fn blank(source: usize) -> Self {
        Self {
            spans: Vec::new(),
            col_sources: Vec::new(),
            boundaries: vec![(source, 0)],
            source_start: source,
            source_end: source,
        }
    }

    fn from_cells(mut cells: Vec<Cell>) -> Self {
        trim_edge_spaces(&mut cells);
        if cells.is_empty() {
            return Self::blank(0);
        }

        let first_source = cells
            .iter()
            .find_map(|cell| cell.source.map(|(start, _)| start));
        let last_source = cells
            .iter()
            .rev()
            .find_map(|cell| cell.source.map(|(_, end)| end));
        let source_start = first_source.unwrap_or(0);
        let source_end = last_source.unwrap_or(source_start);

        let fallback_source = first_source.unwrap_or(source_start);
        let mut spans = Vec::new();
        let mut col_sources = Vec::new();
        let mut boundaries = vec![(source_start, 0)];
        let mut col = 0usize;

        for cell in cells {
            push_cell_span(&mut spans, cell.ch, cell.style);
            let width = cell_width(cell.ch);
            if let Some((start, end)) = cell.source {
                boundaries.push((start, col));
                boundaries.push((end, col + width));
                col_sources.extend(std::iter::repeat_n(start, width));
            } else {
                col_sources.extend(std::iter::repeat_n(fallback_source, width));
            }
            col += width;
        }
        boundaries.push((source_end, col));
        boundaries.sort_unstable();
        boundaries.dedup();

        Self {
            spans,
            col_sources,
            boundaries,
            source_start,
            source_end,
        }
    }

    fn include_source_start(&mut self, source: usize) {
        self.source_start = self.source_start.min(source);
        self.boundaries.push((self.source_start, 0));
        self.boundaries.sort_unstable();
        self.boundaries.dedup();
    }

    fn include_source_end(&mut self, source: usize) {
        self.source_end = self.source_end.max(source);
        self.boundaries.push((self.source_end, self.width()));
        self.boundaries.sort_unstable();
        self.boundaries.dedup();
    }

    fn to_line(&self) -> Line<'static> {
        Line::from(
            self.spans
                .iter()
                .map(|span| Span::styled(span.content.clone(), span.style))
                .collect::<Vec<_>>(),
        )
    }

    fn width(&self) -> usize {
        self.col_sources.len()
    }

    fn source_at_col(&self, col: usize) -> usize {
        self.col_sources
            .get(col)
            .copied()
            .unwrap_or(self.source_end)
    }

    fn col_for_source(&self, char_pos: usize) -> usize {
        let mut best_col = 0usize;
        for (source, col) in &self.boundaries {
            if *source == char_pos {
                return *col;
            }
            if *source > char_pos {
                return best_col;
            }
            best_col = *col;
        }
        self.width()
    }
}

fn wrap_cells(
    cells: Vec<Cell>,
    line_source: Option<(usize, usize)>,
    width: usize,
) -> Vec<VisualRow> {
    if cells.is_empty() {
        return vec![
            line_source
                .map(|(start, _)| VisualRow::blank(start))
                .unwrap_or_else(|| VisualRow::blank(0)),
        ];
    }

    let mut wrapper = CellWrapper::new(width);
    for cell in cells {
        wrapper.push(cell);
    }
    let mut rows = wrapper.finish();
    if let Some((start, end)) = line_source {
        if let Some(row) = rows.first_mut() {
            row.include_source_start(start);
        }
        if let Some(row) = rows.last_mut() {
            row.include_source_end(end);
        }
    }
    rows
}

struct CellWrapper {
    width: usize,
    rows: Vec<VisualRow>,
    current: Vec<Cell>,
    current_width: usize,
}

impl CellWrapper {
    fn new(width: usize) -> Self {
        Self {
            width,
            rows: Vec::new(),
            current: Vec::new(),
            current_width: 0,
        }
    }

    fn push(&mut self, cell: Cell) {
        let width = cell_width(cell.ch);
        if self.current_width + width <= self.width || self.current.is_empty() {
            self.push_unchecked(cell);
            return;
        }

        if cell.ch.is_whitespace() {
            trim_trailing_spaces(&mut self.current);
            self.recompute_width();
            self.flush_current();
            return;
        }

        if let Some(space_idx) = self
            .current
            .iter()
            .rposition(|cell| cell.ch.is_whitespace())
            && space_idx > 0
        {
            let mut carry = self.current.split_off(space_idx + 1);
            self.current.pop();
            trim_trailing_spaces(&mut self.current);
            self.recompute_width();
            self.flush_current();
            trim_leading_spaces(&mut carry);
            self.current = carry;
            self.recompute_width();
            self.push(cell);
            return;
        }

        self.flush_current();
        self.push(cell);
    }

    fn push_unchecked(&mut self, cell: Cell) {
        self.current_width += cell_width(cell.ch);
        self.current.push(cell);
    }

    fn flush_current(&mut self) {
        if self.current.is_empty() {
            return;
        }
        self.rows
            .push(VisualRow::from_cells(std::mem::take(&mut self.current)));
        self.current_width = 0;
    }

    fn recompute_width(&mut self) {
        self.current_width = self.current.iter().map(|cell| cell_width(cell.ch)).sum();
    }

    fn finish(mut self) -> Vec<VisualRow> {
        if !self.current.is_empty() {
            self.rows.push(VisualRow::from_cells(self.current));
        }
        if self.rows.is_empty() {
            self.rows.push(VisualRow::blank(0));
        }
        self.rows
    }
}

fn rendered_line_cells(line: &RenderedLine) -> Vec<Cell> {
    let mut cells = Vec::new();
    for span in &line.spans {
        for (idx, ch) in span.content.chars().enumerate() {
            cells.push(Cell {
                ch,
                style: span.style,
                source: span.source.as_ref().map(|source| {
                    let start = (source.char_start + idx).min(source.char_end);
                    let end = (start + 1).min(source.char_end);
                    (start, end)
                }),
            });
        }
    }
    cells
}

fn push_cell_span(spans: &mut Vec<VisualSpan>, ch: char, style: Style) {
    if let Some(last) = spans.last_mut()
        && last.style == style
    {
        last.content.push(ch);
        return;
    }
    spans.push(VisualSpan {
        content: ch.to_string(),
        style,
    });
}

fn cell_width(ch: char) -> usize {
    UnicodeWidthChar::width(ch).unwrap_or(0)
}

fn trim_edge_spaces(cells: &mut Vec<Cell>) {
    trim_leading_spaces(cells);
    trim_trailing_spaces(cells);
}

fn trim_leading_spaces(cells: &mut Vec<Cell>) {
    let keep_from = cells
        .iter()
        .position(|cell| !cell.ch.is_whitespace())
        .unwrap_or(cells.len());
    if keep_from > 0 {
        cells.drain(..keep_from);
    }
}

fn trim_trailing_spaces(cells: &mut Vec<Cell>) {
    while cells.last().is_some_and(|cell| cell.ch.is_whitespace()) {
        cells.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jones_render::render_markdown_mapped;

    #[test]
    fn source_wraps_on_words_without_changing_source_positions() {
        let doc = VisualDocument::from_source("alpha beta gamma", 10, Style::default());

        assert_eq!(doc.rows.len(), 2);
        assert_eq!(doc.rows[0].to_line().to_string(), "alpha beta");
        assert_eq!(doc.rows[1].to_line().to_string(), "gamma");
        assert_eq!(doc.display_to_source(1, 0), Some(11));
        assert_eq!(doc.source_to_display(13), Some((1, 2)));
    }

    #[test]
    fn wrapped_rendered_lines_keep_hidden_marker_mapping() {
        let rendered = render_markdown_mapped("# Alpha beta gamma");
        let doc = VisualDocument::from_rendered(&rendered, 10);

        assert_eq!(doc.source_to_display(0), Some((0, 0)));
        assert_eq!(doc.display_to_source(0, 0), Some(2));
        assert_eq!(doc.source_to_display(13), Some((1, 0)));
    }

    #[test]
    fn vertical_navigation_can_preserve_columns_across_short_rows() {
        let doc = VisualDocument::from_source("abcdefgh ij klmnopqr", 8, Style::default());

        assert_eq!(doc.source_to_display(6), Some((0, 6)));
        assert_eq!(doc.display_to_source(1, 6), Some(11));
        assert_eq!(doc.display_to_source(2, 6), Some(18));
    }
}
