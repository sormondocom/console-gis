//! Interactive shape editor — build Point, Line, Polygon (and Multi variants)
//! coordinate by coordinate, then export to a GeoJSON FeatureCollection file.

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Widget},
};

use crate::render::canvas::TerminalCapability;
use crate::tui::app::{ShapeEditorState, ShapeEditorStep, ShapeType};

pub struct ShapeEditorView<'a> {
    pub state:      &'a ShapeEditorState,
    pub capability: TerminalCapability,
}

impl<'a> Widget for ShapeEditorView<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let tc      = self.capability.supports_true_colour();
        let accent  = if tc { Color::Rgb(30, 200, 240) } else { Color::Cyan };
        let dim     = if tc { Color::Rgb(50, 80, 100)  } else { Color::DarkGray };
        let ok_col  = if tc { Color::Rgb(80, 220, 100) } else { Color::Green };
        let err_col = if tc { Color::Rgb(220, 80, 60)  } else { Color::Red };
        let hi_bg   = if tc { Color::Rgb(5, 20, 40)    } else { Color::DarkGray };
        let coord_c = if tc { Color::Rgb(200, 200, 100)} else { Color::Yellow };

        // ── Layout: left (type selector) + right (step panel) ───────────────
        let list_w = 24u16.min(area.width.saturating_sub(28));
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(list_w), Constraint::Min(10)])
            .split(area);

        // ── Left: geometry type list ─────────────────────────────────────────
        let left_block = Block::default()
            .title(" Geometry Type ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(
                if self.state.step == ShapeEditorStep::SelectType { accent } else { dim }
            ));
        let li = left_block.inner(chunks[0]);
        left_block.render(chunks[0], buf);

        for (i, st) in ShapeType::ALL.iter().enumerate() {
            let row = li.top() + 1 + i as u16 * 2;
            if row >= li.bottom().saturating_sub(2) { break; }
            let is_sel = i == self.state.type_idx;
            let marker = if is_sel { "▸ " } else { "  " };
            let label = format!("{marker}[{}] {}", st.key(), st.name());
            for (ci, ch) in label.chars().enumerate() {
                let col = li.left() + ci as u16;
                if col >= li.right() { break; }
                let cell = buf.get_mut(col, row);
                cell.set_char(ch)
                    .set_fg(if is_sel { accent } else { Color::White })
                    .set_bg(if is_sel { hi_bg } else { Color::Reset });
                if is_sel {
                    cell.set_style(cell.style().add_modifier(Modifier::BOLD));
                }
            }
        }

        // Step indicator at bottom of left panel
        let step_label = match self.state.step {
            ShapeEditorStep::SelectType    => "Step 1: pick type",
            ShapeEditorStep::AddVertices   => "Step 2: add coords",
            ShapeEditorStep::EnterName     => "Step 3: name",
            ShapeEditorStep::EnterExportPath => "Step 4: export",
        };
        write_row(buf, li.left(), li.bottom().saturating_sub(1), li.right(), step_label, dim);

        // ── Right panel ───────────────────────────────────────────────────────
        let (title, border_color) = match self.state.step {
            ShapeEditorStep::SelectType    => (" Select Type ", dim),
            ShapeEditorStep::AddVertices   => (" Add Coordinates ", accent),
            ShapeEditorStep::EnterName     => (" Feature Name ", accent),
            ShapeEditorStep::EnterExportPath => (" Export to GeoJSON ", accent),
        };
        let right_block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));
        let ri = right_block.inner(chunks[1]);
        right_block.render(chunks[1], buf);

        match self.state.step {
            ShapeEditorStep::SelectType => {
                render_select_type(buf, ri, self.state, dim, accent);
            }
            ShapeEditorStep::AddVertices => {
                render_add_vertices(buf, ri, self.state, dim, accent, coord_c, ok_col, err_col);
            }
            ShapeEditorStep::EnterName => {
                render_enter_name(buf, ri, self.state, dim, accent, ok_col);
            }
            ShapeEditorStep::EnterExportPath => {
                render_export_path(buf, ri, self.state, dim, accent, ok_col, err_col);
            }
        }
    }
}

// ── Step renderers ─────────────────────────────────────────────────────────────

fn render_select_type(
    buf: &mut Buffer, ri: Rect,
    state: &ShapeEditorState,
    dim: Color, accent: Color,
) {
    let st = state.current_type();
    let row = ri.top() + 1;
    write_row(buf, ri.left(), row, ri.right(), st.name(), accent);
    write_row(buf, ri.left(), row + 2, ri.right(), st.hint(), dim);
    write_row(buf, ri.left(), row + 4, ri.right(), "GeoJSON geometry types:", dim);

    let descs = [
        ("Point",           "Single geographic location (lat/lon)"),
        ("MultiPoint",      "Collection of unconnected points"),
        ("LineString",      "Ordered sequence of positions forming a path"),
        ("MultiLineString", "Multiple independent paths"),
        ("Polygon",         "Closed area defined by an outer ring"),
        ("MultiPolygon",    "Multiple independent closed areas"),
    ];
    for (i, (_, desc)) in descs.iter().enumerate() {
        let r = row + 5 + i as u16;
        if r >= ri.bottom() { break; }
        let label = format!("  [{}] {desc}", i + 1);
        write_row(buf, ri.left(), r, ri.right(), &label, dim);
    }
    write_row(buf, ri.left(), ri.bottom().saturating_sub(1), ri.right(),
        "↑↓/1-6 select · Enter=start · q=menu", dim);
}

fn render_add_vertices(
    buf: &mut Buffer, ri: Rect,
    state: &ShapeEditorState,
    dim: Color, accent: Color, coord_c: Color, ok_col: Color, err_col: Color,
) {
    let st = state.current_type();
    let mut row = ri.top() + 1;

    // Hint line
    write_row(buf, ri.left(), row, ri.right(), st.hint(), dim);
    row += 2;

    // Input fields
    let lat_active = state.coord_field == 0;
    let lon_active = state.coord_field == 1;

    write_row(buf, ri.left(), row, ri.right(), "Latitude:", if lat_active { accent } else { dim });
    let lat_val = if lat_active { format!("{}█", state.lat_buf) } else { state.lat_buf.clone() };
    write_row(buf, ri.left() + 12, row, ri.right(), &lat_val,
        if lat_active { Color::White } else { Color::Rgb(160, 160, 160) });
    row += 1;

    write_row(buf, ri.left(), row, ri.right(), "Longitude:", if lon_active { accent } else { dim });
    let lon_val = if lon_active { format!("{}█", state.lon_buf) } else { state.lon_buf.clone() };
    write_row(buf, ri.left() + 12, row, ri.right(), &lon_val,
        if lon_active { Color::White } else { Color::Rgb(160, 160, 160) });
    row += 2;

    // Key hints row
    let keys = if st.is_multi() {
        "Tab=switch  Enter=add point  F=finish part  U=undo  N=next step"
    } else {
        "Tab=switch  Enter=add point  U=undo  N=next step"
    };
    write_row(buf, ri.left(), row, ri.right(), keys, dim);
    row += 1;

    // Separator
    let sep: String = std::iter::repeat('─').take(ri.width as usize).collect();
    write_row(buf, ri.left(), row, ri.right(), &sep, dim);
    row += 1;

    // Vertex list header
    let total = state.total_coords();
    let parts_done = state.parts.len();
    let header = if st.is_multi() && parts_done > 0 {
        format!("Vertices ({total})  ·  {parts_done} part{} finished",
            if parts_done == 1 { "" } else { "s" })
    } else {
        format!("Vertices ({total})")
    };
    write_row(buf, ri.left(), row, ri.right(), &header, accent);
    row += 1;

    // Display coords with part grouping
    let list_rows = ri.bottom().saturating_sub(3).saturating_sub(row);
    let all = state.all_coords();
    let scroll = state.vert_scroll.min(all.len().saturating_sub(1));
    let visible = all.iter().enumerate().skip(scroll).take(list_rows as usize);

    // Global index offset to figure out part boundaries
    let mut part_offsets: Vec<usize> = vec![0];
    for p in &state.parts {
        part_offsets.push(part_offsets.last().unwrap() + p.len());
    }

    for (gi, (lat, lon)) in visible {
        if row >= ri.bottom().saturating_sub(2) { break; }

        // Part label on boundary
        if let Some(pi) = part_offsets.iter().position(|&o| o == gi) {
            if pi > 0 {
                // This starts a new finalized part — already counted
            }
            if pi < state.parts.len() {
                let plabel = format!("  — Part {} —", pi + 1);
                write_row(buf, ri.left(), row, ri.right(), &plabel, dim);
                row += 1;
                if row >= ri.bottom().saturating_sub(2) { break; }
            }
        }
        // If we're past all finalized parts and into current
        if gi == part_offsets.last().copied().unwrap_or(0) && !state.current.is_empty()
            && state.parts.len() > 0
        {
            let plabel = format!("  — Current part —");
            write_row(buf, ri.left(), row, ri.right(), &plabel, dim);
            row += 1;
            if row >= ri.bottom().saturating_sub(2) { break; }
        }

        let dir_lat = if *lat >= 0.0 { 'N' } else { 'S' };
        let dir_lon = if *lon >= 0.0 { 'E' } else { 'W' };
        let line = format!("  [{gi:>3}]  {:>10.6}°{}  {:>11.6}°{}",
            lat.abs(), dir_lat, lon.abs(), dir_lon);
        write_row(buf, ri.left(), row, ri.right(), &line, coord_c);
        row += 1;
    }

    // Scroll indicator
    if all.len() > list_rows as usize {
        let pct = if all.is_empty() { 0 } else { scroll * 100 / all.len() };
        let ind = format!("  ↑↓ scroll ({pct}%)");
        write_row(buf, ri.left(), ri.bottom().saturating_sub(3), ri.right(), &ind, dim);
    }

    // Error / message
    if let Some(msg) = &state.message {
        let col = if msg.starts_with("Error") || msg.starts_with("Need")
                  || msg.starts_with("Invalid") { err_col } else { ok_col };
        write_row(buf, ri.left(), ri.bottom().saturating_sub(2), ri.right(), msg, col);
    }

    write_row(buf, ri.left(), ri.bottom().saturating_sub(1), ri.right(),
        "Esc=back to type select", dim);
}

fn render_enter_name(
    buf: &mut Buffer, ri: Rect,
    state: &ShapeEditorState,
    dim: Color, accent: Color, _ok_col: Color,
) {
    let row = ri.top() + 1;
    let st = state.current_type();

    let summary = format!("Type: {}   Coords: {}", st.name(), state.total_coords());
    write_row(buf, ri.left(), row, ri.right(), &summary, dim);
    write_row(buf, ri.left(), row + 2, ri.right(), "Feature name (optional):", accent);
    let name_disp = format!("{}█", state.name_buf);
    write_row(buf, ri.left(), row + 3, ri.right(), &name_disp, Color::White);
    write_row(buf, ri.left(), row + 5, ri.right(),
        "Enter=next · Tab=skip · Esc=back", dim);
}

fn render_export_path(
    buf: &mut Buffer, ri: Rect,
    state: &ShapeEditorState,
    dim: Color, accent: Color, ok_col: Color, err_col: Color,
) {
    let row = ri.top() + 1;
    let st = state.current_type();

    let name_disp = if state.name_buf.is_empty() { "(unnamed)".to_string() } else { state.name_buf.clone() };
    let summary = format!("{}  \"{}\"  ({} coords)",
        st.name(), name_disp, state.total_coords());
    write_row(buf, ri.left(), row, ri.right(), &summary, dim);

    // GeoJSON preview
    match state.to_geojson() {
        Ok(json) => {
            let preview_lines: Vec<&str> = json.lines().take(12).collect();
            write_row(buf, ri.left(), row + 2, ri.right(), "GeoJSON preview:", accent);
            for (i, line) in preview_lines.iter().enumerate() {
                let r = row + 3 + i as u16;
                if r >= ri.bottom().saturating_sub(7) { break; }
                write_row(buf, ri.left(), r, ri.right(), line,
                    Color::Rgb(160, 200, 160));
            }
        }
        Err(e) => {
            write_row(buf, ri.left(), row + 2, ri.right(), &format!("⚠  {e}"), err_col);
        }
    }

    let input_row = ri.bottom().saturating_sub(6);
    write_row(buf, ri.left(), input_row, ri.right(), "Output path (.geojson):", accent);
    let path_disp = format!("{}█", state.export_buf);
    write_row(buf, ri.left(), input_row + 1, ri.right(), &path_disp, Color::White);
    write_row(buf, ri.left(), input_row + 2, ri.right(),
        "Enter=export · Esc=back", dim);

    if let Some(msg) = &state.message {
        let col = if msg.contains("error") || msg.contains("Error") { err_col } else { ok_col };
        write_row(buf, ri.left(), ri.bottom().saturating_sub(2), ri.right(), msg, col);
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn write_row(buf: &mut Buffer, x: u16, y: u16, x_max: u16, text: &str, fg: Color) {
    for (i, ch) in text.chars().enumerate() {
        let col = x + i as u16;
        if col >= x_max { break; }
        buf.get_mut(col, y).set_char(ch).set_fg(fg);
    }
}
