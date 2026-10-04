/// View, render screen from state (app)
/// Using the 'ratatui' crate
pub mod common;
pub mod lines;
pub mod popups;
pub mod status_bar;
pub mod terminal;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    widgets::{Block, Borders},
};

use crate::{
    app::App,
    configs::{InputMode, KeywordHighlight, ThemeConfig},
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
        status_bar::render_status_bar,
        terminal::{render_timestamp_gutter, render_vt100_terminal},
    },
};

#[derive(Debug, PartialEq, Eq)]
pub enum PostRenderEffect {
    None,
    CopyToClipboard(String),
}

pub fn render(
    app: &mut App,
    theme: &ThemeConfig,
    frame: &mut Frame,
    highlights: &[KeywordHighlight],
) -> PostRenderEffect {
    let screen_area = frame.area();

    let (monitor_area, prompt_area, status_bar_area, focus_target): (
        Rect,
        Option<Rect>,
        Rect,
        FocusTarget,
    ) = match app.settings().input_mode {
        InputMode::Terminal => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3), Constraint::Length(1)])
                .split(screen_area);
            (chunks[0], None, chunks[1], FocusTarget::Terminal)
        }
        InputMode::Line => {
            let prompt_height = if app.settings().ui.show_borders { 3 } else { 1 };
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(3),
                    Constraint::Length(prompt_height),
                    Constraint::Length(1),
                ])
                .split(screen_area);
            (
                chunks[0],
                Some(chunks[1]),
                chunks[2],
                FocusTarget::LineHistory,
            )
        }
    };

    let focus = app.focus_target();
    let border_style = border_style_for(focus_target, focus, app.is_app_focused(), theme);

    let mut block = Block::default();
    if app.settings().ui.show_borders {
        block = block.borders(Borders::ALL).border_style(border_style);
    } else {
        block = block.borders(Borders::NONE);
    }

    let inner_area = block.inner(monitor_area);

    let sample_ts = chrono::Local::now();
    let ts_width = sample_ts
        .format(&app.settings().timestamp_format)
        .to_string()
        .chars()
        .count();

    let (gutter_area, terminal_area) = match app.settings().input_mode {
        InputMode::Terminal if app.settings().display_live_timestamp => {
            let gutter_width = (ts_width.saturating_add(1) as u16).min(inner_area.width);
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(gutter_width), Constraint::Min(0)])
                .split(inner_area);
            (Some(chunks[0]), chunks[1])
        }
        _ => (None, inner_area),
    };

    app.update_viewport(ViewportSize::new(terminal_area.width, terminal_area.height));

    if app.settings().ui.show_borders {
        let title = monitor_title(app, &monitor_area);
        block = block.title(title);

        let bottom_titles = monitor_bottom_title(app);
        block = bottom_titles
            .into_iter()
            .fold(block, |b, t| b.title_bottom(t));
    }
    // TODO: handle rendering search when borders off
    // TODO: handle the vertical scroll bar when borders off

    frame.render_widget(block, monitor_area);

    let mut render_effect = PostRenderEffect::None;
    match app.settings().input_mode {
        InputMode::Terminal => {
            if let Some(g_area) = gutter_area {
                render_timestamp_gutter(app, frame, g_area, theme);
            }
            render_vt100_terminal(app, frame, terminal_area, monitor_area, theme, highlights);
        }
        InputMode::Line => {
            render_effect = render_monitor(app, frame, inner_area, monitor_area, theme, highlights);
        }
    }

    if app.settings().ui.show_status_bar {
        render_status_bar(app, frame, status_bar_area, theme);
    }

    if let Some(prompt_rect) = prompt_area {
        render_input_prompt(app, frame, prompt_rect, theme);
    }

    render_notifications(&app.notifications, frame);

    match app.overlay() {
        Some(Overlay::Settings) => render_settings_popup(app, frame, theme),
        Some(Overlay::Help(dialog)) => render_help_popup(app, dialog, frame, theme),
        Some(Overlay::TerminalSize(dialog)) => render_resize_popup(app, dialog, frame, theme),
        Some(Overlay::SerialSettings(dialog)) => render_reconfigure_popup(dialog, frame),
        Some(Overlay::FilePath(dialog)) => render_filepath_popup(app, dialog, frame, theme),
        Some(Overlay::DeviceDisconnected(port)) => render_disconnected_popup(port, frame),
        Some(Overlay::Error(err)) => render_error_popup(err, frame),
        None => {}
    }

    render_effect
}
