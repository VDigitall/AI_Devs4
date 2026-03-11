pub mod log_view;
pub mod plan_view;
pub mod status_bar;
pub mod task_list;

use ratatui::{
    layout::{Constraint, Direction, Layout},
    Frame,
};

use crate::app::App;
use log_view::LogViewWidget;
use plan_view::PlanViewWidget;
use status_bar::StatusBarWidget;
use task_list::TaskListWidget;

pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    // Outer layout: main content + status bar
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(area);

    let main_area = outer[0];
    let status_area = outer[1];

    // Main layout: task list (left) + right panels (plan + log)
    let main_split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(22), Constraint::Min(0)])
        .split(main_area);

    let left_area = main_split[0];
    let right_area = main_split[1];

    // Right: plan (top) + log (bottom)
    let right_split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(right_area);

    let plan_area = right_split[0];
    let log_area = right_split[1];

    // ── Task list ──────────────────────────────────────────────────────────────
    frame.render_widget(
        TaskListWidget::new(&app.task_names, &mut app.task_list_state),
        left_area,
    );

    // ── Plan view ──────────────────────────────────────────────────────────────
    frame.render_widget(
        PlanViewWidget::new(&app.plan_steps, &app.sub_plan_steps, app.current_step, &app.secrets, app.reveal_flags),
        plan_area,
    );

    // ── Log view ───────────────────────────────────────────────────────────────
    frame.render_widget(
        LogViewWidget::new(&app.logs, app.log_scroll, &app.secrets, app.reveal_flags),
        log_area,
    );

    // ── Status bar ─────────────────────────────────────────────────────────────
    let selected_name = app
        .task_list_state
        .selected()
        .and_then(|i| app.task_names.get(i))
        .map(|s| s.as_str());

    frame.render_widget(
        StatusBarWidget::new(&app.state, selected_name, app.reveal_flags),
        status_area,
    );
}
