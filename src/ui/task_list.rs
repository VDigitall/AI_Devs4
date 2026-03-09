use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, StatefulWidget, Widget},
};

pub struct TaskListWidget<'a> {
    pub tasks: &'a [String],
    pub state: &'a mut ListState,
}

impl<'a> TaskListWidget<'a> {
    pub fn new(tasks: &'a [String], state: &'a mut ListState) -> Self {
        Self { tasks, state }
    }
}

impl<'a> Widget for TaskListWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let items: Vec<ListItem> = self
            .tasks
            .iter()
            .map(|name| {
                ListItem::new(Line::from(vec![Span::raw(name.clone())]))
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .title(" Tasks ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        StatefulWidget::render(list, area, buf, self.state);
    }
}
