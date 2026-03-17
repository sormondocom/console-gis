/// Raycast globe renderer with full 3-D rotation, zoom, and world-map land data.
///
/// # Coordinate system
///
/// - Eye at (0, 0, −`eye_dist`); sphere at origin, radius 1.
/// - Y-axis = geographic north; X-axis = east at lon=0°.
/// - Two rotation angles: `rot_y` (longitude spin) and `rot_x` (tilt/latitude tilt).
///
/// # Zoom
///
/// Zoom is implemented as a field-of-view scale: larger `zoom_scale` means the
/// eye moves closer (or equivalently, the sphere appears larger in NDC space).
/// Valid range: [0.5, 4.0] — 1.0 is the default full-globe view.
///
/// At zoom_scale = 2.0 the visible geographic extent is halved in each axis.
///
/// # Half-block pixels
///
/// At 2:1 character aspect ratio (standard terminal), half-block CPEs are
/// square → the globe renders as a perfect circle, not an ellipse.

use crate::data::WorldMap;
use super::canvas::{Canvas, Color, TerminalCapability};

// ── Constants ─────────────────────────────────────────────────────────────────

const AMBIENT:  f64 = 0.08;
// Sun direction (normalised): upper-right, slightly toward viewer.
// Raw (0.8, 0.5, −0.3) normalised ≈ (0.808, 0.505, −0.303).
const SUN: (f64, f64, f64) = (0.8081, 0.5051, -0.3030);

// ── Math helpers ──────────────────────────────────────────────────────────────

#[inline]
fn normalize(x: f64, y: f64, z: f64) -> (f64, f64, f64) {
    let n = (x * x + y * y + z * z).sqrt();
    (x / n, y / n, z / n)
}

#[inline]
fn dot(a: (f64, f64, f64), b: (f64, f64, f64)) -> f64 {
    a.0 * b.0 + a.1 * b.1 + a.2 * b.2
}

// ── Ray–sphere intersection ───────────────────────────────────────────────────

const BASE_EYE_Z: f64 = 2.5; // distance of eye from sphere centre

fn intersect(ndx: f64, ndy: f64, eye_z: f64) -> Option<(f64, f64, f64)> {
    let (ex, ey, ez) = (0.0_f64, 0.0_f64, -eye_z);
    let (dx, dy, dz) = normalize(ndx - ex, ndy - ey, -ez);
    let b = 2.0 * (ex * dx + ey * dy + ez * dz);
    let c = ex * ex + ey * ey + ez * ez - 1.0;
    let disc = b * b - 4.0 * c;
    if disc < 0.0 { return None; }
    let t = (-b - disc.sqrt()) * 0.5;
    if t <= 0.0 { return None; }
    Some((ex + t * dx, ey + t * dy, ez + t * dz))
}

// ── 3-D rotation ──────────────────────────────────────────────────────────────

/// Rotate around Y-axis by `a` radians.
#[inline]
fn rot_y(x: f64, y: f64, z: f64, a: f64) -> (f64, f64, f64) {
    let (c, s) = (a.cos(), a.sin());
    (x * c + z * s, y, -x * s + z * c)
}

/// Rotate around X-axis by `a` radians.
#[inline]
fn rot_x(x: f64, y: f64, z: f64, a: f64) -> (f64, f64, f64) {
    let (c, s) = (a.cos(), a.sin());
    (x, y * c - z * s, y * s + z * c)
}

// ── Sphere → geographic coords ────────────────────────────────────────────────

fn to_latlon(x: f64, y: f64, z: f64) -> (f64, f64) {
    let lat = y.clamp(-1.0, 1.0).asin().to_degrees();
    let lon = x.atan2(-z).to_degrees();
    (lat, lon)
}

// ── Surface classification ────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum Surface {
    SpecialLatitude, // equator, tropics, polar circles — gold
    Graticule,       // 15° grid — cyan
    Land,            // continental land — green
    Ocean,           // deep ocean — blue
}

const SPECIAL_LATS: &[f64] = &[0.0, 23.5, -23.5, 66.5, -66.5];
const GRID_DEG: f64 = 15.0;
// Tolerance in degrees for drawing graticule lines.
// Smaller = thinner, crisper grid lines.  1° ≈ 1 CPE at zoom-0 globe scale,
// so 0.4° keeps lines to a single pixel at most zoom levels.
const LAT_TOL:  f64 = 0.4;
const LON_TOL:  f64 = 0.4;

fn classify(lat: f64, lon: f64, world: &WorldMap) -> Surface {
    // Special parallels first (highest priority, rendered gold)
    for &sl in SPECIAL_LATS {
        if (lat - sl).abs() < LAT_TOL { return Surface::SpecialLatitude; }
    }
    // Graticule grid
    let lat_mod = lat.rem_euclid(GRID_DEG);
    let lon_mod = lon.rem_euclid(GRID_DEG);
    if lat_mod < LAT_TOL || lat_mod > GRID_DEG - LAT_TOL
        || lon_mod < LON_TOL || lon_mod > GRID_DEG - LON_TOL
    {
        return Surface::Graticule;
    }
    // Land / Ocean
    if world.is_land(lat, lon) { Surface::Land } else { Surface::Ocean }
}

fn surface_color(surface: Surface, light: f64) -> Color {
    let base = match surface {
        Surface::SpecialLatitude => Color::GOLD,
        Surface::Graticule       => Color::GRID,
        Surface::Land            => Color::LAND,
        Surface::Ocean           => Color::OCEAN,
    };
    base.shade(light)
}

// ── Star field ────────────────────────────────────────────────────────────────

fn star_luminance(col: u32, prow: u32) -> u8 {
    let h = col.wrapping_mul(0x9E37_79B9) ^ prow.wrapping_mul(0x6C62_272E);
    if h > 0xF200_0000 { (120 + (h & 0x7F)) as u8 } else { 0 }
}

// ── Globe render parameters ───────────────────────────────────────────────────

/// Parameters for a single globe frame.
pub struct GlobeParams {
    /// Y-axis rotation in radians (east-west spin: A/D or ←/→).
    pub rot_y: f64,
    /// X-axis rotation in radians (tilt: ↑/↓).
    pub rot_x: f64,
    /// Zoom scale (1.0 = full globe, 4.0 = zoomed to ~1/4 of the surface).
    pub zoom:  f64,
}

impl Default for GlobeParams {
    fn default() -> Self {
        Self { rot_y: 0.0, rot_x: 0.0, zoom: 1.0 }
    }
}

// ── Single-pixel colour ───────────────────────────────────────────────────────

pub fn pixel_color(
    col: usize,
    prow: usize,
    cx: f64,
    cy: f64,
    scale: f64,
    params: &GlobeParams,
    world: &WorldMap,
) -> Color {
    // Compute effective eye distance: zoom moves eye closer.
    let eye_z = BASE_EYE_Z / params.zoom.clamp(0.5, 4.0);

    let ndx = (col  as f64 - cx) / scale;
    let ndy = -(prow as f64 - cy) / scale;

    match intersect(ndx, ndy, eye_z) {
        None => {
            let lum = star_luminance(col as u32, prow as u32);
            if lum > 0 { Color::new(lum, lum, lum) } else { Color::BG }
        }
        Some((hx, hy, hz)) => {
            // Lighting uses view-space normal (before rotation).
            let light = AMBIENT + (1.0 - AMBIENT) * dot((hx, hy, hz), SUN).max(0.0);

            // Un-rotate to world space: apply inverse of (rot_x then rot_y).
            let (wx, wy, wz) = rot_x(hx, hy, hz, -params.rot_x);
            let (wx, wy, wz) = rot_y(wx, wy, wz, -params.rot_y);

            let (lat, lon) = to_latlon(wx, wy, wz);
            surface_color(classify(lat, lon, world), light)
        }
    }
}

/// Public re-export for TUI views that write directly into ratatui buffers.
pub fn pixel_color_pub(
    col: usize, prow: usize,
    cx: f64, cy: f64, scale: f64,
    params: &GlobeParams,
    world: &WorldMap,
) -> (u8, u8, u8) {
    let c = pixel_color(col, prow, cx, cy, scale, params, world);
    (c.r, c.g, c.b)
}

// ── Frame renderer ────────────────────────────────────────────────────────────

/// Render one globe frame into a [`Canvas`].
pub fn render_frame(canvas: &mut Canvas, params: &GlobeParams, world: &WorldMap) {
    let pw = canvas.pixel_width;
    let ph = canvas.pixel_height;
    let cx = pw as f64 / 2.0;
    let cy = ph as f64 / 2.0;
    let scale = cx.min(cy) * 0.95;

    for py in 0..ph {
        for px in 0..pw {
            let c = pixel_color(px, py, cx, cy, scale, params, world);
            canvas.set_pixel(px, py, c);
        }
    }
}

/// Project a geographic (lat_deg, lon_deg) to a screen (col, row) in the
/// half-block pixel grid.  Returns `None` if the point is on the back
/// hemisphere (not visible).
///
/// `cx`, `cy` — pixel-space centre; `scale` — pixels per unit-sphere radius.
pub fn project_latlon(
    lat_deg: f64, lon_deg: f64,
    params: &GlobeParams,
    cx: f64, cy: f64, scale: f64,
) -> Option<(i32, i32)> {
    let lat_r = lat_deg.to_radians();
    let lon_r = lon_deg.to_radians();

    // Geographic → unit sphere (same convention as to_latlon's inverse)
    let x = lat_r.cos() * lon_r.sin();
    let y = lat_r.sin();
    let z = -(lat_r.cos() * lon_r.cos());

    // Apply globe rotation (forward: rot_y then rot_x)
    let (x, y, z) = rot_y(x, y, z, params.rot_y);
    let (x, y, z) = rot_x(x, y, z, params.rot_x);

    // Back hemisphere — not visible
    if z >= 0.0 { return None; }

    let sc = (x  * scale + cx) as i32;
    let sr = (-y * scale + cy) as i32;
    Some((sc, sr))
}

/// High-level render → `Vec<String>` (one string per terminal row).
pub fn render(
    cols: usize,
    rows: usize,
    params: &GlobeParams,
    world: &WorldMap,
    capability: TerminalCapability,
) -> Vec<String> {
    let mut canvas = Canvas::new(cols, rows, capability);
    render_frame(&mut canvas, params, world);
    canvas.render_rows()
}
