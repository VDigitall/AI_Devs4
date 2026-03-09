use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};

pub struct LogViewWidget<'a> {
    pub logs: &'a [String],
    pub scroll_offset: u16,
}

impl<'a> LogViewWidget<'a> {
    pub fn new(logs: &'a [String], scroll_offset: u16) -> Self {
        Self { logs, scroll_offset }
    }
}

impl<'a> Widget for LogViewWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let lines: Vec<Line> = if self.logs.is_empty() {
            vec![Line::from(Span::styled(
                "Waiting for output...",
                Style::default().fg(Color::DarkGray),
            ))]
        } else {
            self.logs
                .iter()
                .map(|entry| {
                    // Colour-code by content prefix
                    let style = if entry.contains("error") || entry.contains("Error") || entry.contains("failed") {
                        Style::default().fg(Color::Red)
                    } else if entry.contains("Flag") || entry.contains("FLG:") || entry.contains("flag") {
                        Style::default().fg(Color::Yellow)
                    } else if entry.contains("completed") || entry.contains("done") || entry.contains("✓") {
                        Style::default().fg(Color::Green)
                    } else if entry.contains("Planning") || entry.contains("Plan ready") {
                        Style::default().fg(Color::Magenta)
                    } else if entry.contains("Step") {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    Line::from(Span::styled(entry.clone(), style))
                })
                .collect()
        };

        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .title(" Log ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Green)),
            )
            .wrap(Wrap { trim: false })
            .scroll((self.scroll_offset, 0));

        Widget::render(paragraph, area, buf);
    }
}
