/// Flat Mercator world-map view.
///
/// Web Mercator projection identical to tile-based GIS systems.
/// Zoom levels 0–20 share the same ground resolution as the globe view.
///
/// Controls:
///   ←/→ / A/D  : pan west / east
///   ↑/↓        : pan north / south
///   W / +       : zoom in
///   S / −       : zoom out
///   M           : place marker at crosshair
///   Esc / Q     : return to menu

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color as RColor, Modifier, Style},
    widgets::Widget,
};
use crate::render::canvas::TerminalCapability;
use crate::data::{WorldMap, Marker};
use crate::geo::zoom::ConsoleResolution;
use crate::tui::app::LayerEntry;

// ── Constants ────────────────────────────────────────────────────────────────

const R: f64           = 6_378_137.0;
const M_PER_DEG: f64   = 2.0 * PI * R / 360.0;

const OCEAN:   (u8, u8, u8) = (10,  30,  80);
const LAND:    (u8, u8, u8) = (34,  85,  34);
const GRID:    (u8, u8, u8) = (30,  55,  90);
const EQUATOR: (u8, u8, u8) = (200, 170,   0);
const TROPIC:  (u8, u8, u8) = (180, 100,  20);
const POLAR:   (u8, u8, u8) = ( 80, 130, 160);

// ── Pixel classifier ──────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum PixelKind { Ocean, Land(u8), Grid, Equator, Tropic, Polar }

/// Forward Mercator: (lat_deg, lon_deg) → (merc_x_m, merc_y_m).
#[inline]
fn merc(lat_deg: f64, lon_deg: f64) -> (f64, f64) {
    let lat_r = lat_deg.to_radians().clamp(-1.484_406, 1.484_406);
    (R * lon_deg.to_radians(), R * (FRAC_PI_4 + lat_r / 2.0).tan().ln())
}

/// Inverse Mercator: (merc_x_m, merc_y_m) → (lat_deg, lon_deg ∈ [−180, 180]).
#[inline]
fn merc_inv(mx: f64, my: f64) -> (f64, f64) {
    let lat = if my.abs() > R * 3.5 {
        my.signum() * 90.0
    } else {
        (2.0 * (my / R).exp().atan() - FRAC_PI_2).to_degrees()
    };
    let lon = (mx / R).to_degrees().rem_euclid(360.0) - 180.0;
    (lat, lon)
}

/// Graticule line spacing (degrees) for the given zoom level.
fn grid_interval(zoom: u8) -> f64 {
    match zoom {
        0..=1  => 30.0,
        2..=4  => 15.0,
        5..=7  =>  5.0,
        8..=10 =>  1.0,
        _      =>  0.25,
    }
}

/// True if `v` is within `thresh` of any multiple of `interval`.
#[inline]
fn on_grid(v: f64, interval: f64, thresh: f64) -> bool {
    let r = v.rem_euclid(interval);
    r < thresh || r > interval - thresh
}

/// Classify one CPE at half-block pixel position `(px, py)`.
fn classify(
    px: f64, py: f64,
    cx_px: f64, cy_px: f64,
    mpp: f64, mpp_y: f64,
    cmerc_x: f64, cmerc_y: f64,
    grid_deg: f64, thresh: f64,
    world: &WorldMap,
    topo: &crate::data::TopoMap,
    topo_enabled: bool,
) -> PixelKind {
    let (lat, lon) = merc_inv(
        cmerc_x + (px - cx_px) * mpp,
        cmerc_y - (py - cy_px) * mpp_y,   // screen y↓ = geo y↑
    );

    // Mercator-corrected latitude threshold: at latitude φ, 1° of lat spans
    // 1/cos(φ) more screen pixels than at the equator, so the threshold in
    // degrees must shrink by cos(φ) to keep lines one pixel wide.
    let lat_thresh = thresh * lat.to_radians().cos().max(0.1);

    // Special parallels (drawn at 2× normal threshold so they're visible)
    if lat.abs() < lat_thresh * 2.0                             { return PixelKind::Equator; }
    if (lat - 23.5).abs() < lat_thresh * 2.0
    || (lat + 23.5).abs() < lat_thresh * 2.0                   { return PixelKind::Tropic; }
    if (lat - 66.5).abs() < lat_thresh * 2.0
    || (lat + 66.5).abs() < lat_thresh * 2.0                   { return PixelKind::Polar; }

    if on_grid(lat, grid_deg, lat_thresh) || on_grid(lon, grid_deg, thresh) {
        return PixelKind::Grid;
    }

    if world.is_land(lat, lon) {
        let tier = if topo_enabled { topo.elevation_tier(lat, lon) } else { 0 };
        PixelKind::Land(tier)
    } else {
        PixelKind::Ocean
    }
}

fn kind_rgb(k: PixelKind) -> (u8, u8, u8) {
    match k {
        PixelKind::Ocean      => OCEAN,
        PixelKind::Land(0)    => LAND,
        PixelKind::Land(1)    => (70, 110, 40),
        PixelKind::Land(2)    => (110, 90, 60),
        PixelKind::Land(_)    => (200, 200, 210),
        PixelKind::Grid       => GRID,
        PixelKind::Equator    => EQUATOR,
        PixelKind::Tropic     => TROPIC,
        PixelKind::Polar      => POLAR,
    }
}

// ── Terminal rendering helpers ─────────────────────────────────────────────────

/// ASCII character for a pixel kind — VT-100 compatible, no colour needed.
fn kind_ascii(k: PixelKind) -> char {
    match k {
        PixelKind::Ocean   => ' ',
        PixelKind::Land(0) => ',',
        PixelKind::Land(1) => ':',
        PixelKind::Land(2) => '^',
        PixelKind::Land(_) => '#',
        PixelKind::Grid    => '.',
        PixelKind::Equator => '=',
        PixelKind::Tropic  => '-',
        PixelKind::Polar   => '~',
    }
}

fn luminance(rgb: (u8, u8, u8)) -> u8 {
    (0.299 * rgb.0 as f64 + 0.587 * rgb.1 as f64 + 0.114 * rgb.2 as f64) as u8
}

fn ascii_shade(lum: u8) -> char {
    const S: &[char] = &[' ', '.', '`', '\'', '-', ':', '+', 'o', '0', '#'];
    S[(lum as usize * (S.len() - 1)) / 255]
}

/// Best-effort ANSI-8 colour approximation for a map RGB value.
fn ansi_color(rgb: (u8, u8, u8)) -> RColor {
    let (r, g, b) = rgb;
    if r > 180 && g > 180 && b > 180 { return RColor::White;  } // snow/ice (tier 3)
    if r > 150 && g > 130 && b < 60  { return RColor::Yellow; } // equator / gold
    if r > 100 && g > 60  && b < 50  { return RColor::Yellow; } // tropics (orange)
    if b < 100 && g > 100 && r < 100 { return RColor::Cyan;   } // polar
    if b > r   && b > g              { return RColor::Blue;   } // ocean
    if g > r   && g > b              { return RColor::Green;  } // land (tiers 0–1)
    if r > g   && r > b              { return RColor::Red;    } // brownish (tier 2)
    let lum = (r as u16 + g as u16 + b as u16) / 3;
    if lum < 40 { RColor::Black } else { RColor::DarkGray }
}

// ── Map widget ────────────────────────────────────────────────────────────────

/// Full-screen flat Mercator world-map widget.
pub struct MapView<'a> {
    pub center_lat:  f64,
    pub center_lon:  f64,
    pub zoom:        u8,
    pub capability:  TerminalCapability,
    pub world:       &'a WorldMap,
    pub topo:        &'a crate::data::TopoMap,
    pub topo_enabled: bool,
    pub markers:     &'a [Marker],
    pub layers:      &'a [LayerEntry],
    pub resolution:  &'a ConsoleResolution,
    /// True when the marker-placement crosshair overlay is active.
    pub placing:     bool,
}

impl<'a> Widget for MapView<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 { return; }

        let cols     = area.width  as usize;
        let rows     = area.height as usize;
        let map_rows = rows.saturating_sub(1).max(1); // bottom row = status bar

        let use_hb = self.capability.supports_half_block();
        // Half-block: 2 pixel rows per terminal row.  ASCII/Block: 1 per row.
        let rows_px: usize = if use_hb { map_rows * 2 } else { map_rows };
        let cx_px = cols as f64 / 2.0;
        let cy_px = rows_px as f64 / 2.0;

        // metres per CPE (horizontal)
        let mpp = self.resolution.metres_per_cpe(self.zoom);
        // In half-block mode CPEs are square.  In ASCII/Block mode the character
        // cell is 2× taller than wide (8×16 px standard font), so vertical
        // coverage doubles.
        let mpp_y = if use_hb { mpp } else { mpp / 0.5 };

        // Mercator coordinates of the viewport centre
        let (cmerc_x, cmerc_y) = merc(self.center_lat, self.center_lon);

        // Graticule line threshold — about half a CPE in degrees
        let grid_deg   = grid_interval(self.zoom);
        let thresh_deg = (mpp / M_PER_DEG).abs() * 0.55;

        // ── Render map cells ──────────────────────────────────────────────────
        for row in 0..map_rows {
            for col in 0..cols {
                let cx_f = col as f64;
                let cell = buf.get_mut(area.x + col as u16, area.y + row as u16);

                if use_hb {
                    let top = kind_rgb(classify(cx_f, (row * 2) as f64,
                        cx_px, cy_px, mpp, mpp_y, cmerc_x, cmerc_y,
                        grid_deg, thresh_deg, self.world, self.topo, self.topo_enabled));
                    let bot = kind_rgb(classify(cx_f, (row * 2 + 1) as f64,
                        cx_px, cy_px, mpp, mpp_y, cmerc_x, cmerc_y,
                        grid_deg, thresh_deg, self.world, self.topo, self.topo_enabled));

                    match self.capability {
                        TerminalCapability::TrueColor => {
                            cell.set_char('▀');
                            cell.set_fg(RColor::Rgb(top.0, top.1, top.2));
                            cell.set_bg(RColor::Rgb(bot.0, bot.1, bot.2));
                        }
                        TerminalCapability::Color256 | TerminalCapability::Ansi8 => {
                            cell.set_char('▀');
                            cell.set_fg(ansi_color(top));
                            cell.set_bg(ansi_color(bot));
                        }
                        _ => {
                            // Legacy terminal: no half-block colour, ASCII shade
                            cell.set_char(ascii_shade(luminance(top)));
                        }
                    }
                } else {
                    // ASCII / Block / VT-100: one character per cell
                    let k = classify(cx_f, row as f64,
                        cx_px, cy_px, mpp, mpp_y, cmerc_x, cmerc_y,
                        grid_deg, thresh_deg, self.world, self.topo, self.topo_enabled);

                    cell.set_char(kind_ascii(k));
                    if matches!(self.capability,
                        TerminalCapability::Ansi8 | TerminalCapability::Color256)
                    {
                        cell.set_fg(ansi_color(kind_rgb(k)));
                    }
                }
            }
        }

        // ── Marker overlay ────────────────────────────────────────────────────
        for marker in self.markers {
            if let Some((mc, mr)) = latlon_to_screen(
                marker.lat, marker.lon,
                cmerc_x, cmerc_y, cx_px, cy_px,
                mpp, mpp_y, cols, map_rows, use_hb,
            ) {
                let col = area.x + mc as u16;
                let row = area.y + mr as u16;
                let sym = if self.capability.supports_unicode() {
                    marker.symbol.chars().next().unwrap_or('*')
                } else {
                    marker.ascii_symbol()
                };

                let mc_cell = buf.get_mut(col, row);
                mc_cell.set_char(sym);
                mc_cell.set_fg(if self.capability.supports_true_colour() {
                    RColor::Rgb(255, 220, 0)
                } else {
                    RColor::Yellow
                });
                let mut mods = Modifier::BOLD;
                if marker.blink { mods |= Modifier::SLOW_BLINK; }
                mc_cell.set_style(mc_cell.style().add_modifier(mods));

                for (i, ch) in marker.label.chars().enumerate() {
                    let lc = col + 1 + i as u16;
                    if lc >= area.x + area.width { break; }
                    let lf = buf.get_mut(lc, row);
                    lf.set_char(ch);
                    lf.set_fg(if self.capability.supports_true_colour() {
                        RColor::Rgb(200, 200, 200)
                    } else {
                        RColor::White
                    });
                }
            }
        }

        // ── GeoJSON layer rendering ───────────────────────────────────────────
        //
        // Cycle through a small palette so multiple layers are visually distinct.
        const LAYER_COLORS_TC: &[(u8, u8, u8)] = &[
            (0, 220, 220),   // cyan
            (220, 180, 0),   // gold
            (180, 80, 220),  // violet
            (80, 220, 80),   // lime
            (220, 80, 80),   // coral
        ];
        const LAYER_COLORS_ANSI: &[RColor] = &[
            RColor::Cyan, RColor::Yellow, RColor::Magenta,
            RColor::Green, RColor::Red,
        ];

        for entry in self.layers.iter().filter(|e| e.visible) {
            let tc_col  = LAYER_COLORS_TC[entry.color_index as usize % LAYER_COLORS_TC.len()];
            let a8_col  = LAYER_COLORS_ANSI[entry.color_index as usize % LAYER_COLORS_ANSI.len()];

            let seg_fg = match self.capability {
                TerminalCapability::TrueColor =>
                    RColor::Rgb(tc_col.0, tc_col.1, tc_col.2),
                TerminalCapability::Color256 | TerminalCapability::Ansi8 => a8_col,
                _ => RColor::Reset,
            };
            let seg_style = Style::default().fg(seg_fg);

            // Line segments (LineString, MultiLineString, Polygon boundaries)
            for ((lon0, lat0), (lon1, lat1)) in entry.layer.segments() {
                if let (Some((c0, r0)), Some((c1, r1))) = (
                    latlon_to_screen(lat0, lon0, cmerc_x, cmerc_y,
                        cx_px, cy_px, mpp, mpp_y, cols, map_rows, use_hb),
                    latlon_to_screen(lat1, lon1, cmerc_x, cmerc_y,
                        cx_px, cy_px, mpp, mpp_y, cols, map_rows, use_hb),
                ) {
                    draw_line(
                        buf, area,
                        c0 as i32, r0 as i32,
                        c1 as i32, r1 as i32,
                        if self.capability.supports_unicode() { '·' } else { '.' },
                        seg_style,
                    );
                }
            }

            // Points
            let pt_fg = if self.capability.supports_true_colour() {
                RColor::Rgb(255, 220, 0)
            } else {
                RColor::Yellow
            };
            for (lon, lat) in entry.layer.all_point_coords() {
                if let Some((sc, sr)) = latlon_to_screen(lat, lon, cmerc_x, cmerc_y,
                    cx_px, cy_px, mpp, mpp_y, cols, map_rows, use_hb)
                {
                    let cell = buf.get_mut(area.x + sc as u16, area.y + sr as u16);
                    cell.set_char(if self.capability.supports_unicode() { '◆' } else { '*' });
                    cell.set_fg(pt_fg);
                    cell.set_style(cell.style().add_modifier(Modifier::BOLD));
                }
            }
        }

        // ── Crosshair (marker-placement mode) ─────────────────────────────────
        if self.placing {
            let cc = area.x + area.width  / 2;
            let cr = area.y + map_rows as u16 / 2;
            buf.get_mut(cc, cr)
                .set_char('+')
                .set_fg(RColor::Green)
                .set_style(Style::default().add_modifier(Modifier::BOLD));
        }

        // ── Status bar ────────────────────────────────────────────────────────
        let place_hint = if self.placing {
            "  [Enter] mark · [Esc] cancel"
        } else {
            "  [M] mark · [I] import · [W/S] zoom · [↑↓←→] pan"
        };
        let visible_count = self.layers.iter().filter(|e| e.visible).count();
        let layer_info = if self.layers.is_empty() {
            String::new()
        } else {
            format!("  │  {}/{} layer{}", visible_count, self.layers.len(),
                if self.layers.len() == 1 { "" } else { "s" })
        };
        let mpp     = self.resolution.metres_per_cpe(self.zoom);
        let res_str = fmt_resolution(mpp);
        let status = format!(
            " console-gis  │  WGS-84 → Mercator  │  {:.2}°N {:.2}°E  │  z{}  │  {}{}  │  Esc menu{}",
            self.center_lat,
            self.center_lon,
            self.zoom,
            res_str,
            layer_info,
            place_hint,
        );

        let sr = area.y + map_rows as u16;
        let sstyle = if self.capability.supports_true_colour() {
            Style::default()
                .fg(RColor::Rgb(80, 80, 80))
                .bg(RColor::Rgb(6, 6, 15))
        } else {
            Style::default().fg(RColor::DarkGray)
        };
        for (i, ch) in status.chars().enumerate() {
            let sc = area.x + i as u16;
            if sc >= area.x + area.width { break; }
            buf.get_mut(sc, sr).set_char(ch).set_style(sstyle);
        }
    }
}

// ── Resolution formatter ──────────────────────────────────────────────────────

/// Format metres-per-CPE into a compact string for status bars.
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

// ── Projection helpers (public for main.rs key-handling) ─────────────────────

/// Project a geographic coordinate to a screen (col, row).
/// Returns `None` if outside the visible area.
fn latlon_to_screen(
    lat: f64, lon: f64,
    cmerc_x: f64, cmerc_y: f64,
    cx_px: f64, cy_px: f64,
    mpp: f64, mpp_y: f64,
    cols: usize, map_rows: usize,
    use_hb: bool,
) -> Option<(usize, usize)> {
    let (mx, my) = merc(lat, lon);
    let px = cx_px + (mx - cmerc_x) / mpp;
    let py = cy_px - (my - cmerc_y) / mpp_y;
    if px < 0.0 || py < 0.0 { return None; }
    let sc = px as usize;
    let sr = if use_hb { py as usize / 2 } else { py as usize };
    if sc < cols && sr < map_rows { Some((sc, sr)) } else { None }
}

// ── Bresenham line drawing ────────────────────────────────────────────────────

/// Draw a line from (x0,y0) to (x1,y1) in buffer space, clipped to `area`.
fn draw_line(
    buf:   &mut Buffer,
    area:  Rect,
    x0: i32, y0: i32,
    x1: i32, y1: i32,
    ch:    char,
    style: Style,
) {
    let dx  =  (x1 - x0).abs();
    let dy  = -(y1 - y0).abs();
    let sx  = if x0 < x1 { 1i32 } else { -1 };
    let sy  = if y0 < y1 { 1i32 } else { -1 };
    let mut err = dx + dy;
    let (mut x, mut y) = (x0, y0);

    let (ax, ay) = (area.x as i32, area.y as i32);
    let (aw, ah) = (area.width as i32, area.height as i32);

    loop {
        if x >= ax && x < ax + aw && y >= ay && y < ay + ah {
            buf.get_mut(x as u16, y as u16).set_char(ch).set_style(style);
        }
        if x == x1 && y == y1 { break; }
        let e2 = 2 * err;
        if e2 >= dy { err += dy; x += sx; }
        if e2 <= dx { err += dx; y += sy; }
    }
}

/// Degrees of longitude to pan per keypress at the given zoom.
pub fn pan_lon_step(zoom: u8) -> f64 {
    match zoom {
        0..=2 => 20.0,
        3..=5 => 10.0,
        6..=8 =>  2.0,
        _     =>  0.5,
    }
}

/// Degrees of latitude to pan per keypress at the given zoom.
pub fn pan_lat_step(zoom: u8) -> f64 { pan_lon_step(zoom) / 2.0 }
