//! Calculator view — slippy tiles, Web Mercator, DMS, haversine, bearing,
//! destination point.  After a computation that yields a geographic point the
//! user can jump to the globe/map or place a marker.

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Widget},
};

use crate::render::canvas::TerminalCapability;
use crate::tui::app::{CalcMode, CalcState};

pub struct CalcView<'a> {
    pub state:      &'a CalcState,
    pub capability: TerminalCapability,
}

impl<'a> Widget for CalcView<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let tc      = self.capability.supports_true_colour();
        let accent  = if tc { Color::Rgb(30, 200, 240) } else { Color::Cyan };
        let dim     = if tc { Color::Rgb(50, 80, 100)  } else { Color::DarkGray };
        let ok_col  = if tc { Color::Rgb(80, 220, 100) } else { Color::Green };
        let err_col = if tc { Color::Rgb(220, 80, 60)  } else { Color::Red };
        let hi_bg   = if tc { Color::Rgb(5, 20, 40)    } else { Color::DarkGray };

        // ── Layout ──────────────────────────────────────────────────────────
        // Left: mode list (≈30 cols). Right: input fields + results.
        let list_w = 30u16.min(area.width.saturating_sub(30));
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(list_w), Constraint::Min(10)])
            .split(area);

        // ── Left panel: calculator list ──────────────────────────────────────
        let left_block = Block::default()
            .title(" Calculators ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if self.state.focus_right { dim } else { accent }));
        let left_inner = left_block.inner(chunks[0]);
        left_block.render(chunks[0], buf);

        for (i, mode) in CalcMode::ALL.iter().enumerate() {
            let row = left_inner.top() + 1 + i as u16 * 2;
            if row >= left_inner.bottom().saturating_sub(2) { break; }

            let is_sel = i == self.state.mode_idx;
            let fg = if is_sel { accent } else { Color::White };
            let bg = if is_sel { hi_bg } else { Color::Reset };
            let marker = if is_sel { "▸ " } else { "  " };
            let label = format!("{marker}[{}] {}", mode.key(), mode.name());

            for (ci, ch) in label.chars().enumerate() {
                let col = left_inner.left() + ci as u16;
                if col >= left_inner.right() { break; }
                let cell = buf.get_mut(col, row);
                cell.set_char(ch).set_fg(fg).set_bg(bg);
                if is_sel {
                    cell.set_style(cell.style().add_modifier(Modifier::BOLD));
                }
            }
        }

        // Footer hint in left panel
        let lfooter = "↑↓ select · Tab=focus inputs";
        let lfy = left_inner.bottom().saturating_sub(1);
        write_row(buf, left_inner.left(), lfy, left_inner.right(), lfooter, dim);

        // ── Right panel: inputs + result ─────────────────────────────────────
        let mode = self.state.current_mode();
        let right_block = Block::default()
            .title(format!(" {} ", mode.name()))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if self.state.focus_right { accent } else { dim }));
        let ri = right_block.inner(chunks[1]);
        right_block.render(chunks[1], buf);

        let labels   = mode.field_labels();
        let label_w  = labels.iter().map(|l| l.len()).max().unwrap_or(10) as u16 + 2;
        let n_fields = labels.len();

        for (fi, label) in labels.iter().enumerate() {
            let row = ri.top() + 1 + fi as u16 * 2;
            if row >= ri.bottom().saturating_sub(8) { break; }

            let is_active = self.state.focus_right && fi == self.state.field_idx;

            // Label column
            let lbl = format!("{label}:");
            for (ci, ch) in lbl.chars().enumerate() {
                let col = ri.left() + ci as u16;
                if col >= ri.right() { break; }
                buf.get_mut(col, row)
                   .set_char(ch)
                   .set_fg(if is_active { accent } else { dim });
            }

            // Value column
            let val_start = ri.left() + label_w;
            let val = &self.state.fields[fi];
            let display = if is_active { format!("{val}█") } else { val.clone() };
            for (ci, ch) in display.chars().enumerate() {
                let col = val_start + ci as u16;
                if col >= ri.right() { break; }
                buf.get_mut(col, row)
                   .set_char(ch)
                   .set_fg(if is_active { Color::White } else { Color::Rgb(160, 160, 160) });
            }
        }

        // Separator
        let sep_row = ri.top() + 1 + n_fields as u16 * 2 + 1;
        if sep_row < ri.bottom().saturating_sub(4) {
            let sep: String = std::iter::repeat('─').take(ri.width as usize).collect();
            write_row(buf, ri.left(), sep_row, ri.right(), &sep, dim);
        }

        // Result / error
        let result_top = (sep_row + 1).min(ri.bottom().saturating_sub(4));
        if let Some(err) = &self.state.error {
            if result_top < ri.bottom() {
                write_row(buf, ri.left(), result_top, ri.right(), &format!("⚠  {err}"), err_col);
            }
        } else if let Some(res) = &self.state.result {
            for (li, line) in res.lines.iter().enumerate() {
                let row = result_top + li as u16;
                if row >= ri.bottom().saturating_sub(2) { break; }
                write_row(buf, ri.left(), row, ri.right(), line, ok_col);
            }

            // Place / go-to hint
            if res.latlon.is_some() {
                let hint = "P=place marker  G=go to globe  M=go to map";
                let hr = ri.bottom().saturating_sub(2);
                if hr > result_top {
                    write_row(buf, ri.left(), hr, ri.right(), hint, dim);
                }
            }
        }

        // Footer
        let rfooter = if self.state.focus_right {
            "Tab/↑↓=next field · Enter=compute · Esc=back"
        } else {
            "Tab/Enter=focus inputs · q=menu"
        };
        write_row(buf, ri.left(), ri.bottom().saturating_sub(1), ri.right(), rfooter, dim);
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────────

fn write_row(buf: &mut Buffer, x: u16, y: u16, x_max: u16, text: &str, fg: Color) {
    for (i, ch) in text.chars().enumerate() {
        let col = x + i as u16;
        if col >= x_max { break; }
        buf.get_mut(col, y).set_char(ch).set_fg(fg);
    }
}
