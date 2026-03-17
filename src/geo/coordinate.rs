/// A geographic coordinate in WGS-84 decimal degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LatLon {
    /// Latitude  – positive North, range [-90, 90]
    pub lat: f64,
    /// Longitude – positive East,  range [-180, 180]
    pub lon: f64,
}

impl LatLon {
    pub fn new(lat: f64, lon: f64) -> Self {
        Self {
            lat: lat.clamp(-90.0, 90.0),
            lon: ((lon + 180.0).rem_euclid(360.0)) - 180.0,
        }
    }

    /// Great-circle distance to another point in metres (Haversine).
    pub fn distance_m(&self, other: LatLon) -> f64 {
        const R: f64 = 6_371_000.0;
        let dlat = (other.lat - self.lat).to_radians();
        let dlon = (other.lon - self.lon).to_radians();
        let a = (dlat / 2.0).sin().powi(2)
            + self.lat.to_radians().cos()
                * other.lat.to_radians().cos()
                * (dlon / 2.0).sin().powi(2);
        2.0 * R * a.sqrt().asin()
    }
}

impl std::fmt::Display for LatLon {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let lat_dir = if self.lat >= 0.0 { 'N' } else { 'S' };
        let lon_dir = if self.lon >= 0.0 { 'E' } else { 'W' };
        write!(
            f,
            "{:.4}°{} {:.4}°{}",
            self.lat.abs(),
            lat_dir,
            self.lon.abs(),
            lon_dir
        )
    }
}

/// An axis-aligned bounding box in geographic coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingBox {
    pub south: f64,
    pub west:  f64,
    pub north: f64,
    pub east:  f64,
}

impl BoundingBox {
    pub fn new(south: f64, west: f64, north: f64, east: f64) -> Self {
        Self { south, west, north, east }
    }

    pub fn center(&self) -> LatLon {
        LatLon::new(
            (self.south + self.north) / 2.0,
            (self.west  + self.east)  / 2.0,
        )
    }

    pub fn lat_span(&self) -> f64 { self.north - self.south }
    pub fn lon_span(&self) -> f64 { self.east  - self.west  }

    /// The whole world.
    pub fn world() -> Self {
        Self::new(-90.0, -180.0, 90.0, 180.0)
    }
}
