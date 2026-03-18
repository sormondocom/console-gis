//! Pure geographic calculation functions — no TUI dependencies.
//!
//! Covers:
//! - OSM/slippy tile ↔ lat/lon conversions
//! - Web Mercator (EPSG:3857) ↔ WGS-84
//! - Decimal degrees ↔ DMS / DDM
//! - Haversine distance, initial bearing, destination point

use std::f64::consts::PI;

/// WGS-84 semi-major axis in metres.
const R: f64 = 6_378_137.0;

// ── Slippy tile math (OSM / XYZ tile scheme) ──────────────────────────────────

/// Convert WGS-84 lat/lon to OSM slippy tile (X, Y) at zoom level `z`.
pub fn latlon_to_tile(lat: f64, lon: f64, zoom: u8) -> (u32, u32) {
    let n = 1u64 << zoom;
    let lat_r = lat.clamp(-85.051129, 85.051129).to_radians();
    let x = ((lon + 180.0) / 360.0 * n as f64).floor() as u64;
    let y = ((1.0 - (lat_r.tan() + 1.0 / lat_r.cos()).ln() / PI) / 2.0 * n as f64).floor() as u64;
    (x.min(n - 1) as u32, y.min(n - 1) as u32)
}

/// Convert slippy tile (X, Y) + zoom to the NW-corner lat/lon.
pub fn tile_to_latlon_nw(tile_x: u32, tile_y: u32, zoom: u8) -> (f64, f64) {
    let n = 1u64 << zoom;
    let lon = tile_x as f64 / n as f64 * 360.0 - 180.0;
    let lat_r = ((1.0 - 2.0 * tile_y as f64 / n as f64) * PI).sinh().atan();
    (lat_r.to_degrees(), lon)
}

/// Bounding box of a slippy tile: (south, west, north, east).
pub fn tile_bbox(tile_x: u32, tile_y: u32, zoom: u8) -> (f64, f64, f64, f64) {
    let (north, west) = tile_to_latlon_nw(tile_x, tile_y, zoom);
    let (south, east) = tile_to_latlon_nw(tile_x + 1, tile_y + 1, zoom);
    (south, west, north, east)
}

// ── Web Mercator (EPSG:3857) ───────────────────────────────────────────────────

/// WGS-84 (lat°, lon°) → Web Mercator (X metres, Y metres).
pub fn wgs84_to_mercator(lat: f64, lon: f64) -> (f64, f64) {
    let x = R * lon.to_radians();
    let y = R * (PI / 4.0 + lat.to_radians() / 2.0).tan().ln();
    (x, y)
}

/// Web Mercator (X metres, Y metres) → WGS-84 (lat°, lon°).
pub fn mercator_to_wgs84(x_m: f64, y_m: f64) -> (f64, f64) {
    let lon = x_m / R * (180.0 / PI);
    let lat = (2.0 * (y_m / R).exp().atan() - PI / 2.0).to_degrees();
    (lat, lon)
}

// ── Coordinate format conversions ─────────────────────────────────────────────

/// Decimal degrees → (degrees, minutes, seconds, hemisphere char).
///
/// `is_lat`: true → 'N'/'S', false → 'E'/'W'.
pub fn dd_to_dms(dd: f64, is_lat: bool) -> (u32, u32, f64, char) {
    let abs = dd.abs();
    let deg = abs.floor() as u32;
    let min_f = (abs - deg as f64) * 60.0;
    let min = min_f.floor() as u32;
    let sec = (min_f - min as f64) * 60.0;
    let dir = if is_lat {
        if dd >= 0.0 { 'N' } else { 'S' }
    } else {
        if dd >= 0.0 { 'E' } else { 'W' }
    };
    (deg, min, sec, dir)
}

/// DMS → decimal degrees. Pass `negative = true` for S/W.
pub fn dms_to_dd(deg: f64, min: f64, sec: f64, negative: bool) -> f64 {
    let abs = deg.abs() + min / 60.0 + sec / 3600.0;
    if negative { -abs } else { abs }
}

/// Decimal degrees → Degrees Decimal Minutes: (degrees, decimal_minutes, hemisphere).
pub fn dd_to_ddm(dd: f64, is_lat: bool) -> (u32, f64, char) {
    let abs = dd.abs();
    let deg = abs.floor() as u32;
    let dec_min = (abs - deg as f64) * 60.0;
    let dir = if is_lat {
        if dd >= 0.0 { 'N' } else { 'S' }
    } else {
        if dd >= 0.0 { 'E' } else { 'W' }
    };
    (deg, dec_min, dir)
}

// ── Great-circle calculations ─────────────────────────────────────────────────

/// Haversine distance in **metres** between two WGS-84 points.
pub fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let a = (dlat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlon / 2.0).sin().powi(2);
    2.0 * R * a.sqrt().asin()
}

/// Initial bearing from (lat1, lon1) → (lat2, lon2), in degrees [0, 360).
pub fn bearing_deg(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let dlon = (lon2 - lon1).to_radians();
    let lat1r = lat1.to_radians();
    let lat2r = lat2.to_radians();
    let y = dlon.sin() * lat2r.cos();
    let x = lat1r.cos() * lat2r.sin() - lat1r.sin() * lat2r.cos() * dlon.cos();
    (y.atan2(x).to_degrees() + 360.0) % 360.0
}

/// Destination point from (lat1°, lon1°) travelling `distance_m` metres at `bearing°`.
/// Returns (lat2°, lon2°).
pub fn destination_point(lat1: f64, lon1: f64, bearing_deg: f64, distance_m: f64) -> (f64, f64) {
    let d = distance_m / R;
    let brng = bearing_deg.to_radians();
    let lat1r = lat1.to_radians();
    let lon1r = lon1.to_radians();
    let lat2r = (lat1r.sin() * d.cos() + lat1r.cos() * d.sin() * brng.cos()).asin();
    let lon2r = lon1r
        + (brng.sin() * d.sin() * lat1r.cos())
            .atan2(d.cos() - lat1r.sin() * lat2r.sin());
    (lat2r.to_degrees(), ((lon2r.to_degrees() + 540.0) % 360.0) - 180.0)
}

/// 16-point compass direction for a bearing in degrees.
pub fn compass_dir(bearing: f64) -> &'static str {
    let b = ((bearing % 360.0) + 360.0) % 360.0;
    match b as u32 {
        0..=11              => "N",
        12..=33             => "NNE",
        34..=56             => "NE",
        57..=78             => "ENE",
        79..=101            => "E",
        102..=123           => "ESE",
        124..=146           => "SE",
        147..=168           => "SSE",
        169..=191           => "S",
        192..=213           => "SSW",
        214..=236           => "SW",
        237..=258           => "WSW",
        259..=281           => "W",
        282..=303           => "WNW",
        304..=326           => "NW",
        327..=348           => "NNW",
        _                   => "N",
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_roundtrip() {
        // London ~51.5°N, 0.1°W at zoom 10
        let (tx, ty) = latlon_to_tile(51.5, -0.1, 10);
        let (south, west, north, east) = tile_bbox(tx, ty, 10);
        assert!(south <= 51.5 && 51.5 <= north, "lat {south}..{north}");
        assert!(west  <= -0.1 && -0.1 <= east,  "lon {west}..{east}");
    }

    #[test]
    fn mercator_roundtrip() {
        let (xm, ym) = wgs84_to_mercator(51.5, -0.1);
        let (lat, lon) = mercator_to_wgs84(xm, ym);
        assert!((lat - 51.5).abs() < 1e-6, "lat {lat}");
        assert!((lon - -0.1).abs() < 1e-6, "lon {lon}");
    }

    #[test]
    fn dms_roundtrip() {
        let dd = 51.4778;
        let (d, m, s, dir) = dd_to_dms(dd, true);
        let back = dms_to_dd(d as f64, m as f64, s, false);
        assert_eq!(dir, 'N');
        assert!((back - dd).abs() < 1e-9, "back {back}");
    }

    #[test]
    fn haversine_london_paris() {
        // London ↔ Paris: spherical haversine with R=6,378,137 gives ~344 km.
        // (The often-cited "341 km" uses a different datum/ellipsoid.)
        let d = haversine_m(51.5074, -0.1278, 48.8566, 2.3522);
        assert!((d / 1000.0 - 344.0).abs() < 5.0, "dist {d}");
    }

    #[test]
    fn bearing_london_paris() {
        let b = bearing_deg(51.5074, -0.1278, 48.8566, 2.3522);
        assert!((b - 148.0).abs() < 2.0, "bearing {b}");
    }

    #[test]
    fn destination_roundtrip() {
        let (lat2, lon2) = destination_point(51.5074, -0.1278, 148.0, 341_571.0);
        assert!((lat2 - 48.8566).abs() < 0.05, "lat2 {lat2}");
        assert!((lon2 - 2.3522).abs() < 0.05,  "lon2 {lon2}");
    }
}
