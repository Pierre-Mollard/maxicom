//! Mouse events to actions (MAX-113): pure like the key path, from the input context and where
//! the last frame drew the history and the raw panel (`App::frame_areas`)

use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Position;
use vt100::{MouseProtocolEncoding, MouseProtocolMode};

use crate::{
    app::{
        App, InputContext,
        selection::{CopySide, MouseUnit},
        types::ClickAction,
    },
    configs::{KeyBindingAction, Mode},
    inputs::{
        Action, AppAction, ClickHoldAction, CopyModeAction, DialogAction, HistoryAction,
        LineInputAction, ScrollbarAction, SearchModeAction, SideHandleAction, SideViewAction,
        TerminalAction,
    },
    overlays::{ActionMenu, Overlay, PopupTarget},
};

/// Rows a wheel notch scrolls
const WHEEL_ROWS: usize = 3;
/// Columns a sideways wheel notch pans the history
const WHEEL_COLS: usize = 4;
/// Bytes a sideways wheel notch pans the raw panel, as its keys do
const WHEEL_BYTES: usize = 2;

#[derive(Clone, Copy)]
enum Wheel {
    Up,
    Down,
    Left,
    Right,
}

pub fn handle_mouse(event: MouseEvent, app: &App) -> Action {
    // a toast is drawn over everything: a press on it dismisses it, whatever lies under
    // (MAX-197)
    if event.kind == MouseEventKind::Down(MouseButton::Left)
        && let Some(id) = app.frame_map().toast_at(event.column, event.row)
    {
        return Action::App(AppAction::DismissToast(Some(id)));
    }
    if let Some(action) = forward_to_remote(&event, app) {
        return action;
    }
    let wheel = match event.kind {
        MouseEventKind::ScrollUp => Wheel::Up,
        MouseEventKind::ScrollDown => Wheel::Down,
        MouseEventKind::ScrollLeft => Wheel::Left,
        MouseEventKind::ScrollRight => Wheel::Right,
        MouseEventKind::Down(MouseButton::Left)
        | MouseEventKind::Drag(MouseButton::Left)
        | MouseEventKind::Up(MouseButton::Left) => return handle_left_button(event, app),
        MouseEventKind::Down(MouseButton::Right) => return open_menu(event, app),
        MouseEventKind::Down(MouseButton::Middle) => return middle_paste(app),
        MouseEventKind::Moved => return lost_release(app).unwrap_or_else(|| hover(event, app)),
        _ => return Action::Ignore,
    };
    // Shift turns the vertical wheel sideways, and so does a pan indicator under it: a status
    // bar block that pans, the border's pan arrows (MAX-143)
    let over_pan = app.input_context() != InputContext::Overlay
        && matches!(
            app.frame_map().click_target(event.column, event.row),
            Some(ClickAction::Run(
                KeyBindingAction::PanLeft | KeyBindingAction::PanRight | KeyBindingAction::PanHome
            ))
        );
    let sideways = over_pan || event.modifiers.contains(KeyModifiers::SHIFT);
    let wheel = match wheel {
        Wheel::Up if sideways => Wheel::Left,
        Wheel::Down if sideways => Wheel::Right,
        wheel => wheel,
    };

    match app.input_context() {
        InputContext::Overlay => match wheel {
            Wheel::Up => Action::Overlay(DialogAction::Up),
            Wheel::Down => Action::Overlay(DialogAction::Down),
            Wheel::Right => Action::Overlay(DialogAction::Right),
            Wheel::Left => Action::Overlay(DialogAction::Left),
        },
        _ => {
            let at = Position::new(event.column, event.row);
            let on_raw_side = app
                .frame_areas()
                .raw_side
                .is_some_and(|area| area.contains(at));
            match wheel {
                Wheel::Up => Action::History(HistoryAction::ScrollUp(WHEEL_ROWS)),
                Wheel::Down => Action::History(HistoryAction::ScrollDown(WHEEL_ROWS)),
                Wheel::Left if on_raw_side => {
                    Action::App(AppAction::SideView(SideViewAction::PanLeft(WHEEL_BYTES)))
                }
                Wheel::Right if on_raw_side => {
                    Action::App(AppAction::SideView(SideViewAction::PanRight(WHEEL_BYTES)))
                }
                Wheel::Left => Action::History(HistoryAction::PanLeft(WHEEL_COLS)),
                Wheel::Right => Action::History(HistoryAction::PanRight(WHEEL_COLS)),
            }
        }
    }
}

/// A move with no button while the left one is still held for maxicom: the release was lost,
/// let go outside the window (MAX-191). It ends the hold as the release would, so a drag
/// selection yanks
fn lost_release(app: &App) -> Option<Action> {
    if app.is_click_held() {
        Some(Action::App(AppAction::ClickHold(ClickHoldAction::Release)))
    } else if app.is_scrollbar_held() {
        Some(Action::App(AppAction::Scrollbar(ScrollbarAction::Release)))
    } else if app.is_side_handle_held() {
        Some(Action::App(AppAction::SideHandle(
            SideHandleAction::Release,
        )))
    } else if app.is_mouse_selecting() {
        Some(Action::CopyMode(CopyModeAction::MouseRelease))
    } else {
        None
    }
}

/// A move onto another clickable part, or off one, is a hover (MAX-131). Within the same part
/// it does nothing, so moves don't redraw
fn hover(event: MouseEvent, app: &App) -> Action {
    let map = app.frame_map();
    let before = app.pointer().and_then(|(x, y)| map.hover_area(x, y));
    if before == map.hover_area(event.column, event.row) {
        return Action::Ignore;
    }
    Action::App(AppAction::Hover(event.column, event.row))
}

/// The right button opens the action menu at the pointer (MAX-130), or moves the open menu
/// there; with another popup open it does nothing. Where typed text goes it pastes the last
/// copy instead, while there is one (MAX-144)
fn open_menu(event: MouseEvent, app: &App) -> Action {
    if app
        .overlay()
        .is_some_and(|overlay| !matches!(overlay, Overlay::ActionMenu(_)))
    {
        return Action::Ignore;
    }
    if app.overlay().is_none() && app.has_paste_buffer() && is_paste_target(&event, app) {
        return Action::App(AppAction::PasteBuffer);
    }
    let at = (event.column, event.row);
    let recording = app.recording().is_some();
    // the marks' rows (MAX-164): a mark on the line clicked, on a scrolled view or in copy
    // mode; the list while there are marks
    let add = (app.is_copy_mode_active() || !app.scroll_status().is_bottom())
        .then(|| (app.frame_map().text_position(at.0, at.1, false)).map(|pos| pos.line_idx));
    let menu = ActionMenu::new(at, app.settings().mode, app.link_kind(), recording)
        .with_marks(add, !app.marks().is_empty());
    Action::App(AppAction::OpenOverlay(Overlay::ActionMenu(menu)))
}

/// The middle button pastes the last copy anywhere outside a popup, as X11 terminals paste
/// their selection; nothing before a copy, or with `middle_click_paste` off (MAX-212)
fn middle_paste(app: &App) -> Action {
    let on = app.settings().ui.middle_click_paste;
    if on && app.overlay().is_none() && app.has_paste_buffer() {
        Action::App(AppAction::PasteBuffer)
    } else {
        Action::Ignore
    }
}

/// Where typed text goes: in line mode the TX input (from the history's copy mode too), in
/// terminal mode the cursor's row on the live view
fn is_paste_target(event: &MouseEvent, app: &App) -> bool {
    let (x, y) = (event.column, event.row);
    let map = app.frame_map();
    match (app.settings().mode, app.input_context()) {
        (Mode::Line, InputContext::LineInput | InputContext::CopyMode) => {
            map.prompt_column(x, y).is_some()
        }
        (Mode::Terminal, InputContext::Terminal) => {
            let screen = app.terminal_mode.vt100_screen();
            let area = map.text_area;
            screen.scrollback() == 0
                && area.contains(Position::new(x, y))
                && (y - area.y) as usize + app.terminal_mode.pan_offset_y()
                    == screen.cursor_position().0 as usize
        }
        _ => false,
    }
}

/// The left button. In a popup a press picks the part under it (a key hint runs its action),
/// or closes the popup from outside it. On the status bar it runs the block under it. In the TX input it moves the input cursor. In the history and the raw panel a
/// press puts the copy cursor under it, a drag selects from there, a release after a drag
/// yanks (`CopyModeState::apply_action`)
fn handle_left_button(event: MouseEvent, app: &App) -> Action {
    let map = app.frame_map();
    let (x, y) = (event.column, event.row);
    let is_press = matches!(event.kind, MouseEventKind::Down(_));

    // a click target held, from its press to the release (MAX-143)
    if app.is_click_held() {
        let hold = |action| Action::App(AppAction::ClickHold(action));
        match event.kind {
            MouseEventKind::Drag(_) => return hold(ClickHoldAction::Drag(x, y)),
            MouseEventKind::Up(_) => return hold(ClickHoldAction::Release),
            _ => {}
        }
    }

    match app.input_context() {
        InputContext::Overlay if is_press => {
            let outside = map
                .popup
                .is_some_and(|area| !area.contains(Position::new(x, y)));
            return if let Some(column) = map.command_line_column(x, y) {
                // the command line's cursor goes under the click (MAX-12)
                Action::Overlay(DialogAction::ClickAt(column))
            } else if let Some(PopupTarget::Hint(action)) = map.popup_target(x, y) {
                // a key hint runs as its key would (MAX-125)
                action.to_action()
            } else if let Some(target) = map.popup_target(x, y) {
                Action::Overlay(DialogAction::Pick(target))
            } else if outside && app.overlay().is_some_and(|o| o.closes_on_click_outside()) {
                Action::Overlay(DialogAction::Close)
            } else {
                Action::Ignore
            };
        }
        // a status bar block, whatever the mode (MAX-125)
        _ if is_press && let Some(target) = map.click_target_at(x, y) => {
            return match target.action {
                // a step repeats while held (MAX-143)
                ClickAction::Run(action) if action.repeats_when_held() => Action::App(
                    AppAction::ClickHold(ClickHoldAction::Press(action, target.area)),
                ),
                ClickAction::Run(action) => action.to_action(app),
                ClickAction::ToggleSide => Action::CopyMode(CopyModeAction::ToggleSide),
                ClickAction::Search(action) => action.to_action(),
            };
        }
        InputContext::Overlay | InputContext::Prefix => return Action::Ignore,
        _ => {}
    }

    // the history's scrollbar, from a press on it to the release (MAX-142); in copy mode too,
    // the cursor stays (MAX-143)
    let scrollbar = |action| Action::App(AppAction::Scrollbar(action));
    if app.is_scrollbar_held() {
        match event.kind {
            MouseEventKind::Drag(_) => {
                return map.scrollbar.map_or(Action::Ignore, |bar| {
                    scrollbar(ScrollbarAction::Drag(i32::from(y) - i32::from(bar.area.y)))
                });
            }
            MouseEventKind::Up(_) => return scrollbar(ScrollbarAction::Release),
            _ => {}
        }
    }
    if is_press
        && let Some(bar) = map.scrollbar
        && bar.area.contains(Position::new(x, y))
    {
        return scrollbar(ScrollbarAction::Press(y - bar.area.y));
    }

    // the raw side panel's border, from a press on it to the release (MAX-189)
    let side_handle = |action| Action::App(AppAction::SideHandle(action));
    if app.is_side_handle_held() {
        match event.kind {
            MouseEventKind::Drag(_) => return side_handle(SideHandleAction::Drag(x, y)),
            MouseEventKind::Up(_) => return side_handle(SideHandleAction::Release),
            _ => {}
        }
    }
    if is_press
        && map
            .side_handle
            .is_some_and(|handle| handle.at(x, y).is_some())
    {
        return side_handle(SideHandleAction::Press);
    }

    match event.kind {
        MouseEventKind::Down(_) => {
            if let Some(column) = map.search_column(x, y) {
                Action::Search(SearchModeAction::ClickAt(column))
            } else if let Some(column) = map.prompt_column(x, y) {
                Action::LineInput(LineInputAction::ClickAt(column))
            } else if let Some(pos) = map.raw_position(x, y, false) {
                Action::CopyMode(CopyModeAction::MousePress(
                    pos,
                    CopySide::Raw,
                    MouseUnit::Char,
                ))
            } else if let Some(pos) = map.text_position(x, y, false) {
                Action::CopyMode(CopyModeAction::MousePress(
                    pos,
                    CopySide::Text,
                    MouseUnit::Char,
                ))
            } else {
                Action::Ignore
            }
        }
        // on the side the press chose, brought back into it when the pointer leaves it
        MouseEventKind::Drag(_) => {
            let pos = (app.copy_side().or(app.pending_press_side()))
                .and_then(|side| map.copy_position(side, x, y, true));
            pos.map_or(Action::Ignore, |pos| {
                Action::CopyMode(CopyModeAction::MouseDrag(pos, (x, y)))
            })
        }
        _ if app.copy_side().or(app.pending_press_side()).is_some() => {
            Action::CopyMode(CopyModeAction::MouseRelease)
        }
        _ => Action::Ignore,
    }
}

/// Terminal mode, when the remote program asked for the mouse (DECSET 1000 / 1002 / 1003, in
/// vt100's `mouse_protocol_mode`): an event on the grid goes to it, encoded as it asked. Only
/// on the live view with none of maxicom's own modes on (copy, search, prefix, popup), like
/// tmux; elsewhere the mouse acts on maxicom. `None`: not for the remote, or a kind of event
/// its mode doesn't report (a drag or a move, which do nothing outside copy mode)
fn forward_to_remote(event: &MouseEvent, app: &App) -> Option<Action> {
    if app.settings().mode != Mode::Terminal || app.input_context() != InputContext::Terminal {
        return None;
    }
    let screen = app.terminal_mode.vt100_screen();
    let mode = screen.mouse_protocol_mode();
    if mode == MouseProtocolMode::None || screen.scrollback() > 0 {
        return None;
    }

    let area = app.frame_map().text_area;
    if !area.contains(Position::new(event.column, event.row)) {
        return None;
    }
    let col = (event.column - area.x) as usize + app.terminal_mode.pan_offset_x();
    let row = (event.row - area.y) as usize + app.terminal_mode.pan_offset_y();
    let (grid_rows, grid_cols) = app.terminal_mode.get_vt100_size();
    if col >= grid_cols as usize || row >= grid_rows as usize {
        return None;
    }

    let report = mouse_report(mode, screen.mouse_protocol_encoding(), event, (col, row));
    report.map(|bytes| Action::Terminal(TerminalAction::MouseReport(bytes)))
}

/// The bytes reporting an event on grid cell `(col, row)` (0-based) in the remote's mode and
/// encoding (xterm's mouse tracking), `None` when the mode doesn't report that kind of event
/// or the encoding can't hold the cell
fn mouse_report(
    mode: MouseProtocolMode,
    encoding: MouseProtocolEncoding,
    event: &MouseEvent,
    (col, row): (usize, usize),
) -> Option<Vec<u8>> {
    let button_code = |button| match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    };
    // button code, released, motion
    let (button, is_release, is_motion) = match event.kind {
        MouseEventKind::Down(button) => (button_code(button), false, false),
        MouseEventKind::Up(button) => (button_code(button), true, false),
        MouseEventKind::Drag(button) => (button_code(button), false, true),
        // no button held
        MouseEventKind::Moved => (3, false, true),
        MouseEventKind::ScrollUp => (64, false, false),
        MouseEventKind::ScrollDown => (65, false, false),
        MouseEventKind::ScrollLeft => (66, false, false),
        MouseEventKind::ScrollRight => (67, false, false),
    };
    let is_reported = match mode {
        MouseProtocolMode::None => false,
        MouseProtocolMode::Press => !is_release && !is_motion,
        MouseProtocolMode::PressRelease => !is_motion,
        MouseProtocolMode::ButtonMotion => event.kind != MouseEventKind::Moved,
        MouseProtocolMode::AnyMotion => true,
    };
    if !is_reported {
        return None;
    }

    let mut modifiers = 0;
    if event.modifiers.contains(KeyModifiers::SHIFT) {
        modifiers += 4;
    }
    if event.modifiers.contains(KeyModifiers::ALT) {
        modifiers += 8;
    }
    if event.modifiers.contains(KeyModifiers::CONTROL) {
        modifiers += 16;
    }
    let motion = if is_motion { 32 } else { 0 };
    let (x, y) = (col + 1, row + 1);

    match encoding {
        // the release keeps its button, told by the final `m`
        MouseProtocolEncoding::Sgr => {
            let code = button + modifiers + motion;
            let end = if is_release { 'm' } else { 'M' };
            Some(format!("\x1b[<{code};{x};{y}{end}").into_bytes())
        }
        // a release is button 3; each value is sent plus 32, as a byte or a UTF-8 char
        MouseProtocolEncoding::Default | MouseProtocolEncoding::Utf8 => {
            let code = if is_release { 3 } else { button } + modifiers + motion;
            let mut bytes = b"\x1b[M".to_vec();
            bytes.push(32 + code as u8);
            for value in [x, y] {
                let value = 32 + value;
                match encoding {
                    MouseProtocolEncoding::Default => bytes.push(u8::try_from(value).ok()?),
                    _ => {
                        let ch = char::from_u32(value as u32).filter(|_| value < 2048)?;
                        bytes.extend_from_slice(ch.encode_utf8(&mut [0; 4]).as_bytes());
                    }
                }
            }
            Some(bytes)
        }
    }
}

#[cfg(test)]
#[path = "../tests/inputs/mouse.rs"]
mod tests;
