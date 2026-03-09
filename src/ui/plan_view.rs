use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};

use crate::agent::{PlanStep, StepStatus};

pub struct PlanViewWidget<'a> {
    pub steps: &'a [PlanStep],
    pub current_step: Option<usize>,
}

impl<'a> PlanViewWidget<'a> {
    pub fn new(steps: &'a [PlanStep], current_step: Option<usize>) -> Self {
        Self { steps, current_step }
    }
}

impl<'a> Widget for PlanViewWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let lines: Vec<Line> = if self.steps.is_empty() {
            vec![Line::from(Span::styled(
                "No plan yet. Select a task and press Enter.",
                Style::default().fg(Color::DarkGray),
            ))]
        } else {
            self.steps
                .iter()
                .enumerate()
                .map(|(i, step)| {
                    let (icon, style) = match &step.status {
                        StepStatus::Pending => (
                            "○",
                            Style::default().fg(Color::DarkGray),
                        ),
                        StepStatus::Running => (
                            "►",
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        ),
                        StepStatus::Done => ("✓", Style::default().fg(Color::Green)),
                        StepStatus::Failed(_) => ("✗", Style::default().fg(Color::Red)),
                        StepStatus::MissingTool => (
                            "?",
                            Style::default()
                                .fg(Color::Magenta)
                                .add_modifier(Modifier::BOLD),
                        ),
                    };

                    let number = format!("{:2}. ", i + 1);
                    let tool = format!("{} ", step.tool_name);
                    let args_preview = truncate_args(&step.arguments, 40);

                    Line::from(vec![
                        Span::styled(number, Style::default().fg(Color::DarkGray)),
                        Span::styled(icon, style),
                        Span::raw(" "),
                        Span::styled(tool, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                        Span::styled(args_preview, Style::default().fg(Color::Gray)),
                    ])
                })
                .collect()
        };

        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .title(" Plan ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Magenta)),
            )
            .wrap(Wrap { trim: true });

        Widget::render(paragraph, area, buf);
    }
}

fn truncate_args(args: &str, max_len: usize) -> String {
    // Show a compact one-line preview of the arguments JSON
    let compact = args.replace('\n', " ").replace("  ", " ");
    if compact.len() > max_len {
        format!("{}…", &compact[..max_len])
    } else {
        compact
    }
}
