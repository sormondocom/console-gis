/// Layer manager view — scrollable list of GeoJSON layers with visibility toggle.
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color as RColor, Modifier, Style},
    widgets::Widget,
};
use crate::render::canvas::TerminalCapability;
use crate::tui::app::LayerEntry;

const TC_COLS: &[(u8, u8, u8)] = &[
    (0, 220, 220),
    (220, 180, 0),
    (180, 80, 220),
    (80, 220, 80),
    (220, 80, 80),
];
const A8_COLS: &[RColor] = &[
    RColor::Cyan, RColor::Yellow, RColor::Magenta, RColor::Green, RColor::Red,
];

pub struct LayerManagerView<'a> {
    pub layers:       &'a [LayerEntry],
    pub selected:     usize,
    pub capability:   TerminalCapability,
    pub topo_enabled: bool,
}

impl<'a> Widget for LayerManagerView<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let tc  = self.capability.supports_true_colour();
        let uni = self.capability.supports_unicode();
        let rows = area.height as usize;

        // ── Title bar ─────────────────────────────────────────────────────────
        let title_style = if tc {
            Style::default().fg(RColor::Rgb(180, 220, 255)).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(RColor::Cyan).add_modifier(Modifier::BOLD)
        };
        // Count visible: topo + geo layers
        let geo_vis = self.layers.iter().filter(|e| e.visible).count();
        let vis_total = geo_vis + if self.topo_enabled { 1 } else { 0 };
        let total_layers = 1 + self.layers.len(); // 1 built-in + geo layers
        let title = format!(
            " Layers  │  {} layer{}  │  {} visible ",
            total_layers,
            if total_layers == 1 { "" } else { "s" },
            vis_total,
        );
        for (i, ch) in title.chars().enumerate() {
            let c = area.x + i as u16;
            if c >= area.x + area.width { break; }
            buf.get_mut(c, area.y).set_char(ch).set_style(title_style);
        }

        // ── Column header ─────────────────────────────────────────────────────
        if rows < 3 { return; }
        let hdr = format!("  {:<2}  {:<3}  {:<4}  {:<30}  {}", "#", "VIS", "COL", "Name", "Features");
        let hdr_style = if tc {
            Style::default().fg(RColor::Rgb(100, 100, 140))
        } else {
            Style::default().fg(RColor::DarkGray)
        };
        for (i, ch) in hdr.chars().enumerate() {
            let c = area.x + i as u16;
            if c >= area.x + area.width { break; }
            buf.get_mut(c, area.y + 1).set_char(ch).set_style(hdr_style);
        }

        // ── Layer list ────────────────────────────────────────────────────────
        // Total rows = 1 built-in topo + geo layers.len()
        let total_rows = 1 + self.layers.len();
        let list_rows = rows.saturating_sub(4); // title + header + separator + footer
        let scroll = if self.selected >= list_rows {
            self.selected - list_rows + 1
        } else {
            0
        };

        let mut display_row = 0usize;
        let mut term_y = area.y + 2;

        // ── Built-in topo row (index 0) ───────────────────────────────────────
        if scroll == 0 && display_row < list_rows {
            let is_sel = self.selected == 0;
            let arrow = if is_sel { if uni { "►" } else { ">" } } else { " " };
            let vis   = if self.topo_enabled { if uni { "●" } else { "*" } } else { if uni { "·" } else { " " } };
            let swatch = if uni { "■" } else { "#" };
            let row_text = format!("{}  {:>2}  {}  {}  {:<30}  {}",
                arrow, 0, vis, swatch, "Topography  (built-in)", "-",
            );
            let topo_swatch_style = if tc {
                Style::default().fg(RColor::Rgb(120, 90, 60))
            } else {
                Style::default().fg(RColor::Red)
            };
            let row_style = if is_sel {
                if tc {
                    Style::default()
                        .fg(RColor::Rgb(220, 220, 255))
                        .bg(RColor::Rgb(28, 28, 50))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(RColor::White).add_modifier(Modifier::REVERSED)
                }
            } else if !self.topo_enabled {
                if tc { Style::default().fg(RColor::Rgb(60, 60, 80)) }
                else  { Style::default().fg(RColor::DarkGray) }
            } else {
                Style::default()
            };
            for (i, ch) in row_text.chars().enumerate() {
                let c = area.x + i as u16;
                if c >= area.x + area.width { break; }
                buf.get_mut(c, term_y).set_char(ch).set_style(row_style);
            }
            // Re-paint swatch with earth-tone colour
            let swatch_col = area.x + 10;
            if swatch_col < area.x + area.width {
                buf.get_mut(swatch_col, term_y).set_style(topo_swatch_style);
            }
            term_y += 1;
            display_row += 1;
        }

        // ── Separator line ────────────────────────────────────────────────────
        if display_row < list_rows && term_y < area.y + area.height {
            let sep_style = if tc {
                Style::default().fg(RColor::Rgb(50, 50, 70))
            } else {
                Style::default().fg(RColor::DarkGray)
            };
            let sep = "  ─────────────────────────────────────────────────────";
            for (i, ch) in sep.chars().enumerate() {
                let c = area.x + i as u16;
                if c >= area.x + area.width { break; }
                buf.get_mut(c, term_y).set_char(ch).set_style(sep_style);
            }
            term_y += 1;
            display_row += 1;
        }

        // ── GeoJSON layers (indices 1..=N) ────────────────────────────────────
        // Adjust scroll to skip built-in row + separator if scrolled past them
        let geo_scroll = if scroll > 1 { scroll - 2 } else { 0 };

        if self.layers.is_empty() && scroll == 0 {
            if display_row < list_rows && term_y < area.y + area.height {
                let msg = "  No GeoJSON layers loaded.  Press I in Globe or Map to import.";
                let style = if tc {
                    Style::default().fg(RColor::Rgb(80, 80, 100))
                } else {
                    Style::default().fg(RColor::DarkGray)
                };
                for (i, ch) in msg.chars().enumerate() {
                    let c = area.x + i as u16;
                    if c >= area.x + area.width { break; }
                    buf.get_mut(c, term_y).set_char(ch).set_style(style);
                }
            }
        } else {
            for (entry_idx, entry) in self.layers.iter()
                .enumerate()
                .skip(geo_scroll)
            {
                if display_row >= list_rows || term_y >= area.y + area.height { break; }
                let list_idx = entry_idx + 1; // +1 for built-in topo row
                let is_sel = list_idx == self.selected;

                let arrow = if is_sel { if uni { "►" } else { ">" } } else { " " };
                let vis   = if entry.visible { if uni { "●" } else { "*" } } else { if uni { "·" } else { " " } };
                let swatch = if uni { "■" } else { "#" };
                let col_style = match self.capability {
                    TerminalCapability::TrueColor => {
                        let c = TC_COLS[entry.color_index as usize % TC_COLS.len()];
                        Style::default().fg(RColor::Rgb(c.0, c.1, c.2))
                    }
                    _ => Style::default().fg(A8_COLS[entry.color_index as usize % A8_COLS.len()]),
                };

                let feat_count = entry.layer.features.len();
                let row_text = format!("{}  {:>2}  {}  {}  {:<30}  {}",
                    arrow,
                    list_idx,
                    vis,
                    swatch,
                    if entry.label.len() > 30 {
                        format!("{}…", &entry.label[..29])
                    } else {
                        entry.label.clone()
                    },
                    feat_count,
                );

                let row_style = if is_sel {
                    if tc {
                        Style::default()
                            .fg(RColor::Rgb(220, 220, 255))
                            .bg(RColor::Rgb(28, 28, 50))
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(RColor::White).add_modifier(Modifier::REVERSED)
                    }
                } else if !entry.visible {
                    if tc {
                        Style::default().fg(RColor::Rgb(60, 60, 80))
                    } else {
                        Style::default().fg(RColor::DarkGray)
                    }
                } else {
                    Style::default()
                };

                for (i, ch) in row_text.chars().enumerate() {
                    let c = area.x + i as u16;
                    if c >= area.x + area.width { break; }
                    buf.get_mut(c, term_y).set_char(ch).set_style(row_style);
                }
                let swatch_col = area.x + 10;
                if swatch_col < area.x + area.width {
                    buf.get_mut(swatch_col, term_y).set_style(col_style);
                }
                term_y += 1;
                display_row += 1;
            }
        }

        // Suppress unused variable warning
        let _ = total_rows;

        // ── Footer / key hints ────────────────────────────────────────────────
        let footer_row = area.y + area.height.saturating_sub(1);
        let hint = if uni {
            "  ↑/↓ navigate  Space toggle  D delete  T topo  Esc back"
        } else {
            "  Up/Dn navigate  Space toggle  D delete  T topo  Esc back"
        };
        let hint_style = if tc {
            Style::default().fg(RColor::Rgb(70, 70, 90))
        } else {
            Style::default().fg(RColor::DarkGray)
        };
        for (i, ch) in hint.chars().enumerate() {
            let c = area.x + i as u16;
            if c >= area.x + area.width { break; }
            buf.get_mut(c, footer_row).set_char(ch).set_style(hint_style);
        }
    }
}
