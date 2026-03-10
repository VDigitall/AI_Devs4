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
    pub secrets: &'a [String],
    pub reveal_flags: bool,
}

impl<'a> LogViewWidget<'a> {
    pub fn new(
        logs: &'a [String],
        scroll_offset: u16,
        secrets: &'a [String],
        reveal_flags: bool,
    ) -> Self {
        Self { logs, scroll_offset, secrets, reveal_flags }
    }
}

/// Replace every occurrence of `{FLG:WORD}` with `{FLG:***}`.
fn mask_flags(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut remaining = s;
    let marker = "{FLG:";
    while let Some(start) = remaining.find(marker) {
        result.push_str(&remaining[..start]);
        let after_marker = &remaining[start + marker.len()..];
        if let Some(end) = after_marker.find('}') {
            result.push_str("{FLG:***}");
            remaining = &after_marker[end + 1..];
        } else {
            // No closing brace — keep the rest as-is
            result.push_str(&remaining[start..]);
            return result;
        }
    }
    result.push_str(remaining);
    result
}

/// Replace every known secret value with `[KEY:***]`.
fn mask_secrets<'s>(s: &str, secrets: &'s [String]) -> String {
    let mut result = s.to_string();
    for secret in secrets {
        if !secret.is_empty() {
            result = result.replace(secret.as_str(), "[KEY:***]");
        }
    }
    result
}

/// Apply all TUI-only masking rules to a single log line.
fn mask_line(line: &str, secrets: &[String], reveal_flags: bool) -> String {
    let after_secrets = mask_secrets(line, secrets);
    if reveal_flags {
        after_secrets
    } else {
        mask_flags(&after_secrets)
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
                    let display = mask_line(entry, self.secrets, self.reveal_flags);

                    // Colour-code by content prefix (use original for detection,
                    // masked string for display)
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
                    Line::from(Span::styled(display, style))
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
