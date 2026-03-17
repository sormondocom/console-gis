/// Console GIS resolution system — mirrors web GIS zoom levels 0–20.
///
/// # Concept
///
/// Web GIS defines ground resolution as:
/// ```text
///   meters_per_pixel(z) = Earth_circumference / (256 × 2^z)
/// ```
/// At zoom 0 the whole world fits in one 256×256 tile (~156 km/px at equator).
/// At zoom 20 resolution is sub-metre (~0.15 m/px).
///
/// Console-GIS adapts this to terminal characters. A "Console Pixel Equivalent"
/// (CPE) is the atomic rendering unit; its physical size depends on the
/// [`RenderMode`]:
///
/// | Mode       | CPE per cell | Notes                                    |
/// |------------|-------------|------------------------------------------|
/// | Block      | 1 × 1       | One char = one geographic sample         |
/// | HalfBlock  | 1 × 2       | `▀`/`▄` — square CPEs at 2:1 char aspect |
/// | Braille    | 2 × 4       | `⠿` — highest density, 8 CPEs per cell  |
/// | Ascii      | 1 × 1       | VT-100 compatible, no Unicode            |
///
/// # Character aspect ratio
///
/// Standard terminal fonts are ~8 px wide × 16 px tall.  A half-block
/// character covers 8 × 8 px → **square CPEs** at the default aspect ratio.
/// If your terminal uses a different font, set `char_aspect` accordingly.
use std::f64::consts::PI;

const EARTH_CIRCUMFERENCE_M: f64 = 2.0 * PI * 6_378_137.0; // WGS-84
const TILE_SIZE_PX: f64 = 256.0; // web Mercator tile size

pub const ZOOM_MIN: u8 = 0;
pub const ZOOM_MAX: u8 = 20;

/// Rendering mode — determines effective pixel density per character cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    /// One char = 1×1 CPE (compatible with most terminals).
    Block,
    /// One char = 1×2 CPEs via `▀`/`▄` half-block Unicode.
    /// CPEs are square when char aspect ratio is 0.5 (default).
    HalfBlock,
    /// One char = 2×4 CPEs via Unicode braille patterns (`⠿`).
    /// Highest density; not supported on all fonts.
    Braille,
    /// ASCII shading only — VT-100 / legacy terminal compatible.
    /// No Unicode, no colour; uses ` .:-+oO0#@` gradient.
    Ascii,
}

impl RenderMode {
    /// Effective CPE dimensions (width, height) per character cell.
    pub const fn cpe_per_cell(self) -> (u32, u32) {
        match self {
            RenderMode::Block     => (1, 1),
            RenderMode::HalfBlock => (1, 2),
            RenderMode::Braille   => (2, 4),
            RenderMode::Ascii     => (1, 1),
        }
    }

    /// True if the mode requires Unicode support.
    pub const fn requires_unicode(self) -> bool {
        matches!(self, RenderMode::HalfBlock | RenderMode::Braille)
    }

    /// True if the mode requires colour support.
    pub const fn requires_colour(self) -> bool {
        matches!(self, RenderMode::HalfBlock | RenderMode::Braille | RenderMode::Block)
    }
}

/// Console GIS resolution — maps zoom levels to terminal geometry.
#[derive(Debug, Clone)]
pub struct ConsoleResolution {
    /// Rendering mode (affects CPE density).
    pub mode: RenderMode,

    /// Character aspect ratio = char_width_px / char_height_px.
    /// Standard terminal: 0.5 (8 px wide, 16 px tall).
    /// Set to 1.0 for square characters.
    pub char_aspect: f64,
}

impl Default for ConsoleResolution {
    fn default() -> Self {
        Self {
            mode: RenderMode::HalfBlock,
            char_aspect: 0.5,
        }
    }
}

impl ConsoleResolution {
    pub fn new(mode: RenderMode) -> Self {
        Self { mode, ..Default::default() }
    }

    /// Ground resolution in **metres per CPE** at the equator for zoom `z`.
    ///
    /// This matches the standard web Mercator formula exactly; the only
    /// difference is that a "pixel" is now a CPE rather than a screen pixel.
    pub fn metres_per_cpe(&self, zoom: u8) -> f64 {
        let zoom = zoom.min(ZOOM_MAX) as u32;
        EARTH_CIRCUMFERENCE_M / (TILE_SIZE_PX * (1u64 << zoom) as f64)
    }

    /// Geographic extent (lon_degrees, lat_degrees) visible in a viewport of
    /// `cols × rows` characters centred at `center_lat` at zoom level `zoom`.
    pub fn viewport_extent_deg(
        &self,
        cols: u16,
        rows: u16,
        zoom: u8,
        center_lat: f64,
    ) -> (f64, f64) {
        let (cpe_w, cpe_h) = self.mode.cpe_per_cell();
        let eff_cols = cols as f64 * cpe_w as f64;
        let eff_rows = rows as f64 * cpe_h as f64;

        let mpp = self.metres_per_cpe(zoom);

        // Adjust for non-square character cells when in Block/Ascii mode.
        // In HalfBlock the CPE is already square, so char_aspect cancels out.
        let (mpp_col, mpp_row) = match self.mode {
            RenderMode::HalfBlock | RenderMode::Braille => (mpp, mpp),
            _ => (mpp / self.char_aspect, mpp * self.char_aspect),
        };

        let width_m  = eff_cols * mpp_col;
        let height_m = eff_rows * mpp_row;

        let m_per_deg_lat = EARTH_CIRCUMFERENCE_M / 360.0;
        let m_per_deg_lon = m_per_deg_lat * center_lat.to_radians().cos().max(1e-9);

        (width_m / m_per_deg_lon, height_m / m_per_deg_lat)
    }

    /// Zoom level (possibly fractional) that fits `lon_extent × lat_extent`
    /// degrees into a `cols × rows` viewport at `center_lat`.
    pub fn zoom_for_extent(
        &self,
        cols: u16,
        rows: u16,
        lon_extent: f64,
        lat_extent: f64,
        center_lat: f64,
    ) -> f64 {
        let (cpe_w, cpe_h) = self.mode.cpe_per_cell();
        let eff_cols = cols as f64 * cpe_w as f64;
        let eff_rows = rows as f64 * cpe_h as f64;

        let m_per_deg_lat = EARTH_CIRCUMFERENCE_M / 360.0;
        let m_per_deg_lon = m_per_deg_lat * center_lat.to_radians().cos().max(1e-9);

        let width_m  = lon_extent * m_per_deg_lon;
        let height_m = lat_extent * m_per_deg_lat;

        let mpp = (width_m / eff_cols).max(height_m / eff_rows);
        (EARTH_CIRCUMFERENCE_M / (TILE_SIZE_PX * mpp)).log2().clamp(0.0, 20.0)
    }

    /// Scale (CPEs per degree of longitude) at the equator for zoom `z`.
    pub fn cpe_per_degree(&self, zoom: u8) -> f64 {
        let m_per_deg = EARTH_CIRCUMFERENCE_M / 360.0;
        m_per_deg / self.metres_per_cpe(zoom)
    }

    /// Human-readable label for a zoom level.
    pub fn zoom_label(zoom: u8) -> &'static str {
        match zoom {
            0  => "World overview",
            1  => "World (2×)",
            2  => "Subcontinental",
            3  => "Largest countries",
            4  => "Large countries",
            5  => "Country / large region",
            6  => "Large metro region",
            7  => "Small metro region",
            8  => "County / district",
            9  => "Wide area",
            10 => "Neighbourhood",
            11 => "City",
            12 => "Town / borough",
            13 => "Village / suburb",
            14 => "Small suburb",
            15 => "Streets",
            16 => "City block",
            17 => "Buildings",
            18 => "Building detail",
            19 => "Room-scale",
            20 => "Sub-metre detail",
            _  => "—",
        }
    }

    /// Minimum zoom level where the whole world fits in `cols` character columns.
    pub fn min_zoom_for_width(&self, cols: u16) -> u8 {
        let z = self.zoom_for_extent(cols, 1, 360.0, 1.0, 0.0);
        z.floor() as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom0_ground_resolution() {
        let res = ConsoleResolution::default();
        let mpp = res.metres_per_cpe(0);
        // Should be ~156,543 m/px at zoom 0
        assert!((mpp - 156_543.0).abs() < 1.0, "zoom 0 mpp={mpp}");
    }

    #[test]
    fn zoom20_ground_resolution() {
        let res = ConsoleResolution::default();
        let mpp = res.metres_per_cpe(20);
        assert!(mpp < 0.2, "zoom 20 mpp={mpp}");
    }

    #[test]
    fn world_fits_at_zoom0_halfblock() {
        let res = ConsoleResolution::new(RenderMode::HalfBlock);
        // At zoom 0 the standard web tile is 256 px wide. With 256 columns
        // (1 CPE per column in HalfBlock mode) the whole world should be visible.
        let (lon_ext, _) = res.viewport_extent_deg(256, 128, 0, 0.0);
        assert!(lon_ext >= 359.0, "lon_ext={lon_ext}");
    }
}
