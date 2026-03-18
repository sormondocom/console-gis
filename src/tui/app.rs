use std::path::PathBuf;
use serde::{Deserialize, Serialize};

use crate::geo::{BoundingBox, ConsoleResolution, LatLon};
use crate::geo::zoom::RenderMode;
use crate::render::canvas::TerminalCapability;
use crate::render::globe::GlobeParams;
use crate::data::{WorldMap, MarkerStore, GeoLayer, TopoMap};
use crate::data::geojson::GeoGeometry;

// ── Layer entry ───────────────────────────────────────────────────────────────

/// A GeoJSON layer plus display metadata owned by the application.
pub struct LayerEntry {
    pub layer:       GeoLayer,
    pub visible:     bool,
    /// Display name shown in the layer manager (defaults to filename stem).
    pub label:       String,
    /// Index (0–4) into the 5-colour palette, locked when the layer is added.
    pub color_index: u8,
}

impl LayerEntry {
    pub fn new(layer: GeoLayer, color_index: u8) -> Self {
        let label = file_stem(&layer.source);
        Self { layer, visible: true, label, color_index }
    }

    pub fn with_label(layer: GeoLayer, label: impl Into<String>, color_index: u8) -> Self {
        Self { layer, visible: true, label: label.into(), color_index }
    }
}

/// Extract the filename stem (no directory, no `.geojson`/`.json` extension).
fn file_stem(path: &str) -> String {
    let base = path.split(['/', '\\']).last().unwrap_or(path);
    base.strip_suffix(".geojson")
        .or_else(|| base.strip_suffix(".json"))
        .unwrap_or(base)
        .to_string()
}

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

/// Per-layer metadata saved across sessions (replaces the old `layer_paths`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SavedLayerEntry {
    pub path:  String,
    #[serde(default = "bool_true")]
    pub visible: bool,
    #[serde(default)]
    pub label:   String,
    #[serde(default)]
    pub color_index: u8,
}

fn bool_true() -> bool { true }

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
    /// Absolute paths of GeoJSON files that were loaded last session (legacy).
    #[serde(default)]
    pub layer_paths:  Vec<String>,
    /// Per-layer metadata saved across sessions (replaces the old `layer_paths`).
    #[serde(default)]
    pub layer_entries: Vec<SavedLayerEntry>,
    /// Named saved view positions.
    #[serde(default)]
    pub bookmarks:    Vec<Bookmark>,
}

impl Default for SavedState {
    fn default() -> Self {
        Self {
            map_lat:      20.0,
            map_lon:      10.0,
            map_zoom:     2,
            globe_rot_y:  0.0,
            globe_rot_x:  0.0,
            globe_zoom:   1.0,
            layer_paths:  Vec::new(),
            layer_entries: Vec::new(),
            bookmarks:    Vec::new(),
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
    /// Topographic elevation layer on/off.  On by default.
    pub topo_enabled: bool,
    /// Shared topographic elevation data (zero allocation after construction).
    pub topo:         TopoMap,
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
    pub geo_layers:     Vec<LayerEntry>,
    /// True when the GeoJSON file-path input overlay is active.
    pub importing:      bool,
    /// Selected row in the layer-manager view.
    pub layer_list_sel:  usize,
    /// View to return to when leaving the layer-manager (Globe or Map).
    pub layers_prev_view: View,
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

    // ── Calculator ────────────────────────────────────────────────────────────
    /// Calculator view state.
    pub calc: CalcState,

    // ── Shape editor ──────────────────────────────────────────────────────────
    /// Interactive shape builder and GeoJSON exporter.
    pub shape_editor: ShapeEditorState,

    // ── Layer info overlay ────────────────────────────────────────────────────
    /// When true, show the GeoJSON breakdown overlay in the Layers view.
    pub layer_info: bool,

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
            topo_enabled:   true,
            topo:           TopoMap::new(),
            markers,
            placing_marker:     false,
            globe_cursor:       LatLon::new(0.0, 0.0),
            marker_input:       None,
            marker_list_sel:    0,
            marker_del_confirm: false,
            geo_layers:         Vec::new(),
            importing:          false,
            layer_list_sel:     0,
            layers_prev_view:   View::Menu,
            import_buf:         String::new(),
            import_error:       None,
            clearing_markers: false,
            bookmarking:      false,
            bookmark_buf:     String::new(),
            calc:             CalcState::new(),
            shape_editor:     ShapeEditorState::new(),
            layer_info:       false,
            state_path,
        }
    }

    /// Restore GeoJSON layers from a previous session.
    ///
    /// Missing or unreadable files produce a warning entry in `import_error`
    /// rather than a hard failure.  The returned vec contains one warning
    /// string per failed path.
    pub fn restore_layers(&mut self, saved: &SavedState) -> Vec<String> {
        let mut warnings = Vec::new();
        if !saved.layer_entries.is_empty() {
            // New format: load from layer_entries
            for entry in &saved.layer_entries {
                let p = PathBuf::from(&entry.path);
                if !p.exists() {
                    warnings.push(format!(
                        "GeoJSON not found (skipped): {}",
                        p.display()
                    ));
                    continue;
                }
                match GeoLayer::load(&p) {
                    Ok(layer) => {
                        let label = if entry.label.is_empty() {
                            file_stem(&layer.source)
                        } else {
                            entry.label.clone()
                        };
                        self.geo_layers.push(LayerEntry {
                            layer,
                            visible:     entry.visible,
                            label,
                            color_index: entry.color_index,
                        });
                    }
                    Err(e) => warnings.push(format!(
                        "Could not load {} — {}",
                        p.display(), e
                    )),
                }
            }
        } else {
            // Legacy format: load from layer_paths with sequential color_index
            for (idx, path_str) in saved.layer_paths.iter().enumerate() {
                let p = PathBuf::from(path_str);
                if !p.exists() {
                    warnings.push(format!(
                        "GeoJSON not found (skipped): {}",
                        p.display()
                    ));
                    continue;
                }
                match GeoLayer::load(&p) {
                    Ok(layer) => {
                        let color_index = idx as u8 % 5;
                        self.geo_layers.push(LayerEntry::new(layer, color_index));
                    }
                    Err(e) => warnings.push(format!(
                        "Could not load {} — {}",
                        p.display(), e
                    )),
                }
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
                let color_index = self.geo_layers.len() as u8 % 5;
                self.geo_layers.push(LayerEntry::new(layer, color_index));
                true
            }
            Err(e) => {
                self.import_error = Some(format!("Load error: {e}"));
                false
            }
        }
    }

    /// Import a GeoJSON file split into one sub-layer per geometry type.
    /// Returns true on success. On failure sets import_error.
    pub fn load_geo_layer_split(&mut self, path: &str) -> bool {
        let p = PathBuf::from(path.trim());
        if !p.exists() {
            self.import_error = Some(format!("File not found: {}", p.display()));
            return false;
        }
        match GeoLayer::load(&p) {
            Ok(base_layer) => {
                let stem = file_stem(&base_layer.source);
                let sub_layers = base_layer.split_by_geometry_type();
                // Determine type suffix label for each sub-layer
                let type_labels: Vec<&str> = sub_layers.iter().map(|sl| {
                    if sl.features.iter().all(|f| matches!(
                        f.geometry, GeoGeometry::Point(_) | GeoGeometry::MultiPoint(_)
                    )) { "Points" }
                    else if sl.features.iter().all(|f| matches!(
                        f.geometry, GeoGeometry::Polygon(_) | GeoGeometry::MultiPolygon(_)
                    )) { "Polygons" }
                    else { "Lines" }
                }).collect();
                for (sl, type_label) in sub_layers.into_iter().zip(type_labels) {
                    let color_index = self.geo_layers.len() as u8 % 5;
                    let label = format!("{} ({})", stem, type_label);
                    self.geo_layers.push(LayerEntry::with_label(sl, label, color_index));
                }
                self.import_error = None;
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
            map_lat:      self.map_centre.lat,
            map_lon:      self.map_centre.lon,
            map_zoom:     self.zoom,
            globe_rot_y:  self.globe.rot_y,
            globe_rot_x:  self.globe.rot_x,
            globe_zoom:   self.globe.zoom,
            layer_paths:  Vec::new(),   // legacy field; layer_entries is canonical
            layer_entries: self.geo_layers.iter().map(|e| SavedLayerEntry {
                path:        e.layer.source.clone(),
                visible:     e.visible,
                label:       e.label.clone(),
                color_index: e.color_index,
            }).collect(),
            bookmarks:    existing.bookmarks,  // preserve bookmarks across sessions
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
    Layers,
    Calculator,
    ShapeEditor,
}

// ── Shape editor state ─────────────────────────────────────────────────────────

/// Geometry type being constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeType {
    Point,
    MultiPoint,
    LineString,
    MultiLineString,
    Polygon,
    MultiPolygon,
}

impl ShapeType {
    pub const ALL: &'static [ShapeType] = &[
        ShapeType::Point,
        ShapeType::MultiPoint,
        ShapeType::LineString,
        ShapeType::MultiLineString,
        ShapeType::Polygon,
        ShapeType::MultiPolygon,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ShapeType::Point           => "Point",
            ShapeType::MultiPoint      => "MultiPoint",
            ShapeType::LineString      => "LineString",
            ShapeType::MultiLineString => "MultiLineString",
            ShapeType::Polygon         => "Polygon",
            ShapeType::MultiPolygon    => "MultiPolygon",
        }
    }

    pub fn key(self) -> char {
        match self {
            ShapeType::Point           => '1',
            ShapeType::MultiPoint      => '2',
            ShapeType::LineString      => '3',
            ShapeType::MultiLineString => '4',
            ShapeType::Polygon         => '5',
            ShapeType::MultiPolygon    => '6',
        }
    }

    /// Minimum coordinates required to form a valid geometry.
    pub fn min_coords_per_part(self) -> usize {
        match self {
            ShapeType::Point | ShapeType::MultiPoint  => 1,
            ShapeType::LineString | ShapeType::MultiLineString => 2,
            ShapeType::Polygon | ShapeType::MultiPolygon       => 3,
        }
    }

    /// Whether the type supports multiple parts (F to finish part).
    pub fn is_multi(self) -> bool {
        matches!(self,
            ShapeType::MultiPoint | ShapeType::MultiLineString | ShapeType::MultiPolygon)
    }

    pub fn hint(self) -> &'static str {
        match self {
            ShapeType::Point           => "Enter one coordinate, then N to continue.",
            ShapeType::MultiPoint      => "Add points. F=finish adding · N=next step",
            ShapeType::LineString      => "Add ≥2 coords for the line. N=next step",
            ShapeType::MultiLineString => "Add coords. F=finish line · N=done adding lines",
            ShapeType::Polygon         => "Add ≥3 coords for the ring. N=next step",
            ShapeType::MultiPolygon    => "Add coords. F=finish polygon · N=done",
        }
    }
}

/// Which step the shape editor is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeEditorStep {
    SelectType,
    AddVertices,
    EnterName,
    EnterExportPath,
}

/// All mutable state for the interactive shape editor.
pub struct ShapeEditorState {
    pub step:        ShapeEditorStep,
    pub type_idx:    usize,
    // ── Coordinate input ──────────────────────────────────────────────────────
    pub lat_buf:     String,
    pub lon_buf:     String,
    /// 0 = lat field focused, 1 = lon field focused.
    pub coord_field: usize,
    // ── Accumulated geometry ──────────────────────────────────────────────────
    /// Finalized parts (for multi-geometries each F press appends one here).
    pub parts:       Vec<Vec<(f64, f64)>>,
    /// Current part still being edited.
    pub current:     Vec<(f64, f64)>,
    /// Scroll offset for the vertex list display.
    pub vert_scroll: usize,
    // ── Name / export ─────────────────────────────────────────────────────────
    pub name_buf:    String,
    pub export_buf:  String,
    /// Feedback from the last export attempt.
    pub message:     Option<String>,
}

impl ShapeEditorState {
    pub fn new() -> Self {
        Self {
            step:        ShapeEditorStep::SelectType,
            type_idx:    0,
            lat_buf:     String::new(),
            lon_buf:     String::new(),
            coord_field: 0,
            parts:       Vec::new(),
            current:     Vec::new(),
            vert_scroll: 0,
            name_buf:    String::new(),
            export_buf:  String::new(),
            message:     None,
        }
    }

    pub fn current_type(&self) -> ShapeType { ShapeType::ALL[self.type_idx] }

    /// Reset to a fresh state for re-use.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Total accumulated coordinate count across all parts + current.
    pub fn total_coords(&self) -> usize {
        self.parts.iter().map(|p| p.len()).sum::<usize>() + self.current.len()
    }

    /// Try to parse lat/lon bufs and add a vertex to `current`.
    /// Returns an error string on parse failure.
    pub fn commit_vertex(&mut self) -> Result<(), String> {
        let lat = self.lat_buf.trim().parse::<f64>()
            .map_err(|_| format!("Invalid lat: \"{}\"", self.lat_buf))?;
        let lon = self.lon_buf.trim().parse::<f64>()
            .map_err(|_| format!("Invalid lon: \"{}\"", self.lon_buf))?;
        if !(-90.0..=90.0).contains(&lat) {
            return Err(format!("Latitude {lat} out of range −90…90"));
        }
        if !(-180.0..=180.0).contains(&lon) {
            return Err(format!("Longitude {lon} out of range −180…180"));
        }
        self.current.push((lat, lon));
        self.lat_buf.clear();
        self.lon_buf.clear();
        self.coord_field = 0;
        self.message = None;
        // Auto-scroll vertex list to bottom
        self.vert_scroll = self.total_coords().saturating_sub(1);
        Ok(())
    }

    /// Finish the current part and start a new one (multi-geometry types only).
    pub fn finish_part(&mut self) -> Result<(), String> {
        let min = self.current_type().min_coords_per_part();
        if self.current.len() < min {
            return Err(format!(
                "Need ≥{min} coordinate{} to finish a {}.",
                if min == 1 { "" } else { "s" },
                self.current_type().name(),
            ));
        }
        self.parts.push(std::mem::take(&mut self.current));
        self.message = None;
        Ok(())
    }

    /// Remove the last coordinate (from `current`, or from the last part).
    pub fn undo_vertex(&mut self) {
        if self.current.pop().is_none() {
            if let Some(last) = self.parts.last_mut() {
                last.pop();
                if last.is_empty() { self.parts.pop(); }
            }
        }
        self.vert_scroll = self.total_coords().saturating_sub(1);
        self.message = None;
    }

    /// All coords in display order: finalized parts + current.
    pub fn all_coords(&self) -> Vec<(f64, f64)> {
        let mut v = Vec::with_capacity(self.total_coords());
        for p in &self.parts { v.extend_from_slice(p); }
        v.extend_from_slice(&self.current);
        v
    }

    /// Validate and serialize to a GeoJSON FeatureCollection string.
    pub fn to_geojson(&self) -> Result<String, String> {
        let geom = self.build_geometry()?;
        let name = self.name_buf.trim();
        let fc = serde_json::json!({
            "type": "FeatureCollection",
            "features": [{
                "type": "Feature",
                "geometry": geom,
                "properties": if name.is_empty() {
                    serde_json::json!({})
                } else {
                    serde_json::json!({ "name": name })
                }
            }]
        });
        serde_json::to_string_pretty(&fc).map_err(|e| e.to_string())
    }

    /// Write GeoJSON to `self.export_buf` path.
    pub fn export(&mut self) {
        let path = self.export_buf.trim().to_string();
        if path.is_empty() {
            self.message = Some("Enter a file path first.".into());
            return;
        }
        match self.to_geojson() {
            Err(e) => { self.message = Some(format!("Build error: {e}")); }
            Ok(json) => {
                match std::fs::write(&path, &json) {
                    Ok(_) => {
                        self.message = Some(format!(
                            "Saved {} bytes → {path}",
                            json.len()
                        ));
                    }
                    Err(e) => { self.message = Some(format!("Write error: {e}")); }
                }
            }
        }
    }

    fn build_geometry(&self) -> Result<serde_json::Value, String> {
        use serde_json::json;
        // Merge finalized parts + any remaining current
        let mut all_parts = self.parts.clone();
        if !self.current.is_empty() {
            all_parts.push(self.current.clone());
        }

        fn coord_to_json(lat: f64, lon: f64) -> serde_json::Value {
            json!([lon, lat])   // GeoJSON is [lon, lat]
        }
        fn ring_to_json(pts: &[(f64, f64)]) -> serde_json::Value {
            let mut v: Vec<_> = pts.iter().map(|(la, lo)| coord_to_json(*la, *lo)).collect();
            // Close polygon ring if needed
            if let (Some(first), Some(last)) = (pts.first(), pts.last()) {
                if first != last { v.push(coord_to_json(first.0, first.1)); }
            }
            json!(v)
        }

        let min = self.current_type().min_coords_per_part();
        let total: usize = all_parts.iter().map(|p| p.len()).sum();
        if total < min {
            return Err(format!(
                "Need ≥{min} coordinate{} for {}.",
                if min == 1 { "" } else { "s" },
                self.current_type().name(),
            ));
        }

        let geom = match self.current_type() {
            ShapeType::Point => {
                let (la, lo) = all_parts[0][0];
                json!({ "type": "Point", "coordinates": coord_to_json(la, lo) })
            }
            ShapeType::MultiPoint => {
                let coords: Vec<_> = all_parts.iter()
                    .flat_map(|p| p.iter())
                    .map(|(la, lo)| coord_to_json(*la, *lo))
                    .collect();
                json!({ "type": "MultiPoint", "coordinates": coords })
            }
            ShapeType::LineString => {
                let coords: Vec<_> = all_parts.iter()
                    .flat_map(|p| p.iter())
                    .map(|(la, lo)| coord_to_json(*la, *lo))
                    .collect();
                json!({ "type": "LineString", "coordinates": coords })
            }
            ShapeType::MultiLineString => {
                let lines: Vec<_> = all_parts.iter()
                    .filter(|p| p.len() >= 2)
                    .map(|p| {
                        let coords: Vec<_> = p.iter().map(|(la, lo)| coord_to_json(*la, *lo)).collect();
                        json!(coords)
                    })
                    .collect();
                if lines.is_empty() { return Err("Need ≥1 finished line with ≥2 points.".into()); }
                json!({ "type": "MultiLineString", "coordinates": lines })
            }
            ShapeType::Polygon => {
                // First part = outer ring; additional parts = holes (not yet supported in UI but structurally correct)
                let rings: Vec<_> = all_parts.iter()
                    .filter(|p| p.len() >= 3)
                    .map(|p| ring_to_json(p))
                    .collect();
                if rings.is_empty() { return Err("Need ≥3 coordinates for a polygon ring.".into()); }
                json!({ "type": "Polygon", "coordinates": rings })
            }
            ShapeType::MultiPolygon => {
                let polys: Vec<_> = all_parts.iter()
                    .filter(|p| p.len() >= 3)
                    .map(|p| json!([ring_to_json(p)]))
                    .collect();
                if polys.is_empty() { return Err("Need ≥1 polygon with ≥3 coordinates.".into()); }
                json!({ "type": "MultiPolygon", "coordinates": polys })
            }
        };
        Ok(geom)
    }
}

// ── Calculator state ───────────────────────────────────────────────────────────

/// The nine built-in calculators, in menu order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalcMode {
    LatLonToTile,
    TileToLatLon,
    Wgs84ToMercator,
    MercatorToWgs84,
    DdToDms,
    DmsToDD,
    Distance,
    Bearing,
    DestinationPoint,
}

impl CalcMode {
    pub const ALL: &'static [CalcMode] = &[
        CalcMode::LatLonToTile,
        CalcMode::TileToLatLon,
        CalcMode::Wgs84ToMercator,
        CalcMode::MercatorToWgs84,
        CalcMode::DdToDms,
        CalcMode::DmsToDD,
        CalcMode::Distance,
        CalcMode::Bearing,
        CalcMode::DestinationPoint,
    ];

    pub fn name(self) -> &'static str {
        match self {
            CalcMode::LatLonToTile     => "Lat/Lon → Slippy Tile",
            CalcMode::TileToLatLon     => "Slippy Tile → Lat/Lon",
            CalcMode::Wgs84ToMercator  => "WGS-84 → Web Mercator",
            CalcMode::MercatorToWgs84  => "Web Mercator → WGS-84",
            CalcMode::DdToDms          => "DD → DMS / DDM",
            CalcMode::DmsToDD          => "DMS → Decimal Degrees",
            CalcMode::Distance         => "Haversine Distance",
            CalcMode::Bearing          => "Bearing",
            CalcMode::DestinationPoint => "Destination Point",
        }
    }

    pub fn key(self) -> char {
        match self {
            CalcMode::LatLonToTile     => '1',
            CalcMode::TileToLatLon     => '2',
            CalcMode::Wgs84ToMercator  => '3',
            CalcMode::MercatorToWgs84  => '4',
            CalcMode::DdToDms          => '5',
            CalcMode::DmsToDD          => '6',
            CalcMode::Distance         => '7',
            CalcMode::Bearing          => '8',
            CalcMode::DestinationPoint => '9',
        }
    }

    pub fn field_labels(self) -> &'static [&'static str] {
        match self {
            CalcMode::LatLonToTile     => &["Latitude", "Longitude", "Zoom (0-20)"],
            CalcMode::TileToLatLon     => &["Tile X", "Tile Y", "Zoom (0-20)"],
            CalcMode::Wgs84ToMercator  => &["Latitude", "Longitude"],
            CalcMode::MercatorToWgs84  => &["X (metres)", "Y (metres)"],
            CalcMode::DdToDms          => &["Latitude (DD)", "Longitude (DD)"],
            CalcMode::DmsToDD          => &["Lat deg", "Lat min", "Lat sec", "N or S",
                                            "Lon deg", "Lon min", "Lon sec", "E or W"],
            CalcMode::Distance         => &["Lat 1", "Lon 1", "Lat 2", "Lon 2"],
            CalcMode::Bearing          => &["Lat 1", "Lon 1", "Lat 2", "Lon 2"],
            CalcMode::DestinationPoint => &["Latitude", "Longitude", "Bearing (°)", "Distance (m)"],
        }
    }
}

/// Computed result from a calculator.
pub struct CalcResult {
    /// Lines of formatted output.
    pub lines: Vec<String>,
    /// Geographic point embedded in the result (enables place/go-to actions).
    pub latlon: Option<(f64, f64)>,
}

/// Mutable state for the calculator view.
pub struct CalcState {
    /// Index into `CalcMode::ALL` for the currently selected calculator.
    pub mode_idx:    usize,
    /// Per-field text buffers.
    pub fields:      Vec<String>,
    /// Which field index has focus (when `focus_right` is true).
    pub field_idx:   usize,
    /// If true the right panel (inputs) has keyboard focus; left panel otherwise.
    pub focus_right: bool,
    /// Most recent successful result.
    pub result:      Option<CalcResult>,
    /// Error message from the last failed compute.
    pub error:       Option<String>,
}

impl CalcState {
    pub fn new() -> Self {
        let mut s = Self {
            mode_idx:    0,
            fields:      Vec::new(),
            field_idx:   0,
            focus_right: false,
            result:      None,
            error:       None,
        };
        s.reset_fields();
        s
    }

    pub fn current_mode(&self) -> CalcMode {
        CalcMode::ALL[self.mode_idx]
    }

    pub fn set_mode(&mut self, idx: usize) {
        if idx < CalcMode::ALL.len() {
            self.mode_idx = idx;
            self.reset_fields();
        }
    }

    fn reset_fields(&mut self) {
        let n = self.current_mode().field_labels().len();
        self.fields   = vec![String::new(); n];
        self.field_idx = 0;
        self.result    = None;
        self.error     = None;
    }

    fn pf(s: &str) -> Option<f64>  { s.trim().parse().ok() }
    fn pu(s: &str) -> Option<u32>  { s.trim().parse().ok() }
    fn p8(s: &str) -> Option<u8>   { s.trim().parse().ok() }

    /// Parse hemisphere letter: 'N'/'E' → false (positive), 'S'/'W' → true (negative).
    fn hem(s: &str) -> Option<bool> {
        match s.trim().to_ascii_uppercase().as_str() {
            "N" | "E" => Some(false),
            "S" | "W" => Some(true),
            _          => None,
        }
    }

    /// Run the selected calculator against the current field values.
    pub fn compute(&mut self) {
        use crate::geo::calc::*;
        self.error  = None;
        self.result = None;
        let f = self.fields.clone();

        match self.current_mode() {
            CalcMode::LatLonToTile => {
                match (Self::pf(&f[0]), Self::pf(&f[1]), Self::p8(&f[2])) {
                    (Some(lat), Some(lon), Some(z)) if z <= 20 => {
                        let (tx, ty) = latlon_to_tile(lat, lon, z);
                        let (s, w, n, e) = tile_bbox(tx, ty, z);
                        self.result = Some(CalcResult {
                            lines: vec![
                                format!("Tile X / Y:  {tx} / {ty}"),
                                format!("URL path:    {z}/{tx}/{ty}"),
                                String::new(),
                                format!("Bbox N: {n:.6}°   S: {s:.6}°"),
                                format!("Bbox E: {e:.6}°   W: {w:.6}°"),
                            ],
                            latlon: Some((lat, lon)),
                        });
                    }
                    _ => self.error = Some("Need valid lat, lon, and zoom 0–20.".into()),
                }
            }

            CalcMode::TileToLatLon => {
                match (Self::pu(&f[0]), Self::pu(&f[1]), Self::p8(&f[2])) {
                    (Some(tx), Some(ty), Some(z)) if z <= 20 => {
                        let (north, west) = tile_to_latlon_nw(tx, ty, z);
                        let (south, east) = tile_to_latlon_nw(tx + 1, ty + 1, z);
                        let clat = (north + south) / 2.0;
                        let clon = (west  + east)  / 2.0;
                        self.result = Some(CalcResult {
                            lines: vec![
                                format!("NW:     {north:.6}°N  {:.6}°{}", west.abs(), if west >= 0.0 { 'E' } else { 'W' }),
                                format!("SE:     {:.6}°{}  {:.6}°{}", south.abs(), if south >= 0.0 { 'N' } else { 'S' }, east.abs(), if east >= 0.0 { 'E' } else { 'W' }),
                                format!("Center: {:.6}°{}  {:.6}°{}", clat.abs(), if clat >= 0.0 { 'N' } else { 'S' }, clon.abs(), if clon >= 0.0 { 'E' } else { 'W' }),
                                String::new(),
                                format!("Bbox:   {south:.6}, {west:.6}, {north:.6}, {east:.6}"),
                            ],
                            latlon: Some((clat, clon)),
                        });
                    }
                    _ => self.error = Some("Need valid tile X, Y, and zoom 0–20.".into()),
                }
            }

            CalcMode::Wgs84ToMercator => {
                match (Self::pf(&f[0]), Self::pf(&f[1])) {
                    (Some(lat), Some(lon)) => {
                        let (xm, ym) = wgs84_to_mercator(lat, lon);
                        self.result = Some(CalcResult {
                            lines: vec![
                                format!("X (Easting):  {xm:.3} m"),
                                format!("Y (Northing): {ym:.3} m"),
                                String::new(),
                                format!("EPSG:3857  ({xm:.1}, {ym:.1})"),
                            ],
                            latlon: Some((lat, lon)),
                        });
                    }
                    _ => self.error = Some("Need valid decimal lat and lon.".into()),
                }
            }

            CalcMode::MercatorToWgs84 => {
                match (Self::pf(&f[0]), Self::pf(&f[1])) {
                    (Some(xm), Some(ym)) => {
                        let (lat, lon) = mercator_to_wgs84(xm, ym);
                        let (ld, lm, ls, ldir) = dd_to_dms(lat, true);
                        let (od, om, os, odir) = dd_to_dms(lon, false);
                        self.result = Some(CalcResult {
                            lines: vec![
                                format!("Latitude:  {lat:.8}°"),
                                format!("Longitude: {lon:.8}°"),
                                String::new(),
                                format!("DMS: {}°{:02}'{:06.3}\"{}  {}°{:02}'{:06.3}\"{}",
                                    ld, lm, ls, ldir, od, om, os, odir),
                            ],
                            latlon: Some((lat, lon)),
                        });
                    }
                    _ => self.error = Some("Need valid X and Y in metres.".into()),
                }
            }

            CalcMode::DdToDms => {
                match (Self::pf(&f[0]), Self::pf(&f[1])) {
                    (Some(lat), Some(lon)) => {
                        let (ld, lm, ls, ldir) = dd_to_dms(lat, true);
                        let (od, om, os, odir) = dd_to_dms(lon, false);
                        let (ld2, ldm, ldir2)  = dd_to_ddm(lat, true);
                        let (od2, odm, odir2)  = dd_to_ddm(lon, false);
                        self.result = Some(CalcResult {
                            lines: vec![
                                format!("DMS Lat:  {}°{:02}'{:06.3}\"{}",  ld,  lm,  ls,  ldir),
                                format!("DMS Lon:  {}°{:02}'{:06.3}\"{}",  od,  om,  os,  odir),
                                String::new(),
                                format!("DDM Lat:  {}°{:09.6}′{}", ld2, ldm, ldir2),
                                format!("DDM Lon:  {}°{:09.6}′{}", od2, odm, odir2),
                            ],
                            latlon: Some((lat, lon)),
                        });
                    }
                    _ => self.error = Some("Enter decimal degrees for lat and lon.".into()),
                }
            }

            CalcMode::DmsToDD => {
                let (lat_d, lat_m, lat_s, lat_h) =
                    (Self::pf(&f[0]), Self::pf(&f[1]), Self::pf(&f[2]), Self::hem(&f[3]));
                let (lon_d, lon_m, lon_s, lon_h) =
                    (Self::pf(&f[4]), Self::pf(&f[5]), Self::pf(&f[6]), Self::hem(&f[7]));
                match (lat_d, lat_m, lat_s, lat_h, lon_d, lon_m, lon_s, lon_h) {
                    (Some(ld), Some(lm), Some(ls), Some(ln),
                     Some(od), Some(om), Some(os), Some(on)) => {
                        let lat = dms_to_dd(ld, lm, ls, ln);
                        let lon = dms_to_dd(od, om, os, on);
                        self.result = Some(CalcResult {
                            lines: vec![
                                format!("Latitude:  {lat:.8}°"),
                                format!("Longitude: {lon:.8}°"),
                                String::new(),
                                format!("WGS-84: {lat:.6}, {lon:.6}"),
                            ],
                            latlon: Some((lat, lon)),
                        });
                    }
                    _ => self.error = Some("Fill D M S and hemisphere (N/S or E/W) for both.".into()),
                }
            }

            CalcMode::Distance => {
                match (Self::pf(&f[0]), Self::pf(&f[1]), Self::pf(&f[2]), Self::pf(&f[3])) {
                    (Some(la1), Some(lo1), Some(la2), Some(lo2)) => {
                        let m  = haversine_m(la1, lo1, la2, lo2);
                        let km = m / 1_000.0;
                        let mi = m / 1_609.344;
                        let nm = m / 1_852.0;
                        self.result = Some(CalcResult {
                            lines: vec![
                                format!("Distance: {m:.1} m"),
                                format!("          {km:.3} km"),
                                format!("          {mi:.3} mi"),
                                format!("          {nm:.3} NM"),
                            ],
                            latlon: None,
                        });
                    }
                    _ => self.error = Some("Need four decimal-degree values.".into()),
                }
            }

            CalcMode::Bearing => {
                match (Self::pf(&f[0]), Self::pf(&f[1]), Self::pf(&f[2]), Self::pf(&f[3])) {
                    (Some(la1), Some(lo1), Some(la2), Some(lo2)) => {
                        let fwd = bearing_deg(la1, lo1, la2, lo2);
                        let rev = (fwd + 180.0) % 360.0;
                        let dir = compass_dir(fwd);
                        self.result = Some(CalcResult {
                            lines: vec![
                                format!("Forward: {fwd:.2}°  ({dir})"),
                                format!("Reverse: {rev:.2}°  ({})", compass_dir(rev)),
                            ],
                            latlon: None,
                        });
                    }
                    _ => self.error = Some("Need four decimal-degree values.".into()),
                }
            }

            CalcMode::DestinationPoint => {
                match (Self::pf(&f[0]), Self::pf(&f[1]), Self::pf(&f[2]), Self::pf(&f[3])) {
                    (Some(lat), Some(lon), Some(brng), Some(dist)) => {
                        let (lat2, lon2) = destination_point(lat, lon, brng, dist);
                        let (ld, lm, ls, ldir) = dd_to_dms(lat2, true);
                        let (od, om, os, odir) = dd_to_dms(lon2, false);
                        self.result = Some(CalcResult {
                            lines: vec![
                                format!("Destination: {lat2:.8}°, {lon2:.8}°"),
                                String::new(),
                                format!("DMS: {}°{:02}'{:06.3}\"{}  {}°{:02}'{:06.3}\"{}",
                                    ld, lm, ls, ldir, od, om, os, odir),
                            ],
                            latlon: Some((lat2, lon2)),
                        });
                    }
                    _ => self.error = Some("Need lat, lon, bearing (°), distance (m).".into()),
                }
            }
        }
    }
}
