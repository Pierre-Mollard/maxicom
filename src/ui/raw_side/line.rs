//! Line mode's raw side panel

use ratatui::{
    Frame,
    layout::Rect,
    text::Line,
    widgets::{Block, Paragraph},
};

use crate::{
    app::{
        App,
        data_processing::{entry_display_text, entry_display_to_wire_map, map_disp_byte_to_raw},
        search::SearchMatch,
        selection::CopySide,
        types::{RawRow, RawRows},
    },
    configs::{RawSideView, ThemeConfig, settings::raw_view_to_display_mode},
    raw_side::byte_classes,
    ui::{
        layout::PreparedLayout,
        raw_side::{CharHighlight, format_raw_bytes_with_highlights, raw_panel_layout},
    },
};

/// Line mode's raw side panel: each history line shown next to its bytes, scrolling with the
/// history
pub fn render(
    app: &mut App,
    frame: &mut Frame,
    raw_area: Rect,
    paragraph_y_offset: usize,
    start_idx: usize,
    layout: &PreparedLayout,
    theme: &ThemeConfig,
) {
    let raw_view = app.settings().raw_side.view;
    if raw_view == RawSideView::Off || raw_area.width == 0 || raw_area.height == 0 {
        return;
    }

    let (block, inner_area) =
        raw_panel_layout(raw_area, app.settings().raw_side.orientation, theme);
    let view_height = inner_area.height as usize;
    let visible_visual_start = paragraph_y_offset;
    let visible_visual_end = visible_visual_start + view_height;

    let raw_display_mode = raw_view_to_display_mode(raw_view);
    let copy_range = app.line_mode.copy_state().normalized_selection();
    let partial = app.line_mode.partial_entry(app.settings());
    let history = app.line_mode.history();
    let is_search_active = app.line_mode.search().shows_matches();
    let active_match_idx = app.line_mode.search().current_match;

    let normal_style = *theme.search.search_match;
    let active_style = *theme.search.search_match_active;

    let mut visible_raw_lines = Vec::with_capacity(view_height);
    // copy cursor on the raw side: its byte's panel row and column, before the pan (MAX-71)
    let copy_state = app.line_mode.copy_state();
    let raw_side = copy_state.side == CopySide::Raw;
    let raw_cursor = (copy_state.active && raw_side)
        .then_some((copy_state.cursor.line_idx, copy_state.cursor.byte_idx));
    let mut raw_cursor_cell = None;
    // copy cursor on the text side: its ghost in the panel
    let raw_ghost = (copy_state.active && !raw_side)
        .then_some((copy_state.cursor.line_idx, copy_state.cursor.char_idx));
    let mut current_visual_row = 0;
    // the bytes of each panel row, for mouse clicks (MAX-113)
    let map_rows = app.settings().ui.mouse;
    let mut raw_rows = Vec::new();

    for (local_layout_idx, entry_layout) in layout.line_layouts.iter().enumerate() {
        let global_idx = start_idx + local_layout_idx;

        let visual_sub_count = entry_layout.sub_lines.len();
        let entry_visual_end = current_visual_row + entry_layout.gap + visual_sub_count;

        if entry_visual_end <= visible_visual_start {
            current_visual_row = entry_visual_end;
            continue;
        }
        if current_visual_row >= visible_visual_end {
            break;
        }

        // the gap rows before the entry are blank in the panel too
        for gap_row in current_visual_row..current_visual_row + entry_layout.gap {
            if (visible_visual_start..visible_visual_end).contains(&gap_row) {
                visible_raw_lines.push(Line::default());
                if map_rows {
                    raw_rows.push(None);
                }
            }
        }
        let first_text_row = current_visual_row + entry_layout.gap;

        // the partial RX line is the layout after the last entry (MAX-250)
        let partial = partial.as_ref().filter(|_| global_idx == history.len());
        if let Some(entry) = history.get(global_idx).or(partial) {
            let raw_bytes = entry.wire_bytes();
            let classes = byte_classes(raw_bytes, 0..raw_bytes.len(), app.settings().encoding);
            let display_text = entry_display_text(entry, app.settings().line_mode.display_mode);

            // Display text character -> byte mapping:
            let mut disp_char_to_byte = Vec::with_capacity(display_text.chars().count() + 1);
            for (b_idx, _) in display_text.char_indices() {
                disp_char_to_byte.push(b_idx);
            }
            disp_char_to_byte.push(display_text.len());

            // Map from display byte -> raw wire byte:
            let disp_map = entry_display_to_wire_map(entry, app.settings().line_mode.display_mode);

            let mut char_offset = 0;

            let line_matches: Vec<(usize, &SearchMatch)> = if is_search_active {
                let search = app.line_mode.search();
                let start = search.matches.partition_point(|m| m.line_idx < global_idx);
                let end = search.matches.partition_point(|m| m.line_idx <= global_idx);
                search.matches[start..end]
                    .iter()
                    .enumerate()
                    .map(|(offset, m)| (start + offset, m))
                    .collect()
            } else {
                Vec::new()
            };

            for (sub_idx, sub) in entry_layout.sub_lines.iter().enumerate() {
                let line_y = first_text_row + sub_idx;
                let is_last_sub = sub_idx == visual_sub_count.saturating_sub(1);

                let sub_char_end = (char_offset + sub.content_char_len)
                    .min(disp_char_to_byte.len().saturating_sub(1));

                if line_y >= visible_visual_start && line_y < visible_visual_end {
                    let disp_byte_start = disp_char_to_byte.get(char_offset).copied().unwrap_or(0);
                    let disp_byte_end = disp_char_to_byte
                        .get(sub_char_end)
                        .copied()
                        .unwrap_or(display_text.len());

                    let raw_byte_start = if char_offset == 0 {
                        0
                    } else {
                        map_disp_byte_to_raw(disp_byte_start, &disp_map, raw_bytes.len())
                    };

                    let raw_byte_end = if is_last_sub {
                        raw_bytes.len()
                    } else {
                        map_disp_byte_to_raw(disp_byte_end, &disp_map, raw_bytes.len())
                    };

                    let sub_range =
                        raw_byte_start.min(raw_bytes.len())..raw_byte_end.min(raw_bytes.len());
                    let sub_raw_bytes = &raw_bytes[sub_range.clone()];

                    let mut highlights = Vec::new();

                    // 1. Search Highlights (m.byte_offset is already in raw byte space!)
                    for &(global_m_idx, m) in &line_matches {
                        let m_start = m.byte_offset;
                        let m_end = m.byte_offset + m.byte_len;
                        if m_end > raw_byte_start && m_start < raw_byte_end {
                            highlights.push(CharHighlight {
                                start_col: m_start.saturating_sub(raw_byte_start),
                                end_col: m_end.saturating_sub(raw_byte_start),
                                style: if global_m_idx == active_match_idx {
                                    active_style
                                } else {
                                    normal_style
                                },
                            });
                        }
                    }

                    // 2. Selection Highlights:
                    if let Some((sel_start, sel_end)) = copy_range
                        && global_idx >= sel_start.line_idx
                        && global_idx <= sel_end.line_idx
                    {
                        let is_first_line = global_idx == sel_start.line_idx;
                        let is_last_line = global_idx == sel_end.line_idx;

                        let raw_from_byte = if raw_side {
                            // a raw side selection is a byte range (MAX-71)
                            if is_first_line { sel_start.byte_idx } else { 0 }
                        } else if !is_first_line || sel_start.char_idx == 0 {
                            0 // Include ALL leading escape codes!
                        } else {
                            let disp_from_b = disp_char_to_byte
                                .get(sel_start.char_idx)
                                .copied()
                                .unwrap_or(0);
                            map_disp_byte_to_raw(disp_from_b, &disp_map, raw_bytes.len())
                        };

                        let raw_to_byte = if !is_last_line {
                            raw_bytes.len() // Include ALL trailing bytes, \r, \n!
                        } else if raw_side {
                            sel_end.byte_idx + 1
                        } else {
                            let to_char = sel_end.char_idx + 1;
                            if to_char >= display_text.chars().count() {
                                raw_bytes.len()
                            } else {
                                let disp_to_b = disp_char_to_byte
                                    .get(to_char)
                                    .copied()
                                    .unwrap_or(display_text.len());
                                map_disp_byte_to_raw(disp_to_b, &disp_map, raw_bytes.len())
                            }
                        };

                        if raw_to_byte > raw_byte_start && raw_from_byte < raw_byte_end {
                            highlights.push(CharHighlight {
                                start_col: raw_from_byte.saturating_sub(raw_byte_start),
                                end_col: raw_to_byte.saturating_sub(raw_byte_start),
                                style: theme.ui.selection_style(raw_side),
                            });
                        }
                    }

                    // 3. Ghost of the text cursor: its char's first byte (MAX-71)
                    if let Some((ghost_line, ghost_char)) = raw_ghost
                        && ghost_line == global_idx
                    {
                        let disp_b = disp_char_to_byte
                            .get(ghost_char)
                            .copied()
                            .unwrap_or(display_text.len());
                        let ghost_byte = map_disp_byte_to_raw(disp_b, &disp_map, raw_bytes.len())
                            .min(raw_bytes.len().saturating_sub(1));
                        if ghost_byte >= raw_byte_start
                            && (ghost_byte < raw_byte_end || is_last_sub)
                        {
                            highlights.push(CharHighlight {
                                start_col: ghost_byte - raw_byte_start,
                                end_col: ghost_byte - raw_byte_start + 1,
                                style: *theme.ui.cursor_ghost,
                            });
                        }
                    }

                    if let Some((cursor_line, cursor_byte)) = raw_cursor
                        && cursor_line == global_idx
                        && (cursor_byte < raw_byte_end || is_last_sub)
                        && cursor_byte >= raw_byte_start
                    {
                        let col = (cursor_byte - raw_byte_start) * raw_view.stride();
                        raw_cursor_cell = Some((visible_raw_lines.len(), col));
                    }
                    if map_rows {
                        raw_rows.push(Some(RawRow {
                            line_idx: global_idx,
                            byte_start: raw_byte_start,
                            byte_count: sub_raw_bytes.len(),
                        }));
                    }
                    visible_raw_lines.push(format_raw_bytes_with_highlights(
                        sub_raw_bytes,
                        &classes[sub_range],
                        raw_display_mode,
                        &highlights,
                        theme,
                    ));
                }

                char_offset = sub_char_end;
            }
        }

        current_visual_row = entry_visual_end;
    }

    let max_visible_raw_width = visible_raw_lines
        .iter()
        .map(|l| l.width())
        .max()
        .unwrap_or(0);

    let usable_width = inner_area.width as usize;
    let line_bytes = app.line_mode.line_bytes_mut();
    line_bytes.set_pan_limit(max_visible_raw_width.saturating_sub(usable_width), raw_view);
    let raw_pan_x = line_bytes.pan_x();

    if map_rows {
        let map = app.frame_map_mut();
        map.raw_area = inner_area;
        map.raw_rows = RawRows::Lines(raw_rows);
        map.raw_pan_x = raw_pan_x;
        map.raw_stride = raw_view.stride();
    }

    if let Some((row, col)) = raw_cursor_cell
        && col >= raw_pan_x
        && col - raw_pan_x < usable_width
    {
        let x = inner_area.x + (col - raw_pan_x) as u16;
        frame.set_cursor_position((x, inner_area.y + row as u16));
    }

    render_raw_panel_widget(
        frame,
        raw_area,
        inner_area,
        block,
        visible_raw_lines,
        raw_pan_x,
    );
}

fn render_raw_panel_widget(
    frame: &mut Frame,
    raw_area: Rect,
    inner_area: Rect,
    block: Block<'static>,
    lines: Vec<Line<'static>>,
    raw_pan_x: usize,
) {
    frame.render_widget(block, raw_area);

    let raw_paragraph = Paragraph::new(lines).scroll((0, raw_pan_x.min(u16::MAX as usize) as u16));

    frame.render_widget(raw_paragraph, inner_area);
}
