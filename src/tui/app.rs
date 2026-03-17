use std::path::PathBuf;
use serde::{Deserialize, Serialize};

use crate::geo::{BoundingBox, ConsoleResolution, LatLon};
use crate::geo::zoom::RenderMode;
use crate::render::canvas::TerminalCapability;
use crate::render::globe::GlobeParams;
use crate::data::{WorldMap, MarkerStore, GeoLayer};

// ── Marker placement / editing input ─────────────────────────────────────────

/// Which field is currently being entered in the marker input overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkerInputStep {
    Symbol,
    Label,
    Blink,  // Yes/No prompt for blink attribute
}

/// State for the two/three-step marker placement or edit overlay.
#[derive(Debug, Clone)]
pub struct MarkerInput {
    /// Geographic position — set at placement time, unchanged during edit.
    pub lat:        f64,
    pub lon:        f64,
    /// Current text in the symbol input box.
    pub symbol_buf: String,
    /// Current text in the label input box.
    pub label_buf:  String,
    /// Whether blink is toggled on.
    pub blink:      bool,
    /// Which step we are on.
    pub step:       MarkerInputStep,
    /// `Some(id)` when editing an existing marker; `None` when inserting new.
    pub edit_id:    Option<u64>,
}

// ── Persisted state ───────────────────────────────────────────────────────────

/// A saved view position — either a globe or a flat-map state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    /// User-supplied name for the bookmark.
    pub label:     String,
    /// `"globe"` or `"map"`.
    pub view_type: String,
    // Globe fields (used when view_type == "globe")
    pub glob_rot_y: f64,
    pub glob_rot_x: f64,
    pub glob_zoom:  f64,
    // Map fields (used when view_type == "map")
    pub map_lat:   f64,
    pub map_lon:   f64,
    pub map_zoom:  u8,
}

/// State that survives across sessions (written to disk on quit).
///
/// Stored as JSON alongside the marker database so the same data directory
/// holds everything.  Fields are kept minimal — only things the user would
/// notice losing (centre position, zoom, loaded layer paths).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedState {
    pub map_lat:      f64,
    pub map_lon:      f64,
    pub map_zoom:     u8,
    pub globe_rot_y:  f64,
    pub globe_rot_x:  f64,
    pub globe_zoom:   f64,
    /// Absolute paths of GeoJSON files that were loaded last session.
    pub layer_paths:  Vec<String>,
    /// Named saved view positions.
    #[serde(default)]
    pub bookmarks:    Vec<Bookmark>,
}

impl Default for SavedState {
    fn default() -> Self {
        Self {
            map_lat:     20.0,
            map_lon:     10.0,
            map_zoom:    2,
            globe_rot_y: 0.0,
            globe_rot_x: 0.0,
            globe_zoom:  1.0,
            layer_paths: Vec::new(),
            bookmarks:   Vec::new(),
        }
    }
}

impl SavedState {
    pub fn load(path: &PathBuf) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &PathBuf) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }
}

// ── Application state ─────────────────────────────────────────────────────────

/// Top-level application state.
pub struct App {
    pub view:           View,
    pub capability:     TerminalCapability,
    pub render_mode:    RenderMode,
    /// Globe rendering parameters (rotation + zoom).
    pub globe:          GlobeParams,
    /// Flat-map centre.
    pub map_centre:     LatLon,
    /// Flat-map zoom level (0–20).
    pub zoom:           u8,
    pub resolution:     ConsoleResolution,
    pub animating:      bool,
    pub should_quit:    bool,
    /// Shared world map data (no heap allocation after construction).
    pub world:          WorldMap,
    /// Persistent geographic annotations.
    pub markers:        MarkerStore,
    /// True when the marker-placement crosshair is shown (globe/map moves the
    /// aim point; actual input collected in `marker_input`).
    pub placing_marker: bool,
    /// Cursor position on the globe (derived from current rotation centre).
    pub globe_cursor:   LatLon,

    // ── Marker input overlay ──────────────────────────────────────────────────
    /// Active when the symbol/label/blink prompt is shown.
    pub marker_input:   Option<MarkerInput>,

    // ── Marker list view state ────────────────────────────────────────────────
    /// Currently highlighted row in the `MarkerList` view.
    pub marker_list_sel:    usize,
    /// True when the single-marker delete confirmation is showing.
    pub marker_del_confirm: bool,

    // ── GeoJSON layer management ──────────────────────────────────────────────
    /// In-memory GeoJSON layers.  Rendered on both map and globe views.
    pub geo_layers:     Vec<GeoLayer>,
    /// True when the GeoJSON file-path input overlay is active.
    pub importing:      bool,
    /// Current text in the import path input box.
    pub import_buf:     String,
    /// Last import error (displayed in the overlay until cleared).
    pub import_error:   Option<String>,

    // ── Marker clear confirmation ─────────────────────────────────────────────
    /// True when the "clear all markers?" confirmation overlay is active.
    pub clearing_markers: bool,

    // ── Bookmark overlay ──────────────────────────────────────────────────────
    /// True when the bookmark-name input overlay is active.
    pub bookmarking:      bool,
    /// Text buffer for the bookmark name being typed.
    pub bookmark_buf:     String,

    // ── Session persistence ───────────────────────────────────────────────────
    /// Path to the saved-state JSON file.
    pub state_path:     PathBuf,
}

impl App {
    pub fn new(
        capability:  TerminalCapability,
        markers:     MarkerStore,
        state_path:  PathBuf,
        saved:       &SavedState,
    ) -> Self {
        let render_mode = if capability.supports_half_block() {
            RenderMode::HalfBlock
        } else {
            RenderMode::Ascii
        };

        Self {
            view:           View::Splash,
            capability,
            render_mode,
            globe:          GlobeParams {
                rot_y: saved.globe_rot_y,
                rot_x: saved.globe_rot_x,
                zoom:  saved.globe_zoom,
            },
            map_centre:     LatLon::new(saved.map_lat, saved.map_lon),
            zoom:           saved.map_zoom,
            resolution:     ConsoleResolution::new(render_mode),
            animating:      true,
            should_quit:    false,
            world:          WorldMap::new(),
            markers,
            placing_marker:     false,
            globe_cursor:       LatLon::new(0.0, 0.0),
            marker_input:       None,
            marker_list_sel:    0,
            marker_del_confirm: false,
            geo_layers:         Vec::new(),
            importing:        false,
            import_buf:       String::new(),
            import_error:     None,
            clearing_markers: false,
            bookmarking:      false,
            bookmark_buf:     String::new(),
            state_path,
        }
    }

    /// Restore GeoJSON layers from a previous session.
    ///
    /// Missing or unreadable files produce a warning entry in `import_error`
    /// rather than a hard failure.  The returned vec contains one warning
    /// string per failed path.
    pub fn restore_layers(&mut self, paths: &[String]) -> Vec<String> {
        let mut warnings = Vec::new();
        for path_str in paths {
            let p = PathBuf::from(path_str);
            if !p.exists() {
                warnings.push(format!(
                    "GeoJSON not found (skipped): {}",
                    p.display()
                ));
                continue;
            }
            match GeoLayer::load(&p) {
                Ok(layer) => self.geo_layers.push(layer),
                Err(e) => warnings.push(format!(
                    "Could not load {} — {}",
                    p.display(), e
                )),
            }
        }
        warnings
    }

    /// Try to load a GeoJSON layer from `path`.
    ///
    /// On success the layer is pushed to `geo_layers`.
    /// On failure the error is stored in `import_error`.
    /// Returns `true` on success.
    pub fn load_geo_layer(&mut self, path: &str) -> bool {
        let p = PathBuf::from(path.trim());
        if !p.exists() {
            self.import_error = Some(format!("File not found: {}", p.display()));
            return false;
        }
        match GeoLayer::load(&p) {
            Ok(layer) => {
                self.import_error = None;
                self.geo_layers.push(layer);
                true
            }
            Err(e) => {
                self.import_error = Some(format!("Load error: {e}"));
                false
            }
        }
    }

    /// Save current state to disk.
    pub fn save_state(&self) {
        let existing = SavedState::load(&self.state_path);
        let saved = SavedState {
            map_lat:     self.map_centre.lat,
            map_lon:     self.map_centre.lon,
            map_zoom:    self.zoom,
            globe_rot_y: self.globe.rot_y,
            globe_rot_x: self.globe.rot_x,
            globe_zoom:  self.globe.zoom,
            layer_paths: self.geo_layers.iter()
                .map(|l| l.source.clone())
                .collect(),
            bookmarks:   existing.bookmarks,  // preserve bookmarks across sessions
        };
        saved.save(&self.state_path);
    }

    /// Save a bookmark for the current globe or map position.
    ///
    /// The bookmark is written directly to the state file so it persists even
    /// if the session ends without a normal quit.
    pub fn save_bookmark(&self, label: &str) {
        let mut state = SavedState::load(&self.state_path);
        let bm = match self.view {
            View::Globe => Bookmark {
                label:      label.to_string(),
                view_type:  "globe".to_string(),
                glob_rot_y: self.globe.rot_y,
                glob_rot_x: self.globe.rot_x,
                glob_zoom:  self.globe.zoom,
                map_lat:    self.globe_cursor.lat,
                map_lon:    self.globe_cursor.lon,
                map_zoom:   self.zoom,
            },
            _ => Bookmark {
                label:      label.to_string(),
                view_type:  "map".to_string(),
                glob_rot_y: 0.0,
                glob_rot_x: 0.0,
                glob_zoom:  1.0,
                map_lat:    self.map_centre.lat,
                map_lon:    self.map_centre.lon,
                map_zoom:   self.zoom,
            },
        };
        state.bookmarks.push(bm);
        state.save(&self.state_path);
    }

    /// Navigate to a new view.
    ///
    /// When switching between the Globe and the flat Map, the geographic centre
    /// is synchronised so both views open at the same location, preventing
    /// the disorienting position jump that would otherwise occur.
    pub fn navigate(&mut self, view: View) {
        match (self.view, view) {
            // Globe → Map: translate globe rotation centre to map centre
            (View::Globe, View::Map) => {
                let c = self.globe_centre();
                self.map_centre = c;
                // Map zoom is the geographic floor of globe zoom; clamp to valid range.
                // globe.zoom 1.0 ≈ full earth → map zoom ~2; each 2× globe zoom ≈ +1 map zoom
                let gz = self.globe.zoom.clamp(0.5, 8.0).log2();
                self.zoom = (2.0 + gz * 1.5).round().clamp(0.0, 20.0) as u8;
            }
            // Map → Globe: set globe rotation so the map centre is front-and-centre
            (View::Map, View::Globe) => {
                let lat_r = self.map_centre.lat.to_radians();
                let lon_r = self.map_centre.lon.to_radians();
                // We want the globe to show map_centre at screen centre.
                // Screen centre = (0,0,−1) in view space before rotation.
                // To bring (lat,lon) to screen centre we set:
                //   rot_y = lon (longitude spin)
                //   rot_x = −lat (latitude tilt, sign because rot_x tilts north up)
                self.globe.rot_y = lon_r;
                self.globe.rot_x = -lat_r;
                // Translate map zoom → globe zoom
                let gz = ((self.zoom as f64 - 2.0) / 1.5).exp2().clamp(0.5, 8.0);
                self.globe.zoom = gz;
                self.animating = false;
            }
            _ => {}
        }
        self.view = view;
    }

    pub fn zoom_in(&mut self) {
        if self.zoom < 20 { self.zoom += 1; }
    }

    pub fn zoom_out(&mut self) {
        if self.zoom > 0 { self.zoom -= 1; }
    }

    /// Globe zoom in (W key).
    pub fn globe_zoom_in(&mut self) {
        self.globe.zoom = (self.globe.zoom * 1.25).min(8.0);
    }

    /// Globe zoom out (S key).
    pub fn globe_zoom_out(&mut self) {
        self.globe.zoom = (self.globe.zoom / 1.25).max(0.5);
    }

    pub fn pan(&mut self, dlat: f64, dlon: f64) {
        self.map_centre = LatLon::new(
            self.map_centre.lat + dlat,
            self.map_centre.lon + dlon,
        );
    }

    /// Advance globe animation by `delta_secs`.
    pub fn tick(&mut self, delta_secs: f64) {
        if self.animating {
            const RPM: f64 = 4.0;
            self.globe.rot_y += 2.0 * std::f64::consts::PI * (RPM / 60.0) * delta_secs;
            self.globe.rot_y %= 2.0 * std::f64::consts::PI;
        }
        self.globe_cursor = self.globe_centre();
    }

    /// The geographic coordinate currently at the centre of the globe view.
    pub fn globe_centre(&self) -> LatLon {
        let (hx, hy, hz) = (0.0_f64, 0.0_f64, -1.0_f64);
        let a = -self.globe.rot_x;
        let (c, s) = (a.cos(), a.sin());
        let (wx, wy, wz) = (hx, hy * c - hz * s, hy * s + hz * c);
        let a = -self.globe.rot_y;
        let (c, s) = (a.cos(), a.sin());
        let (wx, _wy, wz) = (wx * c + wz * s, wy, -wx * s + wz * c);
        let lat = wy.clamp(-1.0, 1.0).asin().to_degrees();
        let lon = wx.atan2(-wz).to_degrees();
        LatLon::new(lat, lon)
    }

    pub fn viewport_extent(&self, cols: u16, rows: u16) -> (f64, f64) {
        self.resolution.viewport_extent_deg(
            cols, rows, self.zoom, self.map_centre.lat,
        )
    }

    pub fn viewport_bbox(&self, cols: u16, rows: u16) -> BoundingBox {
        let (lon_ext, lat_ext) = self.viewport_extent(cols, rows);
        BoundingBox::new(
            self.map_centre.lat - lat_ext / 2.0,
            self.map_centre.lon - lon_ext / 2.0,
            self.map_centre.lat + lat_ext / 2.0,
            self.map_centre.lon + lon_ext / 2.0,
        )
    }
}

// ── Application view / screen ─────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Splash,
    Menu,
    Globe,
    Map,
    MarkerList,
    ZoomExplorer,
    Diagnostics,
}
