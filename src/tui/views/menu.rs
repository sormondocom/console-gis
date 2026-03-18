/// Main navigation menu — GIS-domain-specific layout.
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Widget},
};
use crate::tui::app::View;
use crate::render::typography::GoldenLayout;

/// Menu entries — each maps to a [`View`] and has a short GIS-flavoured blurb.
pub const MENU_ITEMS: &[MenuItem] = &[
    MenuItem {
        key:   '1',
        label: "Globe",
        view:  View::Globe,
        icon:  "◉",
        desc:  "Rotating 3-D orthographic projection.  Graticule + special parallels.\n\nControls: A/D rotate · ↑↓ tilt · W/S zoom · Space pause · M marker · I import · B bookmark",
    },
    MenuItem {
        key:   '2',
        label: "World Map",
        view:  View::Map,
        icon:  "▭",
        desc:  "Flat Web Mercator map.  Pan with arrow keys, zoom with W/S or +/−.\n\nControls: ↑↓←→ pan · W/S zoom · M marker · I import · B bookmark",
    },
    MenuItem {
        key:   '3',
        label: "Markers",
        view:  View::MarkerList,
        icon:  "◆",
        desc:  "Browse, edit, and delete geographic annotations.\n\nControls: ↑↓ navigate · E edit · D delete · G go to globe · P go to map · X clear all",
    },
    MenuItem {
        key:   '4',
        label: "Zoom Explorer",
        view:  View::ZoomExplorer,
        icon:  "⊕",
        desc:  "Inspect zoom levels 0–20: ground resolution, viewport extent, CPE count.",
    },
    MenuItem {
        key:   '5',
        label: "Terminal Diagnostics",
        view:  View::Diagnostics,
        icon:  "≡",
        desc:  "Colour capability, character metrics, effective DPI, render mode.",
    },
    MenuItem {
        key:   '6',
        label: "Layers",
        view:  View::Layers,
        icon:  "≋",
        desc:  "Manage imported GeoJSON layers — toggle visibility, delete, view details.\n\nControls: ↑↓ navigate · Space toggle visibility · D delete · Esc back",
    },
    MenuItem {
        key:   '7',
        label: "Calculator",
        view:  View::Calculator,
        icon:  "⊞",
        desc:  "Geographic calculators: slippy tile XY, Web Mercator (EPSG:3857), DMS/DDM conversions, haversine distance, bearing, destination point.\n\nAfter computing, press P to place the result as a marker, G to jump to the globe, or M to jump to the map.",
    },
    MenuItem {
        key:   '8',
        label: "Shape Editor",
        view:  View::ShapeEditor,
        icon:  "◈",
        desc:  "Interactively define Point, MultiPoint, LineString, MultiLineString, Polygon, or MultiPolygon geometries by entering coordinates one at a time.\n\nExports a valid GeoJSON FeatureCollection that can be re-imported as a layer.\n\nControls: ↑↓/1-6 pick type · Enter start · Tab switch lat/lon · Enter add coord · F finish part · U undo · N next step",
    },
];

pub struct MenuItem {
    pub key:   char,
    pub label: &'static str,
    pub view:  View,
    pub icon:  &'static str,
    pub desc:  &'static str,
}

/// Stateful menu — tracks the highlighted row.
pub struct MenuWidget<'a> {
    pub items:     &'a [MenuItem],
    pub selected:  usize,
    pub true_color: bool,
}

impl<'a> Widget for MenuWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Apply golden-ratio split: sidebar ≈ 38.2% (primary), desc ≈ 61.8% (secondary).
        let layout = GoldenLayout::compute(area.width, area.height);
        let sidebar_w = layout.primary_cols.max(24).min(area.width.saturating_sub(20));

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(sidebar_w), Constraint::Min(10)])
            .split(area);

        // ── Item list ────────────────────────────────────────────────────────
        let accent = if self.true_color {
            Color::Rgb(30, 200, 240)
        } else {
            Color::Cyan
        };
        let dim = if self.true_color {
            Color::Rgb(50, 80, 100)
        } else {
            Color::DarkGray
        };

        let block = Block::default()
            .title(" console-gis ")
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(accent));

        let inner = block.inner(chunks[0]);
        block.render(chunks[0], buf);

        // Blank out inner area
        for row in inner.top()..inner.bottom() {
            for col in inner.left()..inner.right() {
                buf.get_mut(col, row).reset();
            }
        }

        let start_row = inner.top() + 1;
        for (i, item) in self.items.iter().enumerate() {
            let row = start_row + i as u16 * 2;
            if row >= inner.bottom() { break; }

            let is_sel = i == self.selected;
            let fg = if is_sel { accent } else { Color::White };
            let bg = if is_sel && self.true_color {
                Color::Rgb(5, 20, 40)
            } else if is_sel {
                Color::DarkGray
            } else {
                Color::Reset
            };

            let marker = if is_sel { "▸ " } else { "  " };
            let label = format!("{marker}{} {}", item.icon, item.label);
            let key_hint = format!("[{}]", item.key);

            // Write label
            for (col_off, ch) in label.chars().enumerate() {
                let col = inner.left() + col_off as u16;
                if col >= inner.right() { break; }
                let cell = buf.get_mut(col, row);
                cell.set_char(ch);
                cell.set_fg(fg);
                cell.set_bg(bg);
                if is_sel {
                    cell.set_style(cell.style().add_modifier(Modifier::BOLD));
                }
            }

            // Write key hint at right edge
            let kh_start = inner.right().saturating_sub(key_hint.len() as u16 + 1);
            for (j, ch) in key_hint.chars().enumerate() {
                let col = kh_start + j as u16;
                if col >= inner.right() { break; }
                let cell = buf.get_mut(col, row);
                cell.set_char(ch);
                cell.set_fg(dim);
            }
        }

        // Footer hint
        let footer = "↑↓ navigate · Enter/key select · q quit";
        let fy = inner.bottom().saturating_sub(1);
        for (i, ch) in footer.chars().enumerate() {
            let col = inner.left() + i as u16;
            if col >= inner.right() { break; }
            let cell = buf.get_mut(col, fy);
            cell.set_char(ch);
            cell.set_fg(dim);
        }

        // ── Description panel ────────────────────────────────────────────────
        let desc_block = Block::default()
            .title(" — ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(dim));

        let desc_inner = desc_block.inner(chunks[1]);
        desc_block.render(chunks[1], buf);

        let item = &self.items[self.selected];

        // Large icon centred
        let icon_row = desc_inner.top() + 1;
        let icon = item.icon;
        let icon_col = desc_inner.left() + (desc_inner.width.saturating_sub(icon.len() as u16)) / 2;
        for (j, ch) in icon.chars().enumerate() {
            let cell = buf.get_mut(icon_col + j as u16, icon_row);
            cell.set_char(ch);
            cell.set_fg(accent);
            cell.set_style(cell.style().add_modifier(Modifier::BOLD));
        }

        // Title
        let title_row = icon_row + 2;
        let title = item.label;
        let title_col = desc_inner.left() + (desc_inner.width.saturating_sub(title.len() as u16)) / 2;
        for (j, ch) in title.chars().enumerate() {
            let cell = buf.get_mut(title_col + j as u16, title_row);
            cell.set_char(ch);
            cell.set_fg(Color::White);
            cell.set_style(cell.style().add_modifier(Modifier::BOLD));
        }

        // Description — word-wrapped, honours \n as paragraph break
        let desc_row = title_row + 2;
        let max_w = desc_inner.width as usize;
        let mut row_off = 0u16;
        for para in item.desc.split('\n') {
            if row_off > 0 { row_off += 1; } // blank line between paragraphs
            let mut col_off = 0usize;
            for word in para.split_whitespace() {
                if col_off + word.len() + 1 > max_w {
                    col_off = 0;
                    row_off += 1;
                }
                for ch in word.chars() {
                    let r = desc_row + row_off;
                    let c = desc_inner.left() + col_off as u16;
                    if r < desc_inner.bottom() && c < desc_inner.right() {
                        buf.get_mut(c, r).set_char(ch).set_fg(dim);
                    }
                    col_off += 1;
                }
                col_off += 1; // trailing space
            }
            row_off += 1;
        }
    }
}
