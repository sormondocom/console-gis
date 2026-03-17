/// Embedded topographic elevation zones.
///
/// Polygon data at ~5° resolution — suitable for console zoom levels 0–8.
///
/// # Tiers
/// | Tier | Elevation     | Typical features                          |
/// |------|---------------|-------------------------------------------|
/// |  0   | < 300 m       | Coastal plains, lowlands (default)        |
/// |  1   | 300–1500 m    | Uplands, plateaus, lower ranges           |
/// |  2   | 1500–4000 m   | Major mountain ranges                     |
/// |  3   | > 4000 m      | Tibetan Plateau, Altiplano, highest peaks |

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

// ── Point-in-polygon (even-odd rule) ─────────────────────────────────────────

/// Ray-casting point-in-polygon test for a horizontal ray toward +∞ lon.
fn pip(lat: f32, lon: f32, poly: &[(f32, f32)]) -> bool {
    let n = poly.len();
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (lat_i, lon_i) = poly[i];
        let (lat_j, lon_j) = poly[j];
        if (lat_i > lat) != (lat_j > lat) {
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

// ── Tier 3 polygons (> 4000 m) ───────────────────────────────────────────────

const TIBETAN_PLATEAU: &[(f32, f32)] = &[
    (36.0, 74.0), (37.0, 82.0), (36.0, 88.0), (34.0, 96.0),
    (32.0, 102.0), (30.0, 104.0), (28.0, 100.0), (26.0, 92.0),
    (27.0, 88.0), (28.0, 82.0), (29.0, 78.0), (30.0, 74.0), (34.0, 72.0),
];
const TIBETAN_PLATEAU_BBOX: BBox = BBox::new(25.0, 38.0, 72.0, 105.0);

const HIGH_ANDES: &[(f32, f32)] = &[
    (-12.0, -69.0), (-14.0, -68.0), (-18.0, -68.0), (-22.0, -68.0),
    (-24.0, -67.0), (-22.0, -65.0), (-17.0, -65.0), (-13.0, -66.0),
];
const HIGH_ANDES_BBOX: BBox = BBox::new(-25.0, -11.0, -70.0, -64.0);

// ── Tier 2 polygons (1500–4000 m) ────────────────────────────────────────────

const ROCKY_MOUNTAINS: &[(f32, f32)] = &[
    (60.0, -136.0), (56.0, -130.0), (50.0, -120.0), (46.0, -116.0),
    (42.0, -114.0), (36.0, -110.0), (30.0, -106.0), (28.0, -107.0),
    (28.0, -109.0), (32.0, -112.0), (36.0, -113.0), (42.0, -118.0),
    (46.0, -122.0), (50.0, -126.0), (56.0, -133.0), (60.0, -140.0),
];
const ROCKY_MOUNTAINS_BBOX: BBox = BBox::new(27.0, 61.0, -141.0, -105.0);

const ANDES: &[(f32, f32)] = &[
    (10.0, -74.0), (6.0, -77.0), (0.0, -78.0), (-6.0, -79.0),
    (-12.0, -76.0), (-18.0, -70.0), (-24.0, -68.0), (-32.0, -70.0),
    (-40.0, -72.0), (-48.0, -75.0), (-54.0, -72.0), (-52.0, -70.0),
    (-46.0, -72.0), (-40.0, -70.0), (-32.0, -68.0), (-24.0, -65.0),
    (-18.0, -66.0), (-12.0, -74.0), (-6.0, -77.0), (0.0, -76.0),
    (6.0, -75.0), (10.0, -72.0),
];
const ANDES_BBOX: BBox = BBox::new(-55.0, 11.0, -80.0, -64.0);

const ALPS: &[(f32, f32)] = &[
    (48.0, 4.0), (47.0, 8.0), (48.0, 14.0), (48.0, 18.0),
    (46.0, 22.0), (44.0, 22.0), (43.0, 18.0), (43.0, 12.0),
    (43.0, 6.0), (45.0, 4.0),
];
const ALPS_BBOX: BBox = BBox::new(42.0, 49.0, 3.0, 23.0);

const CAUCASUS: &[(f32, f32)] = &[
    (44.0, 38.0), (44.0, 42.0), (44.0, 48.0), (40.0, 50.0),
    (40.0, 44.0), (40.0, 38.0), (42.0, 38.0),
];
const CAUCASUS_BBOX: BBox = BBox::new(39.0, 45.0, 37.0, 51.0);

const ZAGROS_IRAN: &[(f32, f32)] = &[
    (38.0, 44.0), (36.0, 52.0), (36.0, 60.0), (34.0, 64.0),
    (28.0, 62.0), (26.0, 58.0), (26.0, 54.0), (28.0, 50.0),
    (30.0, 48.0), (34.0, 44.0),
];
const ZAGROS_IRAN_BBOX: BBox = BBox::new(25.0, 39.0, 43.0, 65.0);

const ETHIOPIAN_HIGHLANDS: &[(f32, f32)] = &[
    (16.0, 36.0), (14.0, 38.0), (16.0, 42.0), (12.0, 44.0),
    (6.0, 42.0), (4.0, 38.0), (4.0, 34.0), (8.0, 34.0), (12.0, 36.0),
];
const ETHIOPIAN_HIGHLANDS_BBOX: BBox = BBox::new(3.0, 17.0, 33.0, 45.0);

const ATLAS: &[(f32, f32)] = &[
    (36.0, -4.0), (34.0, 4.0), (34.0, 10.0), (32.0, 8.0),
    (30.0, 2.0), (30.0, -2.0), (32.0, -4.0),
];
const ATLAS_BBOX: BBox = BBox::new(29.0, 37.0, -5.0, 11.0);

const SCANDINAVIAN_MTNS: &[(f32, f32)] = &[
    (70.0, 14.0), (68.0, 18.0), (64.0, 16.0), (62.0, 14.0),
    (60.0, 12.0), (58.0, 8.0), (58.0, 6.0), (60.0, 10.0),
    (64.0, 14.0), (68.0, 16.0), (70.0, 18.0),
];
const SCANDINAVIAN_MTNS_BBOX: BBox = BBox::new(57.0, 71.0, 5.0, 19.0);

const HINDU_KUSH: &[(f32, f32)] = &[
    (38.0, 62.0), (38.0, 68.0), (38.0, 74.0), (36.0, 76.0),
    (34.0, 72.0), (34.0, 64.0), (36.0, 62.0),
];
const HINDU_KUSH_BBOX: BBox = BBox::new(33.0, 39.0, 61.0, 77.0);

// ── Tier 1 polygons (300–1500 m) ─────────────────────────────────────────────

const APPALACHIANS: &[(f32, f32)] = &[
    (46.0, -72.0), (44.0, -72.0), (40.0, -76.0), (36.0, -80.0),
    (34.0, -84.0), (34.0, -86.0), (36.0, -84.0), (40.0, -78.0),
    (44.0, -72.0), (46.0, -70.0),
];
const APPALACHIANS_BBOX: BBox = BBox::new(33.0, 47.0, -87.0, -69.0);

const BRAZILIAN_HIGHLANDS: &[(f32, f32)] = &[
    (-4.0, -36.0), (-6.0, -40.0), (-10.0, -44.0), (-14.0, -46.0),
    (-20.0, -50.0), (-26.0, -50.0), (-30.0, -52.0), (-28.0, -50.0),
    (-22.0, -44.0), (-16.0, -40.0), (-8.0, -36.0),
];
const BRAZILIAN_HIGHLANDS_BBOX: BBox = BBox::new(-31.0, -3.0, -53.0, -35.0);

const DECCAN: &[(f32, f32)] = &[
    (24.0, 72.0), (22.0, 78.0), (22.0, 82.0), (18.0, 84.0),
    (14.0, 80.0), (10.0, 76.0), (8.0, 76.0), (10.0, 72.0),
    (16.0, 72.0), (22.0, 70.0),
];
const DECCAN_BBOX: BBox = BBox::new(7.0, 25.0, 69.0, 85.0);

const CENTRAL_AFRICAN_PLATEAU: &[(f32, f32)] = &[
    (-14.0, 24.0), (-12.0, 30.0), (-8.0, 38.0), (-2.0, 36.0),
    (2.0, 30.0), (0.0, 24.0), (-6.0, 22.0), (-12.0, 22.0),
];
const CENTRAL_AFRICAN_PLATEAU_BBOX: BBox = BBox::new(-15.0, 3.0, 21.0, 39.0);

const SIBERIAN_PLATEAU: &[(f32, f32)] = &[
    (70.0, 92.0), (70.0, 106.0), (70.0, 114.0), (62.0, 118.0),
    (56.0, 110.0), (54.0, 96.0), (58.0, 90.0), (64.0, 90.0),
];
const SIBERIAN_PLATEAU_BBOX: BBox = BBox::new(53.0, 71.0, 89.0, 119.0);

// ── TopoMap ───────────────────────────────────────────────────────────────────

/// Topographic elevation query interface.
///
/// Returns an elevation tier (0–3) for any geographic coordinate.
/// Zero-sized; all data is compiled into the binary.
pub struct TopoMap;

impl TopoMap {
    pub const fn new() -> Self { Self }

    /// Returns the elevation tier for the given geographic coordinate.
    ///
    /// | Tier | Elevation     |
    /// |------|---------------|
    /// |  0   | < 300 m       |
    /// |  1   | 300–1500 m    |
    /// |  2   | 1500–4000 m   |
    /// |  3   | > 4000 m      |
    pub fn elevation_tier(&self, lat: f64, lon: f64) -> u8 {
        let lat = lat as f32;
        let lon = lon as f32;

        // Tier 3 first (highest elevation)
        if TIBETAN_PLATEAU_BBOX.contains(lat, lon)
            && pip(lat, lon, TIBETAN_PLATEAU) { return 3; }
        if HIGH_ANDES_BBOX.contains(lat, lon)
            && pip(lat, lon, HIGH_ANDES) { return 3; }

        // Tier 2
        if ROCKY_MOUNTAINS_BBOX.contains(lat, lon)
            && pip(lat, lon, ROCKY_MOUNTAINS) { return 2; }
        if ANDES_BBOX.contains(lat, lon)
            && pip(lat, lon, ANDES) { return 2; }
        if ALPS_BBOX.contains(lat, lon)
            && pip(lat, lon, ALPS) { return 2; }
        if CAUCASUS_BBOX.contains(lat, lon)
            && pip(lat, lon, CAUCASUS) { return 2; }
        if ZAGROS_IRAN_BBOX.contains(lat, lon)
            && pip(lat, lon, ZAGROS_IRAN) { return 2; }
        if ETHIOPIAN_HIGHLANDS_BBOX.contains(lat, lon)
            && pip(lat, lon, ETHIOPIAN_HIGHLANDS) { return 2; }
        if ATLAS_BBOX.contains(lat, lon)
            && pip(lat, lon, ATLAS) { return 2; }
        if SCANDINAVIAN_MTNS_BBOX.contains(lat, lon)
            && pip(lat, lon, SCANDINAVIAN_MTNS) { return 2; }
        if HINDU_KUSH_BBOX.contains(lat, lon)
            && pip(lat, lon, HINDU_KUSH) { return 2; }

        // Tier 1
        if APPALACHIANS_BBOX.contains(lat, lon)
            && pip(lat, lon, APPALACHIANS) { return 1; }
        if BRAZILIAN_HIGHLANDS_BBOX.contains(lat, lon)
            && pip(lat, lon, BRAZILIAN_HIGHLANDS) { return 1; }
        if DECCAN_BBOX.contains(lat, lon)
            && pip(lat, lon, DECCAN) { return 1; }
        if CENTRAL_AFRICAN_PLATEAU_BBOX.contains(lat, lon)
            && pip(lat, lon, CENTRAL_AFRICAN_PLATEAU) { return 1; }
        if SIBERIAN_PLATEAU_BBOX.contains(lat, lon)
            && pip(lat, lon, SIBERIAN_PLATEAU) { return 1; }

        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_tiers() {
        let t = TopoMap::new();
        assert_eq!(t.elevation_tier(34.0, 88.0), 3, "Tibetan Plateau");
        assert_eq!(t.elevation_tier(-18.0, -67.0), 3, "High Andes");
        assert_eq!(t.elevation_tier(46.0, 10.0), 2, "Alps");
        assert_eq!(t.elevation_tier(40.0, -78.0), 1, "Appalachians");
        assert_eq!(t.elevation_tier(51.5, -0.1), 0, "London lowlands");
        assert_eq!(t.elevation_tier(0.0, -30.0), 0, "Mid-Atlantic ocean");
    }
}
