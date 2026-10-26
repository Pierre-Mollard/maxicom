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
    configs::Mode,
    inputs::{
        Action, AppAction, CopyModeAction, DialogAction, HistoryAction, LineInputAction,
        SideViewAction, TerminalAction,
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
        MouseEventKind::Moved => return hover(event, app),
        _ => return Action::Ignore,
    };
    // Shift turns the vertical wheel sideways
    let wheel = match wheel {
        Wheel::Up if event.modifiers.contains(KeyModifiers::SHIFT) => Wheel::Left,
        Wheel::Down if event.modifiers.contains(KeyModifiers::SHIFT) => Wheel::Right,
        wheel => wheel,
    };

    match app.input_context() {
        InputContext::Overlay => match wheel {
            Wheel::Up => Action::Overlay(DialogAction::Up),
            Wheel::Down => Action::Overlay(DialogAction::Down),
            Wheel::Right => Action::Overlay(DialogAction::CycleNext),
            Wheel::Left => Action::Overlay(DialogAction::CyclePrev),
        },
        // the view follows the copy cursor: the wheel moves it
        InputContext::CopyMode => Action::CopyMode(match wheel {
            Wheel::Up => CopyModeAction::MoveUp,
            Wheel::Down => CopyModeAction::MoveDown,
            Wheel::Left => CopyModeAction::MoveLeft,
            Wheel::Right => CopyModeAction::MoveRight,
        }),
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
/// there; with another popup open it does nothing
fn open_menu(event: MouseEvent, app: &App) -> Action {
    if app
        .overlay()
        .is_some_and(|overlay| !matches!(overlay, Overlay::ActionMenu(_)))
    {
        return Action::Ignore;
    }
    let menu = ActionMenu::new(
        (event.column, event.row),
        app.settings().mode,
        app.is_logging_active(),
    );
    Action::App(AppAction::OpenOverlay(Overlay::ActionMenu(menu)))
}

/// The left button. In a popup a press picks the part under it (a key hint runs its action),
/// or closes the popup from outside it. On the status bar it runs the block under it. In the TX input it moves the input cursor. In the history and the raw panel a
/// press puts the copy cursor under it, a drag selects from there, a release after a drag
/// yanks (`CopyModeState::apply_action`)
fn handle_left_button(event: MouseEvent, app: &App) -> Action {
    let map = app.frame_map();
    let (x, y) = (event.column, event.row);
    let is_press = matches!(event.kind, MouseEventKind::Down(_));

    match app.input_context() {
        InputContext::Overlay if is_press => {
            let outside = map
                .popup
                .is_some_and(|area| !area.contains(Position::new(x, y)));
            return if let Some(PopupTarget::Hint(action)) = map.popup_target(x, y) {
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
        _ if is_press && let Some(click) = map.click_target(x, y) => {
            return match click {
                ClickAction::Run(action) => action.to_action(app),
                ClickAction::ToggleSide => Action::CopyMode(CopyModeAction::ToggleSide),
                ClickAction::Search(action) => action.to_action(),
            };
        }
        InputContext::Overlay | InputContext::Prefix => return Action::Ignore,
        _ => {}
    }

    match event.kind {
        MouseEventKind::Down(_) => {
            if let Some(column) = map.prompt_column(x, y) {
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
            let pos = app
                .copy_side()
                .and_then(|side| map.copy_position(side, x, y, true));
            pos.map_or(Action::Ignore, |pos| {
                Action::CopyMode(CopyModeAction::MouseDrag(pos, (x, y)))
            })
        }
        _ if app.copy_side().is_some() => Action::CopyMode(CopyModeAction::MouseRelease),
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
mod tests {
    //! Mouse reports for a remote program (MAX-113).

    use super::*;

    fn event(kind: MouseEventKind, modifiers: KeyModifiers) -> MouseEvent {
        MouseEvent {
            kind,
            column: 0,
            row: 0,
            modifiers,
        }
    }

    fn report(
        mode: MouseProtocolMode,
        encoding: MouseProtocolEncoding,
        kind: MouseEventKind,
        modifiers: KeyModifiers,
        cell: (usize, usize),
    ) -> Option<Vec<u8>> {
        mouse_report(mode, encoding, &event(kind, modifiers), cell)
    }

    /// SGR: button, 1-based cell, `M` for a press and `m` for a release; the wheel is 64 and
    /// up, a drag adds 32, modifiers add 4 / 8 / 16.
    #[test]
    fn sgr_reports() {
        use MouseProtocolEncoding::Sgr;
        use MouseProtocolMode::ButtonMotion;
        let none = KeyModifiers::NONE;
        let left = MouseButton::Left;
        let sgr = |kind, modifiers| report(ButtonMotion, Sgr, kind, modifiers, (4, 2));
        assert_eq!(
            sgr(MouseEventKind::Down(left), none).unwrap(),
            b"\x1b[<0;5;3M"
        );
        assert_eq!(
            sgr(MouseEventKind::Up(left), none).unwrap(),
            b"\x1b[<0;5;3m"
        );
        assert_eq!(
            sgr(MouseEventKind::Down(MouseButton::Right), none).unwrap(),
            b"\x1b[<2;5;3M"
        );
        assert_eq!(
            sgr(MouseEventKind::ScrollDown, none).unwrap(),
            b"\x1b[<65;5;3M"
        );
        assert_eq!(
            sgr(MouseEventKind::Drag(left), KeyModifiers::CONTROL).unwrap(),
            b"\x1b[<48;5;3M"
        );
        assert_eq!(
            sgr(
                MouseEventKind::Down(left),
                KeyModifiers::SHIFT | KeyModifiers::ALT
            )
            .unwrap(),
            b"\x1b[<12;5;3M"
        );
    }

    /// The default encoding: a release is button 3, values are bytes plus 32, a cell past 223
    /// can't be sent; the UTF-8 one sends it as a char.
    #[test]
    fn default_and_utf8_reports() {
        use MouseProtocolEncoding::{Default, Utf8};
        use MouseProtocolMode::PressRelease;
        let none = KeyModifiers::NONE;
        let left = MouseButton::Left;
        let press = MouseEventKind::Down(left);
        assert_eq!(
            report(PressRelease, Default, press, none, (4, 2)).unwrap(),
            b"\x1b[M\x20\x25\x23"
        );
        assert_eq!(
            report(
                PressRelease,
                Default,
                MouseEventKind::Up(left),
                none,
                (4, 2)
            )
            .unwrap(),
            b"\x1b[M\x23\x25\x23"
        );
        assert_eq!(report(PressRelease, Default, press, none, (300, 2)), None);
        assert_eq!(
            report(PressRelease, Utf8, press, none, (300, 2)).unwrap(),
            "\x1b[M\x20\u{14D}\x23".as_bytes()
        );
    }

    /// Each mode reports its kinds of events only: presses (X10), and releases (1000), and
    /// drags (1002), and every move (1003).
    #[test]
    fn modes_filter_events() {
        use MouseProtocolEncoding::Sgr;
        use MouseProtocolMode::{AnyMotion, ButtonMotion, Press, PressRelease};
        let none = KeyModifiers::NONE;
        let left = MouseButton::Left;
        let is_reported = |mode, kind| report(mode, Sgr, kind, none, (0, 0)).is_some();
        assert!(is_reported(Press, MouseEventKind::Down(left)));
        assert!(!is_reported(Press, MouseEventKind::Up(left)));
        assert!(is_reported(PressRelease, MouseEventKind::Up(left)));
        assert!(!is_reported(PressRelease, MouseEventKind::Drag(left)));
        assert!(is_reported(ButtonMotion, MouseEventKind::Drag(left)));
        assert!(!is_reported(ButtonMotion, MouseEventKind::Moved));
        assert!(is_reported(AnyMotion, MouseEventKind::Moved));
        assert_eq!(
            report(AnyMotion, Sgr, MouseEventKind::Moved, none, (0, 0)).unwrap(),
            b"\x1b[<35;1;1M"
        );
    }
}
