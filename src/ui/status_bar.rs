use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

use crate::app::AppState;

pub struct StatusBarWidget<'a> {
    pub state: &'a AppState,
    pub task_name: Option<&'a str>,
}

impl<'a> StatusBarWidget<'a> {
    pub fn new(state: &'a AppState, task_name: Option<&'a str>) -> Self {
        Self { state, task_name }
    }
}

impl<'a> Widget for StatusBarWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let state_label = match self.state {
            AppState::Idle => Span::styled(
                " IDLE ",
                Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            AppState::Planning => Span::styled(
                " PLANNING ",
                Style::default().fg(Color::Black).bg(Color::Magenta).add_modifier(Modifier::BOLD),
            ),
            AppState::Executing { .. } => Span::styled(
                " EXECUTING ",
                Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            AppState::Done => Span::styled(
                " DONE ",
                Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD),
            ),
            AppState::Error(_) => Span::styled(
                " ERROR ",
                Style::default().fg(Color::White).bg(Color::Red).add_modifier(Modifier::BOLD),
            ),
        };

        let task_span = if let Some(name) = self.task_name {
            Span::styled(
                format!(" Task: {name} "),
                Style::default().fg(Color::White),
            )
        } else {
            Span::styled(" No task selected ", Style::default().fg(Color::DarkGray))
        };

        let error_span = if let AppState::Error(e) = self.state {
            Span::styled(
                format!(" {e} "),
                Style::default().fg(Color::Red),
            )
        } else {
            Span::raw("")
        };

        let keys = Span::styled(
            " [↑↓] Navigate  [Enter] Run  [q] Quit  [PgUp/PgDn] Scroll Log ",
            Style::default().fg(Color::DarkGray),
        );

        let line = Line::from(vec![
            state_label,
            Span::raw(" "),
            task_span,
            error_span,
            Span::raw("  "),
            keys,
        ]);

        let paragraph = Paragraph::new(line).block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(Color::DarkGray)),
        );

        Widget::render(paragraph, area, buf);
    }
}
