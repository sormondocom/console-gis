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
///
/// # Performance notes
///
/// The inner pixel loop uses f32 arithmetic throughout (vs f64 in the original).
/// On x86_64 this doubles SIMD lane width (f32x8 AVX vs f64x4) and reduces
/// transcendental call cost.  Frame-constant values (eye_z, trig for rot_x/rot_y)
/// are hoisted into [`FrameConsts`] and computed once per frame rather than
/// once per pixel.  On x86_64, `normalize` uses `rsqrtss` (~4 cycles vs
/// ~20 for `sqrtss`) via `_mm_rsqrt_ss`.

use crate::data::WorldMap;
use super::canvas::{Canvas, Color, TerminalCapability};

// ── Constants ─────────────────────────────────────────────────────────────────

const AMBIENT: f32 = 0.08;
// Sun direction (normalised): upper-right, slightly toward viewer.
// Raw (0.8, 0.5, −0.3) normalised ≈ (0.808, 0.505, −0.303).
const SUN: (f32, f32, f32) = (0.8081, 0.5051, -0.3030);

// ── Fast reciprocal sqrt ──────────────────────────────────────────────────────

/// Reciprocal sqrt using `rsqrtss` on x86_64.
///
/// SSE1 (`rsqrtss`) is part of the x86_64 baseline ABI — no feature gate needed.
/// Relative error < 1.5×10⁻³, more than sufficient for terminal-pixel precision.
#[inline(always)]
fn recip_sqrt(x: f32) -> f32 {
    #[cfg(target_arch = "x86_64")]
    {
        // Safety: SSE1 is guaranteed on every x86_64 target.
        unsafe {
            use std::arch::x86_64::{_mm_cvtss_f32, _mm_rsqrt_ss, _mm_set_ss};
            _mm_cvtss_f32(_mm_rsqrt_ss(_mm_set_ss(x)))
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        1.0 / x.sqrt()
    }
}

// ── f32 math helpers (hot-path pixel loop) ────────────────────────────────────

#[inline(always)]
fn normalize(x: f32, y: f32, z: f32) -> (f32, f32, f32) {
    let rn = recip_sqrt(x * x + y * y + z * z);
    (x * rn, y * rn, z * rn)
}

#[inline(always)]
fn dot(a: (f32, f32, f32), b: (f32, f32, f32)) -> f32 {
    a.0 * b.0 + a.1 * b.1 + a.2 * b.2
}

// ── Ray–sphere intersection (f32) ────────────────────────────────────────────

const BASE_EYE_Z: f32 = 2.5; // distance of eye from sphere centre

/// Intersect a ray from (0,0,−eye_z) through NDC point (ndx,ndy,0) with the
/// unit sphere.  Returns the nearest hit point, or `None` for misses.
#[inline(always)]
fn intersect(ndx: f32, ndy: f32, eye_z: f32) -> Option<(f32, f32, f32)> {
    // Ray direction toward pixel, eye at (0, 0, -eye_z).
    let (dx, dy, dz) = normalize(ndx, ndy, eye_z);
    // b = 2·(e·d) with e=(0,0,-eye_z)  →  -2·eye_z·dz
    let b    = -2.0 * eye_z * dz;
    let c    = eye_z * eye_z - 1.0;
    let disc = b * b - 4.0 * c;
    if disc < 0.0 { return None; }
    let t = (-b - disc.sqrt()) * 0.5;
    if t <= 0.0 { return None; }
    Some((t * dx, t * dy, -eye_z + t * dz))
}

// ── f32 rotation with precomputed cos/sin (hot path) ─────────────────────────

/// Rotate around Y-axis given precomputed (cos a, sin a).
#[inline(always)]
fn rot_y_cs(x: f32, y: f32, z: f32, c: f32, s: f32) -> (f32, f32, f32) {
    (x * c + z * s, y, -x * s + z * c)
}

/// Rotate around X-axis given precomputed (cos a, sin a).
#[inline(always)]
fn rot_x_cs(x: f32, y: f32, z: f32, c: f32, s: f32) -> (f32, f32, f32) {
    (x, y * c - z * s, y * s + z * c)
}

// ── f64 rotation helpers (project_latlon only — not in hot path) ──────────────

#[inline]
fn rot_y_f64(x: f64, y: f64, z: f64, a: f64) -> (f64, f64, f64) {
    let (c, s) = (a.cos(), a.sin());
    (x * c + z * s, y, -x * s + z * c)
}

#[inline]
fn rot_x_f64(x: f64, y: f64, z: f64, a: f64) -> (f64, f64, f64) {
    let (c, s) = (a.cos(), a.sin());
    (x, y * c - z * s, y * s + z * c)
}

// ── Sphere → geographic coords ────────────────────────────────────────────────

/// f32 hot-path variant — returns degrees as f64 for downstream `classify`.
#[inline(always)]
fn to_latlon(x: f32, y: f32, z: f32) -> (f64, f64) {
    let lat = f64::from(y.clamp(-1.0, 1.0).asin()).to_degrees();
    let lon = f64::from(x.atan2(-z)).to_degrees();
    (lat, lon)
}

// ── Surface classification ────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum Surface {
    SpecialLatitude, // equator, tropics, polar circles — gold
    Graticule,       // 15° grid — cyan
    Land(u8),        // continental land — elevation tier 0–3
    Ocean,           // deep ocean — blue
}

const SPECIAL_LATS: &[f64] = &[0.0, 23.5, -23.5, 66.5, -66.5];
const GRID_DEG: f64 = 15.0;
// Tolerance in degrees for drawing graticule lines.
// Smaller = thinner, crisper grid lines.  1° ≈ 1 CPE at zoom-0 globe scale,
// so 0.4° keeps lines to a single pixel at most zoom levels.
const LAT_TOL: f64 = 0.4;
const LON_TOL: f64 = 0.4;

fn classify(lat: f64, lon: f64, world: &WorldMap, topo: &crate::data::TopoMap, topo_enabled: bool) -> Surface {
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
    if world.is_land(lat, lon) {
        let tier = if topo_enabled { topo.elevation_tier(lat, lon) } else { 0 };
        Surface::Land(tier)
    } else {
        Surface::Ocean
    }
}

fn surface_color(surface: Surface, light: f64) -> Color {
    let base = match surface {
        Surface::SpecialLatitude  => Color::GOLD,
        Surface::Graticule        => Color::GRID,
        Surface::Land(0)          => Color::LAND,
        Surface::Land(1)          => Color::new(90, 130, 50),
        Surface::Land(2)          => Color::new(120, 100, 70),
        Surface::Land(_)          => Color::new(200, 205, 220),
        Surface::Ocean            => Color::OCEAN,
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

// ── Frame-constant values ─────────────────────────────────────────────────────

/// Per-frame invariants pre-computed once before the pixel loop.
///
/// Constructing this once eliminates 4 transcendental calls (cos/sin for each
/// rotation axis) from the inner loop.  Pass a reference to
/// [`pixel_color_pub_fc`] instead of using the per-pixel [`pixel_color_pub`].
pub struct FrameConsts {
    /// Effective eye distance along −Z (= BASE_EYE_Z / zoom).
    pub eye_z: f32,
    /// cos(rot_x) — used for the inverse X-rotation.
    pub irx_c: f32,
    /// −sin(rot_x) — sine of the inverse angle (sin(−a) = −sin a).
    pub irx_s: f32,
    /// cos(rot_y) — used for the inverse Y-rotation.
    pub iry_c: f32,
    /// −sin(rot_y).
    pub iry_s: f32,
    /// Pixel-space horizontal centre.
    pub cx:    f32,
    /// Pixel-space vertical centre.
    pub cy:    f32,
    /// Pixels per unit-sphere radius.
    pub scale: f32,
}

impl FrameConsts {
    /// Build frame constants from [`GlobeParams`] and viewport geometry.
    pub fn new(params: &GlobeParams, cx: f64, cy: f64, scale: f64) -> Self {
        let eye_z = (BASE_EYE_Z as f64 / params.zoom.clamp(0.5, 4.0)) as f32;
        let rx    = params.rot_x as f32;
        let ry    = params.rot_y as f32;
        Self {
            eye_z,
            irx_c:  rx.cos(),
            irx_s: -rx.sin(),
            iry_c:  ry.cos(),
            iry_s: -ry.sin(),
            cx:    cx    as f32,
            cy:    cy    as f32,
            scale: scale as f32,
        }
    }
}

// ── Inner pixel colour (hot path) ─────────────────────────────────────────────

#[inline(always)]
fn pixel_color_inner(
    col:          usize,
    prow:         usize,
    fc:           &FrameConsts,
    world:        &WorldMap,
    topo:         &crate::data::TopoMap,
    topo_enabled: bool,
) -> Color {
    let ndx =  (col  as f32 - fc.cx) / fc.scale;
    let ndy = -(prow as f32 - fc.cy) / fc.scale;

    match intersect(ndx, ndy, fc.eye_z) {
        None => {
            let lum = star_luminance(col as u32, prow as u32);
            if lum > 0 { Color::new(lum, lum, lum) } else { Color::BG }
        }
        Some((hx, hy, hz)) => {
            // Lighting in view space (before rotation).
            let light = f64::from(AMBIENT + (1.0 - AMBIENT) * dot((hx, hy, hz), SUN).max(0.0));

            // Inverse rotation: rot_x⁻¹ then rot_y⁻¹ (precomputed cos/sin).
            let (wx, wy, wz) = rot_x_cs(hx, hy, hz, fc.irx_c, fc.irx_s);
            let (wx, wy, wz) = rot_y_cs(wx, wy, wz, fc.iry_c, fc.iry_s);

            let (lat, lon) = to_latlon(wx, wy, wz);
            surface_color(classify(lat, lon, world, topo, topo_enabled), light)
        }
    }
}

// ── Public pixel APIs ─────────────────────────────────────────────────────────

/// Render a single pixel given pre-computed [`FrameConsts`].
///
/// Preferred for inner loops — avoids per-pixel transcendental calls.
pub fn pixel_color_pub_fc(
    col:          usize,
    prow:         usize,
    fc:           &FrameConsts,
    world:        &WorldMap,
    topo:         &crate::data::TopoMap,
    topo_enabled: bool,
) -> (u8, u8, u8) {
    let c = pixel_color_inner(col, prow, fc, world, topo, topo_enabled);
    (c.r, c.g, c.b)
}

/// Single-pixel API that builds [`FrameConsts`] internally on every call.
///
/// Preserved for external callers and tests.  For inner loops prefer
/// constructing a [`FrameConsts`] once and calling [`pixel_color_pub_fc`].
pub fn pixel_color_pub(
    col: usize, prow: usize,
    cx: f64, cy: f64, scale: f64,
    params: &GlobeParams,
    world: &WorldMap,
    topo: &crate::data::TopoMap,
    topo_enabled: bool,
) -> (u8, u8, u8) {
    let fc = FrameConsts::new(params, cx, cy, scale);
    let c  = pixel_color_inner(col, prow, &fc, world, topo, topo_enabled);
    (c.r, c.g, c.b)
}

/// Color-returning variant (used by `render_frame` and tests).
pub fn pixel_color(
    col: usize,
    prow: usize,
    cx: f64,
    cy: f64,
    scale: f64,
    params: &GlobeParams,
    world: &WorldMap,
    topo: &crate::data::TopoMap,
    topo_enabled: bool,
) -> Color {
    let fc = FrameConsts::new(params, cx, cy, scale);
    pixel_color_inner(col, prow, &fc, world, topo, topo_enabled)
}

// ── Frame renderer ────────────────────────────────────────────────────────────

/// Render one globe frame into a [`Canvas`].
pub fn render_frame(
    canvas: &mut Canvas,
    params: &GlobeParams,
    world: &WorldMap,
    topo: &crate::data::TopoMap,
    topo_enabled: bool,
) {
    let pw = canvas.pixel_width;
    let ph = canvas.pixel_height;
    let cx = pw as f64 / 2.0;
    let cy = ph as f64 / 2.0;
    let scale = cx.min(cy) * 0.95;

    // Pre-compute frame constants once — eliminates 4 trig calls per pixel.
    let fc = FrameConsts::new(params, cx, cy, scale);

    for py in 0..ph {
        for px in 0..pw {
            let c = pixel_color_inner(px, py, &fc, world, topo, topo_enabled);
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
    let (x, y, z) = rot_y_f64(x, y, z, params.rot_y);
    let (x, y, z) = rot_x_f64(x, y, z, params.rot_x);

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
    topo: &crate::data::TopoMap,
    topo_enabled: bool,
    capability: TerminalCapability,
) -> Vec<String> {
    let mut canvas = Canvas::new(cols, rows, capability);
    render_frame(&mut canvas, params, world, topo, topo_enabled);
    canvas.render_rows()
}
