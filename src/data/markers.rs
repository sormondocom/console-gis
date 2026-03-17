/// Geographic annotation (marker) system backed by an embedded sled database.
///
/// # Data model
///
/// Each marker has:
/// - A unique `id` (auto-incrementing u64).
/// - A geographic position (`lat`, `lon` in WGS-84 decimal degrees).
/// - A `symbol` — any single Unicode grapheme, or ASCII char for VT-100 mode.
/// - A `label`  — free-text string.
///
/// # Persistence
///
/// Markers are stored in a `sled` embedded database at a configurable path.
/// The database is opened once and kept open for the process lifetime.
/// All operations are synchronous and immediately durable.
///
/// # VT-100 compatibility
///
/// `symbol` fields may contain Unicode characters (e.g. "★"), but when
/// rendering on a VT-100 terminal the caller should substitute a plain ASCII
/// character.  The [`Marker::ascii_symbol`] helper does this.

use std::path::Path;
use serde::{Deserialize, Serialize};

// ── Marker data type ──────────────────────────────────────────────────────────

/// A geographic annotation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Marker {
    /// Unique identifier (auto-assigned by the store).
    pub id:     u64,
    /// Latitude in WGS-84 decimal degrees [-90, 90].
    pub lat:    f64,
    /// Longitude in WGS-84 decimal degrees [-180, 180].
    pub lon:    f64,
    /// Display symbol — a single grapheme cluster (e.g. "★", "●", "A").
    pub symbol: String,
    /// Human-readable label.
    pub label:  String,
    /// When true the symbol is rendered with the terminal blink attribute
    /// (`ESC[5m`).  Falls back gracefully on terminals that ignore blink.
    #[serde(default)]
    pub blink:  bool,
}

impl Marker {
    /// Return an ASCII-safe symbol for VT-100 rendering.
    pub fn ascii_symbol(&self) -> char {
        self.symbol.chars().next()
            .map(|c| if c.is_ascii_graphic() { c } else { '*' })
            .unwrap_or('*')
    }

    /// Convert this marker's (lat, lon) to a unit-sphere 3-D point.
    pub fn to_xyz(&self) -> (f64, f64, f64) {
        let lat_r = self.lat.to_radians();
        let lon_r = self.lon.to_radians();
        (
            lat_r.cos() * lon_r.sin(),
            lat_r.sin(),
            -(lat_r.cos() * lon_r.cos()),
        )
    }
}

// ── MarkerStore ───────────────────────────────────────────────────────────────

/// Persistent marker store backed by a `sled` embedded database.
pub struct MarkerStore {
    db:      sled::Db,
    markers: sled::Tree, // key: u64 id (big-endian), value: JSON bytes
    seq:     sled::Tree, // single entry: "next_id" → u64
}

impl MarkerStore {
    /// Open (or create) a marker database at `path`.
    ///
    /// The directory is created automatically if it does not exist.
    pub fn open<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let db      = sled::open(path.as_ref())?;
        let markers = db.open_tree("markers")?;
        let seq     = db.open_tree("seq")?;
        Ok(Self { db, markers, seq })
    }

    /// Insert a new marker.  `id` is assigned automatically; the returned
    /// `Marker` has the assigned id filled in.
    pub fn insert(
        &self,
        lat:    f64,
        lon:    f64,
        symbol: impl Into<String>,
        label:  impl Into<String>,
    ) -> anyhow::Result<Marker> {
        self.insert_with_blink(lat, lon, symbol, label, false)
    }

    /// Insert a new marker with explicit blink setting.
    pub fn insert_with_blink(
        &self,
        lat:    f64,
        lon:    f64,
        symbol: impl Into<String>,
        label:  impl Into<String>,
        blink:  bool,
    ) -> anyhow::Result<Marker> {
        let id = self.next_id()?;
        let marker = Marker {
            id,
            lat,
            lon,
            symbol: symbol.into(),
            label:  label.into(),
            blink,
        };
        let json = serde_json::to_vec(&marker)?;
        self.markers.insert(id.to_be_bytes(), json)?;
        Ok(marker)
    }

    /// Update an existing marker (matched by `id`).
    pub fn update(&self, marker: &Marker) -> anyhow::Result<bool> {
        let key = marker.id.to_be_bytes();
        if self.markers.contains_key(key)? {
            let json = serde_json::to_vec(marker)?;
            self.markers.insert(key, json)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Delete a marker by id.  Returns `true` if it existed.
    pub fn delete(&self, id: u64) -> anyhow::Result<bool> {
        Ok(self.markers.remove(id.to_be_bytes())?.is_some())
    }

    /// Retrieve a marker by id.
    pub fn get(&self, id: u64) -> anyhow::Result<Option<Marker>> {
        match self.markers.get(id.to_be_bytes())? {
            Some(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            None        => Ok(None),
        }
    }

    /// Return all markers as a `Vec`, sorted by id.
    pub fn all(&self) -> anyhow::Result<Vec<Marker>> {
        let mut out = Vec::new();
        for result in self.markers.iter() {
            let (_k, v) = result?;
            out.push(serde_json::from_slice::<Marker>(&v)?);
        }
        Ok(out)
    }

    /// Return markers whose position is within `radius_deg` degrees of
    /// (lat, lon) — useful for viewport-culling before rendering.
    pub fn near(&self, lat: f64, lon: f64, radius_deg: f64) -> anyhow::Result<Vec<Marker>> {
        Ok(self.all()?.into_iter().filter(|m| {
            let dlat = (m.lat - lat).abs();
            let dlon = (m.lon - lon).abs().min(360.0 - (m.lon - lon).abs());
            let dist = (dlat * dlat + dlon * dlon).sqrt();
            dist <= radius_deg
        }).collect())
    }

    /// Total number of stored markers.
    pub fn count(&self) -> usize {
        self.markers.len()
    }

    /// Permanently delete every marker in the store.
    ///
    /// The sequence counter is **not** reset — subsequent inserts continue
    /// from the next id, ensuring old ids are never reused within a session.
    pub fn clear_all(&self) -> anyhow::Result<usize> {
        let count = self.markers.len();
        self.markers.clear()?;
        self.markers.flush()?;
        Ok(count)
    }

    // ── Internal ──────────────────────────────────────────────────────────────

    fn next_id(&self) -> anyhow::Result<u64> {
        // Atomic fetch-and-increment stored in the `seq` tree.
        let old = self.seq
            .fetch_and_update("next_id", |old| {
                let n = old.map(|b| {
                    let mut arr = [0u8; 8];
                    arr.copy_from_slice(&b[..8.min(b.len())]);
                    u64::from_be_bytes(arr)
                }).unwrap_or(0);
                Some((n + 1).to_be_bytes().to_vec())
            })?;
        let id = old.map(|b| {
            let mut arr = [0u8; 8];
            arr.copy_from_slice(&b[..8.min(b.len())]);
            u64::from_be_bytes(arr)
        }).unwrap_or(0);
        Ok(id)
    }
}

// ── Globe projection helpers ──────────────────────────────────────────────────

use crate::render::globe::{GlobeParams};

/// Project a marker onto the screen.
///
/// Returns `Some((screen_col, screen_row))` if the marker is on the visible
/// hemisphere, or `None` if it is behind the globe.
///
/// `cx`, `cy` = screen centre in pixel coords.
/// `scale`    = pixels per unit sphere radius.
pub fn project_marker(
    marker: &Marker,
    params: &GlobeParams,
    cx: f64,
    cy: f64,
    scale: f64,
) -> Option<(i32, i32)> {
    let (x, y, z) = marker.to_xyz();

    // Apply globe rotation (same as in the renderer, but forward direction).
    let (x, y, z) = {
        // rot_y around Y
        let a = params.rot_y;
        let (c, s) = (a.cos(), a.sin());
        let rx = x * c + z * s;
        let rz = -x * s + z * c;
        (rx, y, rz)
    };
    let (x, y, z) = {
        // rot_x around X
        let a = params.rot_x;
        let (c, s) = (a.cos(), a.sin());
        let ry = y * c - z * s;
        let rz = y * s + z * c;
        (x, ry, rz)
    };

    // The eye is at (0, 0, -eye_z).  A point is visible if it faces the eye,
    // i.e. if its z component in view space is negative (front hemisphere).
    if z >= 0.0 { return None; }

    // Simple orthographic-ish projection onto screen (same as renderer NDC).
    let screen_col = (x  * scale + cx) as i32;
    let screen_row = (-y * scale + cy) as i32;

    Some((screen_col, screen_row))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    static TEST_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn temp_db() -> PathBuf {
        let id = TEST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut p = std::env::temp_dir();
        p.push(format!("console_gis_test_{}_{}", std::process::id(), id));
        p
    }

    #[test]
    fn insert_and_retrieve() {
        let path = temp_db();
        let store = MarkerStore::open(&path).unwrap();
        let m = store.insert(51.5, -0.1, "★", "London").unwrap();
        assert_eq!(m.lat, 51.5);
        assert_eq!(m.label, "London");
        let fetched = store.get(m.id).unwrap().unwrap();
        assert_eq!(fetched.symbol, "★");
        drop(store);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn delete_marker() {
        let path = temp_db();
        let store = MarkerStore::open(&path).unwrap();
        let m = store.insert(0.0, 0.0, "X", "Null Island").unwrap();
        assert!(store.delete(m.id).unwrap());
        assert!(!store.delete(m.id).unwrap()); // already deleted
        drop(store);
        let _ = std::fs::remove_dir_all(path);
    }
}
