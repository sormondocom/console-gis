/// Full-screen rotating globe view.
///
/// Controls:
///   A / ← : rotate west
///   D / → : rotate east
///   ↑      : tilt north
///   ↓      : tilt south
///   W      : zoom in
///   S      : zoom out
///   M      : place marker at current globe centre
///   Space  : toggle auto-rotate
///   Esc/Q  : return to menu

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color as RColor, Modifier, Style},
    widgets::Widget,
};
use crate::render::canvas::TerminalCapability;
use crate::render::globe::{GlobeParams, pixel_color_pub, project_latlon};
use crate::data::{WorldMap, Marker, GeoLayer};
use crate::data::markers::project_marker;

pub struct GlobeView<'a> {
    pub params:     &'a GlobeParams,
    pub capability: TerminalCapability,
    pub world:      &'a WorldMap,
    pub markers:    &'a [Marker],
    pub layers:     &'a [GeoLayer],
    pub animating:  bool,
    pub cursor_lat: f64,
    pub cursor_lon: f64,
    pub placing:    bool,
}

impl<'a> Widget for GlobeView<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let cols = area.width  as usize;
        let rows = area.height as usize;
        let globe_rows = rows.saturating_sub(1).max(1);

        let pw     = cols;
        let ph     = globe_rows * 2; // half-block pixels
        let cx     = pw as f64 / 2.0;
        let cy     = ph as f64 / 2.0;
        let scale  = cx.min(cy) * 0.95;

        // ── Render sphere pixels ──────────────────────────────────────────────
        for row in 0..globe_rows {
            for col in 0..cols {
                let top = pixel_color_pub(col, row * 2,     cx, cy, scale, self.params, self.world);
                let bot = pixel_color_pub(col, row * 2 + 1, cx, cy, scale, self.params, self.world);

                let cell = buf.get_mut(area.x + col as u16, area.y + row as u16);

                match self.capability {
                    TerminalCapability::TrueColor => {
                        cell.set_char('▀');
                        cell.set_fg(RColor::Rgb(top.0, top.1, top.2));
                        cell.set_bg(RColor::Rgb(bot.0, bot.1, bot.2));
                    }
                    TerminalCapability::Color256 | TerminalCapability::Ansi8 => {
                        cell.set_char('▀');
                        cell.set_fg(ansi8_color(top));
                    }
                    TerminalCapability::Vt100 | TerminalCapability::Vt220 => {
                        let lum = luminance(top);
                        cell.set_char(ascii_shade(lum));
                    }
                }
            }
        }

        // ── Render markers ────────────────────────────────────────────────────
        for marker in self.markers {
            if let Some((mc, mr)) = project_marker(marker, self.params, cx, cy / 2.0, scale) {
                // mr is in half-block pixel rows; convert to terminal rows
                let term_row = mr / 2;
                if mc >= 0 && (mc as usize) < cols
                    && term_row >= 0 && (term_row as usize) < globe_rows
                {
                    let col = area.x + mc as u16;
                    let row = area.y + term_row as u16;
                    let cell = buf.get_mut(col, row);

                    let sym = if self.capability.supports_unicode() {
                        marker.symbol.chars().next().unwrap_or('*')
                    } else {
                        marker.ascii_symbol()
                    };
                    cell.set_char(sym);
                    cell.set_fg(if self.capability.supports_true_colour() {
                        RColor::Rgb(255, 220, 0)
                    } else {
                        RColor::Yellow
                    });
                    let mut mods = Modifier::BOLD;
                    if marker.blink { mods |= Modifier::SLOW_BLINK; }
                    cell.set_style(cell.style().add_modifier(mods));

                    // Label to the right of the symbol (if space available)
                    for (i, ch) in marker.label.chars().enumerate() {
                        let lc = col + 1 + i as u16;
                        if lc >= area.x + area.width { break; }
                        let lcell = buf.get_mut(lc, row);
                        lcell.set_char(ch);
                        lcell.set_fg(if self.capability.supports_true_colour() {
                            RColor::Rgb(200, 200, 200)
                        } else {
                            RColor::White
                        });
                    }
                }
            }
        }

        // ── GeoJSON layer rendering ───────────────────────────────────────────
        //
        // Layer colours cycle: cyan, gold, violet, lime, coral.
        const TC_COLS: &[(u8, u8, u8)] = &[
            (0, 220, 220), (220, 180, 0), (180, 80, 220), (80, 220, 80), (220, 80, 80),
        ];
        const A8_COLS: &[RColor] = &[
            RColor::Cyan, RColor::Yellow, RColor::Magenta, RColor::Green, RColor::Red,
        ];

        for (li, layer) in self.layers.iter().enumerate() {
            let fg = match self.capability {
                TerminalCapability::TrueColor => {
                    let c = TC_COLS[li % TC_COLS.len()];
                    RColor::Rgb(c.0, c.1, c.2)
                }
                _ => A8_COLS[li % A8_COLS.len()],
            };
            let lstyle = Style::default().fg(fg);
            let seg_char = if self.capability.supports_unicode() { '·' } else { '.' };

            // ── Line segments: sample the arc to approximate great-circle paths
            for ((lon0, lat0), (lon1, lat1)) in layer.segments() {
                // Sample up to 16 intermediate points along each segment so
                // curves and longer edges look smooth at large zoom levels.
                let dist = ((lat1 - lat0).powi(2) + (lon1 - lon0).powi(2)).sqrt();
                let steps = ((dist / 5.0) as usize).clamp(1, 16);
                let mut prev: Option<(i32, i32)> = None;
                for i in 0..=steps {
                    let t   = i as f64 / steps as f64;
                    let lat = lat0 + t * (lat1 - lat0);
                    let lon = lon0 + t * (lon1 - lon0);
                    let cur = project_latlon(lat, lon, self.params, cx, cy / 2.0, scale);
                    if let (Some((pc, pr)), Some((cc, cr))) = (prev, cur) {
                        // Convert half-block pixel rows to terminal rows
                        let (ptr, ctr) = (pr / 2, cr / 2);
                        if pc >= 0 && (pc as usize) < cols
                            && cc >= 0 && (cc as usize) < cols
                            && ptr >= 0 && (ptr as usize) < globe_rows
                            && ctr >= 0 && (ctr as usize) < globe_rows
                        {
                            draw_globe_line(
                                buf, area, pc, ptr, cc, ctr,
                                seg_char, lstyle, cols, globe_rows,
                            );
                        }
                    }
                    prev = cur.map(|(c, r)| (c, r / 2));
                    if cur.is_none() { prev = None; } // gap at limb
                }
            }

            // ── Point coords (Point / MultiPoint features)
            for (lon, lat) in layer.all_point_coords() {
                if let Some((sc, sr)) = project_latlon(lat, lon, self.params, cx, cy / 2.0, scale) {
                    let tr = sr / 2;
                    if sc >= 0 && (sc as usize) < cols && tr >= 0 && (tr as usize) < globe_rows {
                        let cell = buf.get_mut(area.x + sc as u16, area.y + tr as u16);
                        cell.set_char(if self.capability.supports_unicode() { '◆' } else { '*' });
                        cell.set_fg(if self.capability.supports_true_colour() {
                            RColor::Rgb(255, 220, 0)
                        } else {
                            RColor::Yellow
                        });
                        cell.set_style(cell.style().add_modifier(Modifier::BOLD));
                    }
                }
            }
        }

        // ── Crosshair cursor (shown when placing a marker) ────────────────────
        if self.placing {
            let cc = area.x + area.width  / 2;
            let cr = area.y + (globe_rows as u16) / 2;
            if cc > area.x && cr > area.y {
                buf.get_mut(cc, cr).set_char('+')
                    .set_fg(RColor::Green)
                    .set_style(Style::default().add_modifier(Modifier::BOLD));
            }
        }

        // ── Status bar ────────────────────────────────────────────────────────
        let status_row = area.y + globe_rows as u16;
        let lon = self.params.rot_y.to_degrees() % 360.0;
        let lat = -self.params.rot_x.to_degrees();
        let zoom_pct = (self.params.zoom * 100.0) as u32;
        let anim = if self.animating { "auto" } else { "paused" };
        let mark_hint = if self.placing {
            "  [Enter] place · [Esc] cancel"
        } else {
            "  [M] mark · [I] import · [Space] pause"
        };
        let layer_info = if self.layers.is_empty() {
            String::new()
        } else {
            format!("  │  {} layer{}", self.layers.len(),
                if self.layers.len() == 1 { "" } else { "s" })
        };

        // Equivalent Web Mercator zoom level and ground resolution for
        // apples-to-apples comparison with the flat map view.
        let eq_zoom = (2.0_f64 + self.params.zoom.clamp(0.5, 8.0).log2() * 1.5)
            .round()
            .clamp(0.0, 20.0) as u8;
        // Earth circumference (WGS-84): 2π × 6,378,137 m = 40,075,016.686 m
        const EARTH_CIRC: f64 = 40_075_016.686;
        let mpp = EARTH_CIRC / (256.0 * (1u64 << eq_zoom) as f64);
        let res_str = fmt_resolution(mpp);

        let status = format!(
            " console-gis  │  WGS-84 → Ortho  │  {:.1}°N {:.1}°E  │  {zoom_pct}%  │  ≈z{eq_zoom}  │  {res_str}  │  {anim}{layer_info}{mark_hint}  │  Esc menu ",
            lat, lon
        );
        let status_style = if self.capability.supports_true_colour() {
            Style::default()
                .fg(RColor::Rgb(80, 80, 80))
                .bg(RColor::Rgb(6, 6, 15))
        } else {
            Style::default().fg(RColor::DarkGray)
        };

        for (i, ch) in status.chars().enumerate() {
            let col = area.x + i as u16;
            if col >= area.x + area.width { break; }
            let cell = buf.get_mut(col, status_row);
            cell.set_char(ch);
            cell.set_style(status_style);
        }
    }
}

// ── Resolution formatter ──────────────────────────────────────────────────────

fn fmt_resolution(mpp: f64) -> String {
    if mpp >= 100_000.0 {
        format!("{:.0} km/C", mpp / 1000.0)
    } else if mpp >= 1_000.0 {
        format!("{:.1} km/C", mpp / 1000.0)
    } else if mpp >= 1.0 {
        format!("{:.0} m/C", mpp)
    } else {
        format!("{:.2} m/C", mpp)
    }
}

// ── Bresenham line (globe, character-space) ───────────────────────────────────

fn draw_globe_line(
    buf:       &mut Buffer,
    area:      Rect,
    x0: i32, y0: i32,
    x1: i32, y1: i32,
    ch:        char,
    style:     Style,
    max_cols:  usize,
    max_rows:  usize,
) {
    let dx  =  (x1 - x0).abs();
    let dy  = -(y1 - y0).abs();
    let sx  = if x0 < x1 { 1i32 } else { -1 };
    let sy  = if y0 < y1 { 1i32 } else { -1 };
    let mut err = dx + dy;
    let (mut x, mut y) = (x0, y0);
    loop {
        if x >= 0 && (x as usize) < max_cols && y >= 0 && (y as usize) < max_rows {
            buf.get_mut(area.x + x as u16, area.y + y as u16)
                .set_char(ch).set_style(style);
        }
        if x == x1 && y == y1 { break; }
        let e2 = 2 * err;
        if e2 >= dy { err += dy; x += sx; }
        if e2 <= dx { err += dx; y += sy; }
    }
}

// ── Colour helpers ─────────────────────────────────────────────────────────────

fn ansi8_color(rgb: (u8, u8, u8)) -> RColor {
    let (r, g, b) = rgb;
    if b > r && b > g { return RColor::Blue; }
    if g > r && g > b { return RColor::Green; }
    if r > 150 && g > 150 { return RColor::Yellow; }
    if b > 150 && g > 150 { return RColor::Cyan; }
    let lum = (r as u16 + g as u16 + b as u16) / 3;
    if lum < 60 { RColor::Black } else { RColor::White }
}

fn luminance(rgb: (u8, u8, u8)) -> u8 {
    (0.299 * rgb.0 as f64 + 0.587 * rgb.1 as f64 + 0.114 * rgb.2 as f64) as u8
}

fn ascii_shade(lum: u8) -> char {
    const S: &[char] = &[' ', '.', '`', '\'', '-', ':', '+', 'o', '0', '#'];
    S[(lum as usize * (S.len() - 1)) / 255]
}
