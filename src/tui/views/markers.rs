/// Marker management list view.
///
/// Full-screen scrollable table of all stored markers.
///
/// Controls (handled in main.rs):
///   ↑ / ↓     navigate rows
///   E         edit selected marker (opens input overlay)
///   D         delete selected marker (shows confirmation bar)
///   G         go to selected marker on Globe view
///   P         go to selected marker on Map (Pan) view
///   X         clear all markers (same as X in other views)
///   Esc / Q   return to menu

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::Widget,
};
use crate::data::Marker;
use crate::render::canvas::TerminalCapability;
use crate::render::typography::GoldenLayout;

pub struct MarkerListView<'a> {
    pub markers:    &'a [Marker],
    pub selected:   usize,
    pub capability: TerminalCapability,
}

impl<'a> Widget for MarkerListView<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let tc = self.capability.supports_true_colour();
        let uc = self.capability.supports_unicode();

        // ── Colours ───────────────────────────────────────────────────────────
        let accent   = if tc { Color::Rgb(30, 200, 240) } else { Color::Cyan };
        let dim      = if tc { Color::Rgb(60, 90, 110)  } else { Color::DarkGray };
        let sel_bg   = if tc { Color::Rgb(5, 25, 45)    } else { Color::DarkGray };
        let warn     = if tc { Color::Rgb(255, 80, 60)  } else { Color::Red };
        let blink_fg = if tc { Color::Rgb(255, 200, 0)  } else { Color::Yellow };

        // ── GoldenLayout column widths ────────────────────────────────────────
        // Apply φ-proportioned column sizing:
        //   ID col      : fixed  5
        //   Symbol col  : fixed  7
        //   Coords col  : fixed  24  (lat + lon)
        //   Blink col   : fixed  2   (★ indicator)
        //   Label col   : remainder — grows with terminal width
        let layout   = GoldenLayout::compute(area.width, area.height);
        let fixed    = 5u16 + 7 + 24 + 2;
        // Label column gets the φ-proportioned secondary width, capped to remainder
        let label_w  = layout.secondary_cols
            .min(area.width.saturating_sub(fixed))
            .max(8);

        // ── Title bar ─────────────────────────────────────────────────────────
        let title = format!(
            " Markers — {} stored ",
            self.markers.len(),
        );
        let title_style = Style::default().fg(accent).add_modifier(Modifier::BOLD);
        // Fill title row with dim background
        for c in area.left()..area.right() {
            buf.get_mut(c, area.top()).set_char(' ').set_fg(dim).set_bg(Color::Reset);
        }
        for (i, ch) in title.chars().enumerate() {
            let c = area.left() + i as u16;
            if c >= area.right() { break; }
            buf.get_mut(c, area.top()).set_char(ch).set_style(title_style);
        }

        // ── Column header row ─────────────────────────────────────────────────
        if area.height < 3 { return; }
        let hrow = area.top() + 1;
        let header_style = Style::default().fg(dim).add_modifier(Modifier::BOLD);
        let sep = if uc { '─' } else { '-' };

        // Clear header row
        for c in area.left()..area.right() {
            buf.get_mut(c, hrow).set_char(sep).set_style(header_style);
        }
        let headers = ["ID", "SYM", "BLINK", "LATITUDE", "LONGITUDE", "LABEL"];
        let col_starts: &[u16] = &[0, 5, 12, 14, 24, 34];
        for (i, &hdr) in headers.iter().enumerate() {
            let base = area.left() + col_starts[i];
            for (j, ch) in hdr.chars().enumerate() {
                let c = base + j as u16;
                if c >= area.right() { break; }
                buf.get_mut(c, hrow).set_char(ch).set_style(header_style);
            }
        }

        // ── Marker rows ───────────────────────────────────────────────────────
        let list_top  = area.top() + 2;
        let list_bot  = area.bottom().saturating_sub(1); // reserve footer
        let visible   = list_bot.saturating_sub(list_top) as usize;

        // Scroll offset: keep selected row visible
        let scroll = if self.markers.is_empty() {
            0
        } else {
            let sel = self.selected.min(self.markers.len().saturating_sub(1));
            if sel < visible { 0 } else { sel - visible + 1 }
        };

        if self.markers.is_empty() {
            let msg = "  No markers yet.  Press M in Globe or Map view to add one.";
            let r   = list_top;
            for (i, ch) in msg.chars().enumerate() {
                let c = area.left() + i as u16;
                if c >= area.right() { break; }
                buf.get_mut(c, r).set_char(ch).set_fg(dim);
            }
        } else {
            for (vi, marker) in self.markers.iter().skip(scroll).enumerate() {
                let r = list_top + vi as u16;
                if r >= list_bot { break; }

                let is_sel = scroll + vi == self.selected;
                let row_bg = if is_sel { sel_bg } else { Color::Reset };
                let row_fg = if is_sel { Color::White } else { Color::Reset };

                // Clear row
                for c in area.left()..area.right() {
                    buf.get_mut(c, r).set_char(' ').set_bg(row_bg);
                }

                // Selection indicator
                let ind = if is_sel {
                    if uc { "▸" } else { ">" }
                } else {
                    " "
                };
                write_str(buf, area.left(), r, ind, Style::default().fg(accent).bg(row_bg));

                // ID
                let id_str = format!("{:>3}", marker.id);
                write_str(buf, area.left() + 1, r, &id_str,
                    Style::default().fg(dim).bg(row_bg));

                // Symbol
                let sym = if uc { &marker.symbol } else {
                    // VT-100: fall back to ascii_symbol char rendered as string
                    &format!("{}", marker.ascii_symbol())
                };
                let sym_style = if is_sel {
                    Style::default().fg(accent).bg(row_bg).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White).bg(row_bg)
                };
                write_str(buf, area.left() + 5, r, sym, sym_style);

                // Blink indicator
                let blink_str = if marker.blink {
                    if uc { "✦" } else { "*" }
                } else { " " };
                let blink_style = Style::default()
                    .fg(if marker.blink { blink_fg } else { dim })
                    .bg(row_bg);
                write_str(buf, area.left() + 12, r, blink_str, blink_style);

                // Latitude
                let lat_dir = if marker.lat >= 0.0 { 'N' } else { 'S' };
                let lat_str = format!("{:>8.3}°{}", marker.lat.abs(), lat_dir);
                write_str(buf, area.left() + 14, r, &lat_str,
                    Style::default().fg(row_fg).bg(row_bg));

                // Longitude
                let lon_dir = if marker.lon >= 0.0 { 'E' } else { 'W' };
                let lon_str = format!("{:>9.3}°{}", marker.lon.abs(), lon_dir);
                write_str(buf, area.left() + 24, r, &lon_str,
                    Style::default().fg(row_fg).bg(row_bg));

                // Label — truncated to label_w
                let label = truncate(&marker.label, label_w as usize);
                write_str(buf, area.left() + 34, r, &label,
                    Style::default().fg(row_fg).bg(row_bg));
            }

            // Scroll indicator
            if self.markers.len() > visible && visible > 0 {
                let pct = (scroll * 100) / (self.markers.len().saturating_sub(visible).max(1));
                let ind = format!(" {}%↕", pct);
                let istart = area.right().saturating_sub(ind.len() as u16 + 1);
                write_str(buf, istart, list_top, &ind, Style::default().fg(dim));
            }
        }

        // ── Footer / key hints ────────────────────────────────────────────────
        let footer_row = area.bottom().saturating_sub(1);
        let key_style = Style::default().fg(accent);
        let sep_style = Style::default().fg(dim);

        // Clear footer
        for c in area.left()..area.right() {
            buf.get_mut(c, footer_row).set_char(' ').set_bg(Color::Reset);
        }

        let hints: &[(&str, &str)] = &[
            ("↑↓", "navigate"),
            ("E", "edit"),
            ("D", "delete"),
            ("G", "globe"),
            ("P", "map"),
            ("X", "clear all"),
            ("Esc", "menu"),
        ];
        let mut cx = area.left() + 1;
        let sep_ch = if uc { " · " } else { " | " };
        for (i, &(key, desc)) in hints.iter().enumerate() {
            if cx >= area.right() { break; }
            if i > 0 {
                write_str(buf, cx, footer_row, sep_ch, sep_style);
                cx += sep_ch.len() as u16;
            }
            write_str(buf, cx, footer_row, &format!("[{}]", key), key_style);
            cx += (key.len() + 2) as u16;
            write_str(buf, cx, footer_row, &format!(" {}", desc), sep_style);
            cx += (desc.len() + 1) as u16;
        }

        // Warn colour for zero-marker state if X would be a no-op
        let _ = warn; // used in main.rs overlays
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn write_str(buf: &mut Buffer, x: u16, y: u16, s: &str, style: Style) {
    for (i, ch) in s.chars().enumerate() {
        let cx = x + i as u16;
        if cx >= buf.area.right() { break; }
        buf.get_mut(cx, y).set_char(ch).set_style(style);
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}
