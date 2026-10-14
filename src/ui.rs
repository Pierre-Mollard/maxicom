/// View, render screen from state (app)
/// Using the 'ratatui' crate
pub mod common;
pub mod data_display;
pub mod layout;
pub mod lines;
pub mod popups;
pub mod raw_side;
pub mod status_bar;
pub mod terminal;

use ratatui::{
    Frame,
    widgets::{Block, Borders},
};

use crate::{
    app::{App, types::ViewportSize},
    configs::{KeywordHighlight, Mode, ThemeConfig},
    overlays::Overlay,
    ui::{
        common::{border_style_for, monitor_bottom_title, monitor_title},
        layout::{LayoutWarning, ScreenLayout},
        lines::{render_input_prompt, render_monitor},
        popups::{
            alert::{render_detached_popup, render_disconnected_popup, render_error_popup},
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
    LayoutWarnings(Vec<LayoutWarning>),
    _CopyToClipboard(String),
}

pub fn render(
    app: &mut App,
    theme: &ThemeConfig,
    frame: &mut Frame,
    highlights: &[KeywordHighlight],
) -> PostRenderEffect {
    let layout = ScreenLayout::compute(frame.area(), app);
    app.update_viewport(ViewportSize::new(
        layout.content_area.width,
        layout.content_area.height,
    ));

    if app.settings().ui.show_borders {
        let border_style = border_style_for(
            layout.focus_target,
            app.focus_target(),
            app.is_app_focused(),
            theme,
        );

        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(monitor_title(app, &layout.monitor_outer));

        for bottom_title in monitor_bottom_title(app) {
            block = block.title_bottom(bottom_title);
        }

        frame.render_widget(block, layout.monitor_outer);
    }

    let post_effect = match app.settings().mode {
        Mode::Terminal => {
            if let Some(g_area) = layout.timestamp_gutter {
                render_timestamp_gutter(app, frame, g_area, theme);
            }
            render_vt100_terminal(
                app,
                frame,
                layout.content_area,
                layout.raw_side_area,
                layout.scrollbar_area,
                theme,
                highlights,
            );
            PostRenderEffect::None
        }
        Mode::Line => render_monitor(
            app,
            frame,
            layout.content_area,
            layout.raw_side_area,
            layout.scrollbar_area,
            theme,
            highlights,
        ),
    };

    if let Some(prompt_rect) = layout.prompt_area {
        render_input_prompt(app, frame, prompt_rect, theme);
    }

    if let Some(status_rect) = layout.status_bar_area {
        render_status_bar(app, frame, status_rect, theme);
    }

    render_active_overlay(app, frame, theme);

    render_notifications(&app.notifications, frame);

    if !layout.warnings.is_empty() {
        PostRenderEffect::LayoutWarnings(layout.warnings)
    } else {
        post_effect
    }
}

fn render_active_overlay(app: &App, frame: &mut Frame, theme: &ThemeConfig) {
    match app.overlay() {
        Some(Overlay::Settings) => render_settings_popup(app, frame, theme),
        Some(Overlay::Help(dialog)) => render_help_popup(app, dialog, frame, theme),
        Some(Overlay::TerminalSize(dialog)) => render_resize_popup(app, dialog, frame, theme),
        Some(Overlay::SerialSettings(dialog)) => render_reconfigure_popup(dialog, frame),
        Some(Overlay::FilePath(dialog)) => render_filepath_popup(app, dialog, frame, theme),
        Some(Overlay::DeviceDisconnected(port)) => render_disconnected_popup(port, frame),
        Some(Overlay::DeviceDetached(port)) => render_detached_popup(port, frame),
        Some(Overlay::Error(err)) => render_error_popup(err, frame),
        None => {}
    }
}
