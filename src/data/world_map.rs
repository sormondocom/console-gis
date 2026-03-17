/// Simplified world map — continent polygons encoded as (lat, lon) vertices.
///
/// # Design goals
///
/// - **Zero external dependencies** — all data is hardcoded.
/// - **VT-100 compatible** — the query interface is just `is_land(lat, lon) -> bool`,
///   with no heap allocation after construction.
/// - **Console-resolution accuracy** — polygons have ~15–35 vertices each,
///   matching the granularity visible at zoom levels 0–5 in the console.
/// - **Fast** — bounding-box pre-filter + simple ray-cast polygon test.
///
/// # Point-in-polygon algorithm
///
/// Uses the even-odd rule (horizontal ray casting):  cast a ray from (lat, lon)
/// toward +∞ longitude and count how many polygon edges it crosses.  An odd
/// count means the point is inside.
///
/// Date-line safety: all polygons are split so no edge spans more than 180° of
/// longitude.  Russia / Chukotka is split into two polygons.
///
/// # Coverage
///
/// Included: North America, Greenland, South America, Europe, Africa,
/// Asia (mainland + Indian subcontinent), SE Asia, Australia + Tasmania,
/// Antarctica (handled analytically — everything south of 65°S).
///
/// Excluded (too small to resolve at console zoom 0–5): Iceland (separate
/// polygon included), Japan, Philippines, Indonesia/Sumatra/Borneo are
/// approximated by the SE Asia polygon.

// ── Bounding box for early rejection ─────────────────────────────────────────

#[derive(Clone, Copy)]
struct BBox {
    lat_min: f32, lat_max: f32,
    lon_min: f32, lon_max: f32,
}

impl BBox {
    const fn new(lat_min: f32, lat_max: f32, lon_min: f32, lon_max: f32) -> Self {
        Self { lat_min, lat_max, lon_min, lon_max }
    }

    fn contains(&self, lat: f32, lon: f32) -> bool {
        lat >= self.lat_min && lat <= self.lat_max
            && lon >= self.lon_min && lon <= self.lon_max
    }
}

// ── Continent polygon data ────────────────────────────────────────────────────
//
// All polygons go counter-clockwise (not required by the algorithm but aids
// readability).  Format: (latitude, longitude) in degrees.

/// North America — outer coast, Alaska, Canada, USA, Mexico, Central America.
const NORTH_AMERICA: &[(f32, f32)] = &[
    // Arctic coast (west to east)
    (72.0, -158.0), (72.0, -140.0), (70.0, -128.0),
    (72.0, -110.0), (74.0, -100.0), (72.0, -84.0),
    (80.0, -87.0),  (83.0, -72.0),  // Ellesmere peak
    // Eastern Canada / Atlantic coast
    (78.0, -64.0),  (70.0, -64.0),  (66.0, -62.0),
    (60.0, -65.0),  (52.0, -55.0),  (47.0, -53.0),
    (44.0, -66.0),
    // US East coast
    (36.0, -76.0),  (25.0, -80.0),
    // Gulf of Mexico
    (25.0, -89.0),  (22.0, -90.0),  (20.0, -87.0),
    (16.0, -88.0),  (15.0, -89.0),
    // Central America / Panama
    (8.0, -77.0),
    // Pacific coast (south to north)
    (8.0, -80.0),   (10.0, -85.0),  (14.0, -92.0),
    (16.0, -98.0),  (20.0, -105.0), (22.0, -106.0),
    (24.0, -110.0), (30.0, -116.0), (32.0, -117.0),
    (38.0, -123.0), (48.0, -124.0), (54.0, -130.0),
    (56.0, -132.0), (58.0, -136.0), (60.0, -146.0),
    (58.0, -153.0), (57.0, -157.0), (56.0, -160.0),
    (57.0, -162.0), (62.0, -165.0), (64.0, -164.0),
    (66.0, -163.0), (70.0, -162.0), (71.0, -157.0),
    (72.0, -158.0), // back to start
];
const NORTH_AMERICA_BBOX: BBox = BBox::new(7.0, 84.0, -168.0, -52.0);

/// Greenland — separate from North America.
const GREENLAND: &[(f32, f32)] = &[
    (83.0, -32.0),  (82.0, -22.0),  (76.0, -18.0),
    (70.0, -22.0),  (65.0, -38.0),  (60.0, -44.0),
    (64.0, -52.0),  (68.0, -54.0),  (72.0, -56.0),
    (76.0, -66.0),  (80.0, -60.0),  (83.0, -40.0),
    (83.0, -32.0),
];
const GREENLAND_BBOX: BBox = BBox::new(59.0, 84.0, -74.0, -16.0);

/// South America — from Colombia/Venezuela to Cape Horn.
const SOUTH_AMERICA: &[(f32, f32)] = &[
    (12.0, -72.0),  (10.0, -62.0),  (8.0, -60.0),
    (6.0, -58.0),   (4.0, -52.0),   (2.0, -50.0),
    (0.0, -50.0),   (-4.0, -36.0),  (-8.0, -35.0),
    (-16.0, -39.0), (-24.0, -43.0), (-34.0, -53.0),
    (-40.0, -62.0), (-52.0, -68.0), (-56.0, -68.0),
    (-52.0, -74.0), (-44.0, -75.0), (-36.0, -72.0),
    (-18.0, -70.0), (-10.0, -78.0), (0.0, -80.0),
    (8.0, -77.0),   (12.0, -72.0),
];
const SOUTH_AMERICA_BBOX: BBox = BBox::new(-57.0, 13.0, -82.0, -34.0);

/// Europe — mainland coast from Norway to Turkey, including Iberian and
/// Italian peninsulas at console resolution.
const EUROPE: &[(f32, f32)] = &[
    (71.0, 28.0),   (66.0, 14.0),   (62.0, 5.0),
    (58.0, 5.0),    (56.0, 8.0),    (57.0, -3.0),
    (54.0, -10.0),  (51.0, -10.0),  (51.0, 2.0),
    (46.0, -2.0),   (44.0, -9.0),   (36.0, -9.0),
    (36.0, -5.0),   (38.0, 0.0),    (40.0, 3.0),
    (38.0, 16.0),   (37.0, 24.0),   (37.0, 36.0),
    (42.0, 36.0),   (42.0, 28.0),   (44.0, 30.0),
    (48.0, 38.0),   (54.0, 22.0),   (60.0, 26.0),
    (66.0, 24.0),   (71.0, 28.0),
];
const EUROPE_BBOX: BBox = BBox::new(35.0, 72.0, -11.0, 42.0);

/// Africa — from Morocco south to Cape of Good Hope.
const AFRICA: &[(f32, f32)] = &[
    (37.0, -6.0),   (37.0, 10.0),   (32.0, 32.0),
    (22.0, 38.0),   (12.0, 44.0),   (2.0, 42.0),
    (-4.0, 40.0),   (-12.0, 40.0),  (-18.0, 36.0),
    (-26.0, 34.0),  (-34.0, 26.0),  (-35.0, 18.0),
    (-30.0, 17.0),  (-18.0, 12.0),  (-8.0, 12.0),
    (-6.0, 10.0),   (4.0, 6.0),     (5.0, -2.0),
    (5.0, -8.0),    (10.0, -17.0),  (22.0, -17.0),
    (30.0, -10.0),  (37.0, -6.0),
];
const AFRICA_BBOX: BBox = BBox::new(-36.0, 38.0, -18.0, 52.0);

/// Asia — mainland from Turkey to the Pacific.  Includes the Middle East,
/// Central Asia, Siberia, and a simplified Indian subcontinent outline.
/// Split at ~40°E to join with Europe via the Ural/Caspian line.
const ASIA_MAINLAND: &[(f32, f32)] = &[
    // Start at Turkey / Bosphorus
    (41.0, 29.0),
    // Turkey S coast
    (37.0, 36.0),   (37.0, 42.0),
    // Middle East / Red Sea
    (30.0, 36.0),   (22.0, 38.0),
    // Arabian Peninsula E coast
    (12.0, 44.0),   (12.0, 45.0),   (22.0, 59.0),
    (26.0, 57.0),   (24.0, 63.0),
    // Pakistan / India W coast
    (22.0, 68.0),   (8.0, 77.0),
    // India S tip
    (8.0, 77.0),    (8.0, 80.0),
    // India E coast
    (15.0, 80.0),   (22.0, 90.0),   (22.0, 92.0),
    // Myanmar / Thailand S
    (16.0, 98.0),   (4.0, 100.0),   (1.0, 104.0),
    (4.0, 109.0),   (6.0, 116.0),
    // SE Asia coast / S China Sea
    (14.0, 108.0),  (18.0, 107.0),  (22.0, 114.0),
    (24.0, 120.0),  (28.0, 122.0),
    // E China / Korea
    (36.0, 122.0),  (40.0, 122.0),  (40.0, 129.0),
    (36.0, 130.0),  (34.0, 130.0),
    // Manchuria / Far East Russia
    (43.0, 131.0),  (46.0, 135.0),  (52.0, 141.0),
    (55.0, 140.0),  (60.0, 151.0),  (62.0, 163.0),
    // Chukotka N coast
    (66.0, 172.0),  (70.0, 180.0),
    // N Russia Arctic coast (west)
    (72.0, 168.0),  (70.0, 142.0),  (74.0, 130.0),
    (76.0, 104.0),  (78.0, 96.0),   (75.0, 76.0),
    (70.0, 58.0),   (66.0, 52.0),
    // Ural / W Siberia W boundary
    (58.0, 56.0),   (52.0, 56.0),   (46.0, 52.0),
    // Caspian
    (42.0, 50.0),   (38.0, 50.0),   (36.0, 46.0),
    // S Caucasus / Turkey N
    (38.0, 42.0),   (41.0, 36.0),   (41.0, 29.0), // back to start
];
const ASIA_MAINLAND_BBOX: BBox = BBox::new(0.0, 82.0, 26.0, 180.0);

/// Chukotka extension that wraps past 180° (treated as −180° to −168°).
/// Required because Russia's far east extends past the date line.
const CHUKOTKA: &[(f32, f32)] = &[
    (70.0, 180.0),  (66.0, 180.0),  (66.0, 172.0),
    (62.0, 170.0),  (60.0, 168.0),  (60.0, 175.0),
    (64.0, 178.0),  (66.0, 180.0),
];
const CHUKOTKA_BBOX: BBox = BBox::new(59.0, 72.0, 168.0, 180.0);

/// Chukotka mirrored on the west side of the date line (−180° to −168°).
const CHUKOTKA_WEST: &[(f32, f32)] = &[
    (66.0, -180.0), (64.0, -178.0), (62.0, -168.0),
    (60.0, -168.0), (60.0, -175.0), (64.0, -178.0),
    (66.0, -180.0),
];
const CHUKOTKA_WEST_BBOX: BBox = BBox::new(59.0, 72.0, -180.0, -166.0);

/// Australia (mainland) — counter-clockwise from NW.
const AUSTRALIA: &[(f32, f32)] = &[
    (-14.0, 128.0), (-12.0, 130.0), (-12.0, 136.0),
    (-12.0, 138.0), (-16.0, 136.0), (-12.0, 142.0),
    (-16.0, 146.0), (-22.0, 152.0), (-28.0, 154.0),
    (-32.0, 153.0), (-38.0, 146.0), (-39.0, 146.0),
    (-38.0, 140.0), (-36.0, 137.0), (-34.0, 135.0),
    (-32.0, 134.0), (-32.0, 116.0), (-22.0, 114.0),
    (-18.0, 122.0), (-16.0, 124.0), (-14.0, 128.0),
];
const AUSTRALIA_BBOX: BBox = BBox::new(-40.0, -10.0, 114.0, 154.0);

/// Iceland.
const ICELAND: &[(f32, f32)] = &[
    (66.0, -24.0),  (66.0, -14.0),  (64.0, -13.0),
    (63.0, -18.0),  (63.0, -24.0),  (64.0, -24.0),
    (66.0, -24.0),
];
const ICELAND_BBOX: BBox = BBox::new(62.0, 67.0, -25.0, -13.0);

/// Japan (Honshu + Kyushu + Shikoku + Hokkaido simplified to one polygon).
const JAPAN: &[(f32, f32)] = &[
    (41.5, 141.5),  // N Honshu / Hokkaido S
    (39.0, 141.5),  // NE Honshu coast
    (35.5, 140.5),  // Boso Peninsula (Tokyo area)
    (34.5, 137.0),  // Tokai coast
    (33.5, 131.5),  // SE Kyushu
    (31.5, 131.0),  // S tip Kyushu
    (31.5, 130.0),  // SW Kyushu
    (33.0, 130.0),  // N Kyushu
    (33.5, 130.5),  // N Kyushu E
    (34.5, 134.0),  // Shikoku N / Seto Inland Sea
    (35.0, 135.5),  // Osaka / Kinki
    (36.5, 136.5),  // Kanazawa
    (38.0, 138.5),  // Niigata
    (40.0, 140.0),  // NW Honshu
    (41.5, 141.5),  // back to start
];
const JAPAN_BBOX: BBox = BBox::new(31.0, 43.0, 129.0, 146.0);

/// Sumatra + Java simplified to a single elongated polygon.
const SUMATRA_JAVA: &[(f32, f32)] = &[
    (5.0, 95.0), (3.0, 99.0), (0.0, 104.0), (-2.0, 107.0),
    (-4.0, 108.0), (-6.0, 107.0), (-7.0, 112.0), (-8.0, 115.0),
    (-8.0, 116.0), (-4.0, 114.0), (-2.0, 110.0), (0.0, 109.0),
    (2.0, 107.0), (4.0, 103.0), (5.0, 99.0), (6.0, 97.0),
    (5.0, 95.0),
];
const SUMATRA_JAVA_BBOX: BBox = BBox::new(-9.0, 6.0, 94.0, 117.0);

/// Borneo.
const BORNEO: &[(f32, f32)] = &[
    (7.0, 117.0), (6.0, 118.0), (4.0, 118.0), (1.0, 119.0),
    (-4.0, 116.0), (-4.0, 114.0), (0.0, 110.0), (2.0, 109.0),
    (4.0, 109.0), (6.0, 116.0), (7.0, 117.0),
];
const BORNEO_BBOX: BBox = BBox::new(-5.0, 8.0, 108.0, 120.0);

/// New Guinea (Papua).
const NEW_GUINEA: &[(f32, f32)] = &[
    (-2.0, 132.0),  (-2.0, 141.0),  (-4.0, 142.0),
    (-6.0, 148.0),  (-8.0, 148.0),  (-8.0, 143.0),
    (-6.0, 140.0),  (-4.0, 138.0),  (-2.0, 132.0),
];
const NEW_GUINEA_BBOX: BBox = BBox::new(-9.0, -1.0, 130.0, 149.0);

/// Madagascar.
const MADAGASCAR: &[(f32, f32)] = &[
    (-12.0, 49.0),  (-14.0, 50.0),  (-18.0, 49.0),
    (-22.0, 48.0),  (-25.0, 44.0),  (-24.0, 43.0),
    (-18.0, 44.0),  (-14.0, 46.0),  (-12.0, 49.0),
];
const MADAGASCAR_BBOX: BBox = BBox::new(-26.0, -11.0, 42.0, 51.0);

// ── Point-in-polygon (even-odd rule) ─────────────────────────────────────────

/// Ray-casting point-in-polygon test for a horizontal ray toward +∞ lon.
fn pip(lat: f32, lon: f32, poly: &[(f32, f32)]) -> bool {
    let n = poly.len();
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (lat_i, lon_i) = poly[i];
        let (lat_j, lon_j) = poly[j];
        // Does the edge (j→i) straddle the query latitude?
        if (lat_i > lat) != (lat_j > lat) {
            // Longitude where the edge crosses the query latitude
            let cross_lon =
                lon_i + (lat - lat_i) / (lat_j - lat_i) * (lon_j - lon_i);
            if cross_lon > lon {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

// ── WorldMap ─────────────────────────────────────────────────────────────────

/// World map query interface.
///
/// ```rust,no_run
/// let wm = console_gis::data::WorldMap::new();
/// assert!(wm.is_land(51.5, -0.1)); // London
/// assert!(!wm.is_land(0.0, -30.0)); // mid-Atlantic
/// ```
pub struct WorldMap;

impl WorldMap {
    pub const fn new() -> Self { Self }

    /// Returns `true` if the given geographic coordinate is on land.
    ///
    /// Resolution: approximately 1–2° — sufficient for console zoom levels 0–8.
    ///
    /// Works entirely on the stack with no heap allocation, making it
    /// suitable for VT-100 and constrained environments.
    pub fn is_land(&self, lat: f64, lon: f64) -> bool {
        let lat = lat as f32;
        let lon = lon as f32;

        // Antarctica: analytic (south of ~65°S)
        if lat < -65.0 { return true; }

        // Bounding-box pre-filter then polygon test.
        // Listed roughly in order of world surface area (largest first for
        // fastest average rejection).
        if ASIA_MAINLAND_BBOX.contains(lat, lon)
            && pip(lat, lon, ASIA_MAINLAND) { return true; }
        if AFRICA_BBOX.contains(lat, lon)
            && pip(lat, lon, AFRICA) { return true; }
        if NORTH_AMERICA_BBOX.contains(lat, lon)
            && pip(lat, lon, NORTH_AMERICA) { return true; }
        if SOUTH_AMERICA_BBOX.contains(lat, lon)
            && pip(lat, lon, SOUTH_AMERICA) { return true; }
        if EUROPE_BBOX.contains(lat, lon)
            && pip(lat, lon, EUROPE) { return true; }
        if AUSTRALIA_BBOX.contains(lat, lon)
            && pip(lat, lon, AUSTRALIA) { return true; }
        if GREENLAND_BBOX.contains(lat, lon)
            && pip(lat, lon, GREENLAND) { return true; }
        if BORNEO_BBOX.contains(lat, lon)
            && pip(lat, lon, BORNEO) { return true; }
        if NEW_GUINEA_BBOX.contains(lat, lon)
            && pip(lat, lon, NEW_GUINEA) { return true; }
        if SUMATRA_JAVA_BBOX.contains(lat, lon)
            && pip(lat, lon, SUMATRA_JAVA) { return true; }
        if ICELAND_BBOX.contains(lat, lon)
            && pip(lat, lon, ICELAND) { return true; }
        if JAPAN_BBOX.contains(lat, lon)
            && pip(lat, lon, JAPAN) { return true; }
        if MADAGASCAR_BBOX.contains(lat, lon)
            && pip(lat, lon, MADAGASCAR) { return true; }
        if CHUKOTKA_BBOX.contains(lat, lon)
            && pip(lat, lon, CHUKOTKA) { return true; }
        if CHUKOTKA_WEST_BBOX.contains(lat, lon)
            && pip(lat, lon, CHUKOTKA_WEST) { return true; }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_land_points() {
        let wm = WorldMap::new();
        assert!(wm.is_land(51.5, -0.1),   "London");
        assert!(wm.is_land(40.7, -74.0),  "New York");
        assert!(wm.is_land(-33.9, 18.4),  "Cape Town");
        assert!(wm.is_land(35.7, 139.7),  "Tokyo");
        assert!(wm.is_land(-25.0, 130.0), "Australian outback");
        assert!(wm.is_land(-80.0, 0.0),   "Antarctica");
    }

    #[test]
    fn known_ocean_points() {
        let wm = WorldMap::new();
        assert!(!wm.is_land(0.0, -30.0),  "Mid-Atlantic");
        assert!(!wm.is_land(0.0, 180.0),  "Pacific date line");
        assert!(!wm.is_land(30.0, -40.0), "N Atlantic");
        assert!(!wm.is_land(-40.0, -90.0),"S Pacific");
    }
}
