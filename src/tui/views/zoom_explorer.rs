/// Zoom Explorer — shows what each zoom level 0–20 means in console terms.
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::Widget,
};
use crate::geo::zoom::{ConsoleResolution, RenderMode, ZOOM_MIN, ZOOM_MAX};
use crate::render::canvas::TerminalCapability;

pub struct ZoomExplorerView {
    pub zoom:       u8,
    pub cols:       u16,
    pub rows:       u16,
    pub capability: TerminalCapability,
}

impl Widget for ZoomExplorerView {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let true_col = self.capability.supports_true_colour();

        let accent = if true_col { Color::Rgb(30, 200, 240) } else { Color::Cyan };
        let dim    = if true_col { Color::Rgb(50, 80, 100)  } else { Color::DarkGray };
        let hi     = if true_col { Color::Rgb(255, 220, 0)  } else { Color::Yellow };

        // Title row
        let title = format!(
            " Zoom Explorer  │  zoom {:>2}  │  {}  │  ↑↓ change  Esc back ",
            self.zoom,
            ConsoleResolution::zoom_label(self.zoom)
        );
        for (i, ch) in title.chars().enumerate() {
            if i >= area.width as usize { break; }
            let cell = buf.get_mut(area.x + i as u16, area.y);
            cell.set_char(ch);
            cell.set_fg(accent);
        }

        // Column headers
        let header_row = area.y + 2;
        let headers = [
            ("Mode",      6),
            ("CPE/cell",  10),
            ("m/CPE",     12),
            ("lon extent",14),
            ("lat extent",14),
            ("CPE total", 12),
        ];
        let mut hx = area.x + 2;
        for (hdr, w) in &headers {
            for (j, ch) in hdr.chars().enumerate() {
                if hx + j as u16 >= area.right() { break; }
                let cell = buf.get_mut(hx + j as u16, header_row);
                cell.set_char(ch);
                cell.set_fg(dim);
                cell.set_style(cell.style().add_modifier(Modifier::UNDERLINED));
            }
            hx += w;
        }

        // One row per render mode
        let modes = [
            ("ASCII    ", RenderMode::Ascii),
            ("Block    ", RenderMode::Block),
            ("HalfBlock", RenderMode::HalfBlock),
            ("Braille  ", RenderMode::Braille),
        ];

        for (row_off, (mode_label, mode)) in modes.iter().enumerate() {
            let row = header_row + 2 + row_off as u16;
            if row >= area.bottom() { break; }

            let res = ConsoleResolution::new(*mode);
            let mpp = res.metres_per_cpe(self.zoom);
            let (lon_ext, lat_ext) = res.viewport_extent_deg(self.cols, self.rows, self.zoom, 0.0);
            let (cpe_w, cpe_h) = mode.cpe_per_cell();
            let total_cpe = self.cols as u64 * cpe_w as u64
                          * self.rows as u64 * cpe_h as u64;

            // Highlight active mode
            let is_active = *mode == RenderMode::HalfBlock; // default
            let fg = if is_active { hi } else { Color::White };

            let cols_data: Vec<String> = vec![
                mode_label.to_string(),
                format!("{}×{}", cpe_w, cpe_h),
                fmt_metres(mpp),
                fmt_degrees(lon_ext),
                fmt_degrees(lat_ext),
                total_cpe.to_string(),
            ];

            let mut cx = area.x + 2;
            for (val, (_, w)) in cols_data.iter().zip(headers.iter()) {
                for (j, ch) in val.chars().enumerate() {
                    if cx + j as u16 >= area.right() { break; }
                    let cell = buf.get_mut(cx + j as u16, row);
                    cell.set_char(ch);
                    cell.set_fg(fg);
                    if is_active {
                        cell.set_style(cell.style().add_modifier(Modifier::BOLD));
                    }
                }
                cx += w;
            }
        }

        // ── Zoom ruler ───────────────────────────────────────────────────────
        // A horizontal bar showing positions 0–20 with current zoom marked.
        let ruler_row = area.bottom().saturating_sub(3);
        let ruler_label = "  zoom: ";
        for (i, ch) in ruler_label.chars().enumerate() {
            let cell = buf.get_mut(area.x + i as u16, ruler_row);
            cell.set_char(ch);
            cell.set_fg(dim);
        }

        let ruler_start = area.x + ruler_label.len() as u16;
        for z in ZOOM_MIN..=ZOOM_MAX {
            let col = ruler_start + z as u16 * 2;
            if col + 2 >= area.right() { break; }

            let is_cur = z == self.zoom;
            let ch = if is_cur { '●' } else { '·' };
            let fg = if is_cur { hi } else { dim };

            let cell = buf.get_mut(col, ruler_row);
            cell.set_char(ch);
            cell.set_fg(fg);

            // Tick labels every 5 levels
            if z % 5 == 0 {
                let lbl = z.to_string();
                for (j, c) in lbl.chars().enumerate() {
                    let cell = buf.get_mut(col + j as u16, ruler_row + 1);
                    cell.set_char(c);
                    cell.set_fg(dim);
                }
            }
        }
    }
}

fn fmt_metres(m: f64) -> String {
    if m >= 1_000.0 {
        format!("{:.1} km", m / 1_000.0)
    } else if m >= 1.0 {
        format!("{:.1} m", m)
    } else {
        format!("{:.3} m", m)
    }
}

fn fmt_degrees(deg: f64) -> String {
    if deg >= 1.0 {
        format!("{:.2}°", deg)
    } else {
        format!("{:.4}°", deg)
    }
}
