use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::{
    app::{App, types::FocusTarget},
    configs::InputMode,
};

pub struct ScreenLayout {
    pub monitor_outer: Rect,
    pub content_area: Rect,
    pub scrollbar_area: Option<Rect>,
    pub timestamp_gutter: Option<Rect>,
    pub prompt_area: Option<Rect>,
    pub status_bar_area: Option<Rect>,
    pub focus_target: FocusTarget,
}

impl ScreenLayout {
    pub fn compute(screen: Rect, app: &App) -> Self {
        let ui = &app.settings().ui;
        let show_borders = ui.show_borders;
        let show_status_bar = ui.show_status_bar;
        let show_scrollbar = ui.show_scrollbar;
        let input_mode = app.settings().input_mode;

        let status_height = if show_status_bar { 1 } else { 0 };
        let prompt_height = match input_mode {
            InputMode::Terminal => 0,
            InputMode::Line => {
                if show_borders {
                    3
                } else {
                    1
                }
            }
        };

        let v_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(prompt_height),
                Constraint::Length(status_height),
            ])
            .split(screen);

        let monitor_outer = v_chunks[0];
        let prompt_area = if prompt_height > 0 {
            Some(v_chunks[1])
        } else {
            None
        };
        let status_bar_area = if show_status_bar {
            Some(v_chunks[2])
        } else {
            None
        };

        let focus_target = match input_mode {
            InputMode::Terminal => FocusTarget::Terminal,
            InputMode::Line => FocusTarget::LineHistory,
        };

        let inner_monitor = if show_borders {
            Rect {
                x: monitor_outer.x.saturating_add(1),
                y: monitor_outer.y.saturating_add(1),
                width: monitor_outer.width.saturating_sub(2),
                height: monitor_outer.height.saturating_sub(2),
            }
        } else {
            monitor_outer
        };

        let (gutter_area, remaining_after_gutter) = match input_mode {
            InputMode::Terminal if app.settings().display_live_timestamp => {
                let sample_ts = chrono::Local::now();
                let ts_width = sample_ts
                    .format(&app.settings().timestamp_format)
                    .to_string()
                    .chars()
                    .count() as u16;

                let gutter_w = ts_width.saturating_add(1).min(inner_monitor.width);
                let chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Length(gutter_w), Constraint::Min(0)])
                    .split(inner_monitor);
                (Some(chunks[0]), chunks[1])
            }
            _ => (None, inner_monitor),
        };

        let (content_area, scrollbar_area) = if show_scrollbar {
            if show_borders {
                let sb_area = Rect {
                    x: monitor_outer.x + monitor_outer.width.saturating_sub(1),
                    y: monitor_outer.y + 1,
                    width: 1,
                    height: monitor_outer.height.saturating_sub(2),
                };
                (remaining_after_gutter, Some(sb_area))
            } else {
                let content = Rect {
                    x: remaining_after_gutter.x,
                    y: remaining_after_gutter.y,
                    width: remaining_after_gutter.width.saturating_sub(1),
                    height: remaining_after_gutter.height,
                };
                let sb = Rect {
                    x: remaining_after_gutter.x + remaining_after_gutter.width.saturating_sub(1),
                    y: remaining_after_gutter.y,
                    width: 1,
                    height: remaining_after_gutter.height,
                };
                (content, Some(sb))
            }
        } else {
            (remaining_after_gutter, None)
        };

        Self {
            monitor_outer,
            content_area,
            scrollbar_area,
            timestamp_gutter: gutter_area,
            prompt_area,
            status_bar_area,
            focus_target,
        }
    }
}
