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
    layout::Rect,
    text::Line,
    widgets::{Block, Borders},
};

use crate::{
    app::{
        App,
        types::{ClickAction, ClickTarget, FrameAreas, SideHandle, ViewportSize},
    },
    configs::{KeyBindingAction, KeywordHighlight, Mode, ThemeConfig},
    overlays::{Overlay, PopupTarget},
    ui::{
        common::{
            border_style_for, monitor_bottom_targets, monitor_bottom_title, monitor_title,
            monitor_title_targets,
        },
        layout::{LayoutWarning, ScreenLayout},
        lines::{render_input_prompt, render_monitor},
        popups::{
            alert::{
                render_confirm_quit_popup, render_delete_link_popup, render_detached_popup,
                render_disconnected_popup, render_error_popup, render_no_device_popup,
                render_restore_popup,
            },
            command_line::render_command_line,
            directory::{render_directory_popup, render_link_form_popup},
            filepath::{render_filepath_popup, render_screenshot_popup},
            help::render_help_popup,
            menu::render_action_menu,
            notifications::render_notifications,
            reconfigure::render_reconfigure_popup,
            resize::render_resize_popup,
            sessions::render_sessions_popup,
            settings::render_settings_popup,
            triggers::render_triggers_popup,
        },
        status_bar::{CompiledStatusBar, render_status_bar},
        terminal::{render_timestamp_gutter, render_vt100_terminal},
    },
};

/// The buttons in the history border's top-right, with what a click runs: for the mouse,
/// with the status bar hidden too (MAX-128). Plain glyphs every font has
const BORDER_BUTTONS: [(&str, KeyBindingAction); 3] = [
    ("[?]", KeyBindingAction::Help),
    ("[≡]", KeyBindingAction::Settings),
    ("[×]", KeyBindingAction::Quit),
];

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
    status_bar: &CompiledStatusBar,
) -> PostRenderEffect {
    let root_area = frame.area();
    let root_backdrop = Block::default().style(*theme.ui.background);
    frame.render_widget(root_backdrop, root_area);

    app.set_screen_size(root_area.width, root_area.height);
    let layout = ScreenLayout::compute(root_area, app);
    app.set_frame_areas(FrameAreas {
        content: layout.content_area,
        raw_side: layout.raw_side_area,
    });
    app.frame_map_mut().clear();
    if let (Some(panel), Some(split)) = (layout.raw_side_area, layout.raw_split_area) {
        let orientation = app.settings().raw_side.orientation;
        app.frame_map_mut().side_handle = Some(SideHandle::new(panel, split, orientation));
    }
    app.update_viewport(ViewportSize::new(
        layout.content_area.width,
        layout.content_area.height,
    ));

    if app.show_borders() {
        let border_style = border_style_for(
            layout.focus_target,
            app.focus_target(),
            app.is_app_focused(),
            theme,
        );

        // the title gets the top border between its corners, but for the buttons and a dash
        // before them (MAX-137)
        let outer = layout.monitor_outer;
        let buttons: String = BORDER_BUTTONS.iter().map(|(text, _)| *text).collect();
        let buttons_width = buttons.chars().count() as u16;
        let room = outer.width.saturating_sub(3 + buttons_width);
        let title = monitor_title(app, &outer, room);
        let targets = monitor_title_targets(&title, outer, *theme.ui.hover);
        app.frame_map_mut().click_targets.extend(targets);
        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .style(*theme.ui.background)
            .title(title);

        // the pan shows once: in the status bar when it is drawn with a pan variable, else here;
        // and in terminal mode the bar can take this bottom border (MAX-135, MAX-139)
        let bar_in_border = layout
            .status_bar_area
            .is_some_and(|bar| bar.y + 1 == outer.bottom());
        let pan_in_bar = layout.status_bar_area.is_some() && status_bar.shows_pan();
        if !bar_in_border && !pan_in_bar {
            for bottom_title in monitor_bottom_title(app, theme) {
                block = block.title_bottom(bottom_title);
            }
            let targets = monitor_bottom_targets(app, outer, *theme.ui.hover);
            app.frame_map_mut().click_targets.extend(targets);
        }

        // right-aligned in the top border, inside its corner
        let mut x = outer.right().saturating_sub(1 + buttons_width);
        block = block.title(Line::styled(buttons, border_style).right_aligned());
        for (text, action) in BORDER_BUTTONS {
            let width = text.chars().count() as u16;
            let cell = Rect::new(x, outer.y, width, 1).intersection(outer);
            if !cell.is_empty() {
                app.frame_map_mut().click_targets.push(ClickTarget {
                    area: cell,
                    action: ClickAction::Run(action),
                    hover: Some(*theme.ui.hover),
                });
            }
            x += width;
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
        let targets = render_status_bar(app, frame, status_rect, theme, status_bar);
        app.frame_map_mut().click_targets.extend(targets);
    }

    render_active_overlay(app, frame, theme, layout.status_bar_area);

    // the part under the pointer: of the popup when one is open, the menu showing it as its
    // selection; else a border button or search bar hint (MAX-131)
    if let Some((x, y)) = app.pointer() {
        let map = app.frame_map();
        let hovered = if map.popup.is_some() {
            map.popup_target_at(x, y)
                .filter(|(_, target)| !matches!(target, PopupTarget::MenuItem(_)))
                .map(|(area, _)| (area, *theme.ui.hover))
        } else {
            map.click_target_at(x, y)
                .and_then(|target| Some((target.area, target.hover?)))
                .or_else(|| Some((map.scrollbar?.part_at(x, y)?, *theme.ui.hover)))
                .or_else(|| Some((map.side_handle?.at(x, y)?, *theme.ui.hover)))
        };
        if let Some((area, style)) = hovered {
            frame.buffer_mut().set_style(area, style);
        }
    }

    render_notifications(&app.notifications, frame, theme);

    if !layout.warnings.is_empty() {
        PostRenderEffect::LayoutWarnings(layout.warnings)
    } else {
        post_effect
    }
}

/// The open popup, `status_bar` the status bar's area when it is drawn: the command line can
/// take its place (MAX-12)
fn render_active_overlay(
    app: &mut App,
    frame: &mut Frame,
    theme: &ThemeConfig,
    status_bar: Option<Rect>,
) {
    let mut targets = std::mem::take(&mut app.frame_map_mut().popup_targets);
    let resolver = &app.key_resolver;
    let mut command_line_prompt = None;
    let area = match app.overlay() {
        Some(Overlay::Settings(dialog)) => {
            render_settings_popup(app, dialog, frame, theme, &mut targets)
        }
        Some(Overlay::Help(dialog)) => render_help_popup(app, dialog, frame, theme, &mut targets),
        Some(Overlay::TerminalSize(dialog)) => {
            render_resize_popup(dialog, resolver, frame, theme, &mut targets)
        }
        Some(Overlay::LinkSettings(dialog)) => {
            render_reconfigure_popup(dialog, resolver, frame, theme, &mut targets)
        }
        Some(Overlay::FilePath(dialog)) => {
            render_filepath_popup(dialog, resolver, frame, theme, &mut targets)
        }
        Some(Overlay::Screenshot(dialog)) => {
            render_screenshot_popup(dialog, resolver, frame, theme, &mut targets)
        }
        Some(Overlay::DeviceDisconnected) => {
            let (kind, label) = (app.link_kind(), app.link_label());
            render_disconnected_popup(kind, label, resolver, frame, theme, &mut targets)
        }
        Some(Overlay::DeviceDetached) => {
            let (kind, label) = (app.link_kind(), app.link_label());
            render_detached_popup(kind, label, resolver, frame, theme, &mut targets)
        }
        Some(Overlay::NoDevice) => render_no_device_popup(resolver, frame, theme, &mut targets),
        Some(Overlay::Error(err)) => render_error_popup(err, resolver, frame, theme, &mut targets),
        Some(Overlay::ConfirmQuit) => {
            render_confirm_quit_popup(resolver, frame, theme, &mut targets)
        }
        Some(Overlay::RestoreSession(session)) => {
            render_restore_popup(session, resolver, frame, theme, &mut targets)
        }
        Some(Overlay::Sessions(browser)) => {
            render_sessions_popup(browser, resolver, frame, theme, &mut targets)
        }
        Some(Overlay::Triggers(browser)) => {
            render_triggers_popup(app, browser, frame, theme, &mut targets)
        }
        Some(Overlay::Directory(directory)) => {
            render_directory_popup(app, directory, frame, theme, &mut targets)
        }
        Some(Overlay::LinkForm(form)) => {
            render_link_form_popup(form, resolver, frame, theme, &mut targets)
        }
        Some(Overlay::DeleteLink(link)) => {
            render_delete_link_popup(link, resolver, frame, theme, &mut targets)
        }
        Some(Overlay::ActionMenu(menu)) => {
            render_action_menu(menu, resolver, frame, theme, &mut targets)
        }
        Some(Overlay::CommandLine(command_line)) => {
            let (area, prompt) =
                render_command_line(app, command_line, status_bar, frame, theme, &mut targets);
            command_line_prompt = Some(prompt);
            area
        }
        None => return,
    };
    let map = app.frame_map_mut();
    map.popup = Some(area);
    map.popup_targets = targets;
    map.command_line = command_line_prompt;
}

#[cfg(test)]
#[path = "tests/ui.rs"]
mod tests;
