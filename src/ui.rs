/// View, render screen from state (app)
/// Using the 'ratatui' crate
pub mod common;
pub mod lines;
pub mod popups;
pub mod terminal;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    widgets::{Block, Borders},
};

use crate::{
    app::App,
    config::InputMode,
    types::{FocusTarget, Overlay, ViewportSize},
    ui::{
        common::{border_style_for, monitor_bottom_title, monitor_title},
        lines::{render_input_prompt, render_monitor},
        popups::{
            alert::{render_disconnected_popup, render_error_popup},
            filepath::render_filepath_popup,
            help::render_help_popup,
            notifications::render_notifications,
            reconfigure::render_reconfigure_popup,
            resize::render_resize_popup,
            settings::render_settings_popup,
        },
        terminal::{render_timestamp_gutter, render_vt100_terminal},
    },
};

pub fn render(app: &mut App, frame: &mut Frame) {
    let screen_area = frame.area();

    let (monitor_area, prompt_area, focus_target): (Rect, Option<Rect>, FocusTarget) =
        match app.config().input_mode {
            InputMode::Terminal => (screen_area, None, FocusTarget::Terminal),
            InputMode::Line => {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Min(3), Constraint::Length(3)])
                    .split(screen_area);

                (chunks[0], Some(chunks[1]), FocusTarget::LineHistory)
            }
        };

    let focus = app.focus_target();
    let border_style = border_style_for(focus_target, focus, app.is_app_focused());

    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner_area = block.inner(monitor_area);

    let (gutter_area, terminal_area) = match app.config().input_mode {
        InputMode::Terminal if app.config().display_live_timestamp => {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(13), Constraint::Min(0)]) // 12 chars + 1 space/separator
                .split(inner_area);
            (Some(chunks[0]), chunks[1])
        }
        _ => (None, inner_area),
    };

    app.update_viewport(ViewportSize::new(terminal_area.width, terminal_area.height));

    let title = monitor_title(app, &monitor_area);
    block = block.title(title);

    let bottom_titles = monitor_bottom_title(app);
    block = bottom_titles
        .into_iter()
        .fold(block, |b, t| b.title_bottom(t));

    frame.render_widget(block, monitor_area);

    match app.config().input_mode {
        InputMode::Terminal => {
            if let Some(g_area) = gutter_area {
                render_timestamp_gutter(app, frame, g_area);
            }
            render_vt100_terminal(app, frame, terminal_area, monitor_area);
        }
        InputMode::Line => {
            render_monitor(app, frame, inner_area, monitor_area);
        }
    }

    if let Some(prompt_rect) = prompt_area {
        render_input_prompt(app, frame, prompt_rect);
    }

    render_notifications(&app.notifications, frame);

    match app.overlay() {
        Some(Overlay::Help) => render_help_popup(app, frame),
        Some(Overlay::Settings) => render_settings_popup(app, frame),
        Some(Overlay::TerminalSize(dialog)) => render_resize_popup(app, dialog, frame),
        Some(Overlay::SerialSettings(dialog)) => render_reconfigure_popup(dialog, frame),
        Some(Overlay::FilePath(dialog)) => render_filepath_popup(app, dialog, frame),
        Some(Overlay::DeviceDisconnected(port)) => render_disconnected_popup(port, frame),
        Some(Overlay::Error(err)) => render_error_popup(err, frame),
        None => {}
    }
}
