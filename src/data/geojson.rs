/// GeoJSON importer for console-gis.
///
/// Supports the full GeoJSON specification (RFC 7946):
///   - FeatureCollection, Feature, bare geometries
///   - Point, MultiPoint, LineString, MultiLineString
///   - Polygon, MultiPolygon, GeometryCollection
///
/// Coordinates are always [longitude, latitude, optional_elevation] per the spec.
///
/// # Rendering tiers
///
/// | Geometry type       | 2-D map            | 3-D globe              |
/// |---------------------|--------------------|------------------------|
/// | Point / MultiPoint  | Symbol at position | Projected dot          |
/// | LineString / Multi  | Bresenham segments | Sampled arc segments   |
/// | Polygon / Multi     | Boundary lines     | Visible boundary arcs  |
/// | GeometryCollection  | Each child         | Each child             |
///
/// # Usage
///
/// ```rust,no_run
/// use console_gis::data::geojson::GeoLayer;
/// let layer = GeoLayer::load("cities.geojson").unwrap();
/// println!("{} features loaded", layer.features.len());
/// ```

use std::path::Path;
use anyhow::{anyhow, Context};
use crate::data::MarkerStore;

// ── Public types ──────────────────────────────────────────────────────────────

/// A (longitude, latitude) coordinate pair in WGS-84 decimal degrees.
pub type Coord = (f64, f64);

/// All GeoJSON geometry variants.
#[derive(Debug, Clone)]
pub enum GeoGeometry {
    Point(Coord),
    MultiPoint(Vec<Coord>),
    LineString(Vec<Coord>),
    MultiLineString(Vec<Vec<Coord>>),
    /// Rings: outer boundary first, then optional interior holes.
    Polygon(Vec<Vec<Coord>>),
    MultiPolygon(Vec<Vec<Vec<Coord>>>),
    /// Mixed collection of geometries.
    Collection(Vec<GeoGeometry>),
}

/// A single GeoJSON feature with geometry and optional properties.
#[derive(Debug, Clone)]
pub struct GeoFeature {
    pub geometry:   GeoGeometry,
    /// Best-effort label: `properties.name` → `properties.title` → `properties.id` → `""`.
    pub name:       String,
    pub properties: serde_json::Map<String, serde_json::Value>,
}

/// An imported GeoJSON file, normalised to a flat feature list.
#[derive(Debug, Clone, Default)]
pub struct GeoLayer {
    /// Source file name (for display in the status bar).
    pub source:   String,
    pub features: Vec<GeoFeature>,
}

impl GeoLayer {
    /// Load a GeoJSON file from `path`.
    pub fn load<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let text  = std::fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        let val: serde_json::Value = serde_json::from_str(&text)
            .with_context(|| format!("parsing JSON in {}", path.display()))?;

        // Store the canonicalised absolute path so session-restore works even
        // when the working directory changes between runs.
        let source = std::fs::canonicalize(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .to_string();

        let mut features = Vec::new();
        collect_features(&val, &serde_json::Map::new(), &mut features)?;

        Ok(GeoLayer { source, features })
    }

    /// Iterate over point-like features (Point and MultiPoint).
    pub fn points(&self) -> impl Iterator<Item = &GeoFeature> {
        self.features.iter().filter(|f| {
            matches!(f.geometry, GeoGeometry::Point(_) | GeoGeometry::MultiPoint(_))
        })
    }

    /// Iterate over line-like features (LineString and MultiLineString).
    pub fn lines(&self) -> impl Iterator<Item = &GeoFeature> {
        self.features.iter().filter(|f| {
            matches!(f.geometry,
                GeoGeometry::LineString(_) | GeoGeometry::MultiLineString(_))
        })
    }

    /// Iterate over polygon features.
    pub fn polygons(&self) -> impl Iterator<Item = &GeoFeature> {
        self.features.iter().filter(|f| {
            matches!(f.geometry, GeoGeometry::Polygon(_) | GeoGeometry::MultiPolygon(_))
        })
    }

    /// Import all Point / MultiPoint features into the given [`MarkerStore`].
    ///
    /// Returns the number of markers inserted.
    /// The marker symbol is "●" and the label is the feature's `name` field.
    pub fn import_points_to_markers(&self, store: &MarkerStore) -> anyhow::Result<usize> {
        let mut count = 0usize;
        for feat in &self.features {
            let label = if feat.name.is_empty() { "GeoJSON" } else { &feat.name };
            match &feat.geometry {
                GeoGeometry::Point((lon, lat)) => {
                    store.insert(*lat, *lon, "●", label)?;
                    count += 1;
                }
                GeoGeometry::MultiPoint(pts) => {
                    for (lon, lat) in pts {
                        store.insert(*lat, *lon, "●", label)?;
                        count += 1;
                    }
                }
                _ => {}
            }
        }
        Ok(count)
    }

    /// All line-segments from all LineString, MultiLineString, Polygon, and
    /// MultiPolygon geometries as a flat iterator of `(Coord, Coord)` pairs.
    pub fn segments(&self) -> impl Iterator<Item = (Coord, Coord)> + '_ {
        self.features.iter().flat_map(|f| geometry_segments(&f.geometry))
    }

    /// All individual points from all point-like geometries.
    pub fn all_point_coords(&self) -> impl Iterator<Item = Coord> + '_ {
        self.features.iter().flat_map(|f| geometry_points(&f.geometry))
    }
}

// ── Geometry helpers ──────────────────────────────────────────────────────────

fn geometry_segments(g: &GeoGeometry) -> Vec<(Coord, Coord)> {
    let mut out = Vec::new();
    match g {
        GeoGeometry::LineString(pts) => {
            push_ring_segs(&mut out, pts, false);
        }
        GeoGeometry::MultiLineString(lines) => {
            for l in lines { push_ring_segs(&mut out, l, false); }
        }
        GeoGeometry::Polygon(rings) => {
            for r in rings { push_ring_segs(&mut out, r, true); }
        }
        GeoGeometry::MultiPolygon(polys) => {
            for poly in polys {
                for r in poly { push_ring_segs(&mut out, r, true); }
            }
        }
        GeoGeometry::Collection(children) => {
            for c in children { out.extend(geometry_segments(c)); }
        }
        _ => {}
    }
    out
}

fn push_ring_segs(out: &mut Vec<(Coord, Coord)>, pts: &[Coord], close: bool) {
    for w in pts.windows(2) {
        out.push((w[0], w[1]));
    }
    if close && pts.len() > 2 {
        out.push((*pts.last().unwrap(), pts[0]));
    }
}

fn geometry_points(g: &GeoGeometry) -> Vec<Coord> {
    match g {
        GeoGeometry::Point(c)       => vec![*c],
        GeoGeometry::MultiPoint(v)  => v.clone(),
        GeoGeometry::Collection(ch) => ch.iter().flat_map(geometry_points).collect(),
        _ => vec![],
    }
}

// ── JSON parsing ──────────────────────────────────────────────────────────────

/// Recursively collect features from a GeoJSON value.
fn collect_features(
    val:         &serde_json::Value,
    parent_props: &serde_json::Map<String, serde_json::Value>,
    out:         &mut Vec<GeoFeature>,
) -> anyhow::Result<()> {
    let typ = val["type"].as_str().unwrap_or("");
    match typ {
        "FeatureCollection" => {
            let feats = val["features"].as_array()
                .ok_or_else(|| anyhow!("FeatureCollection missing 'features' array"))?;
            for f in feats {
                collect_features(f, parent_props, out)?;
            }
        }
        "Feature" => {
            let geom_val = &val["geometry"];
            if geom_val.is_null() { return Ok(()); }
            if let Some(geom) = parse_geometry(geom_val) {
                let props = val["properties"]
                    .as_object()
                    .cloned()
                    .unwrap_or_default();
                let name = extract_name(&props);
                out.push(GeoFeature { geometry: geom, name, properties: props });
            }
        }
        // Bare geometry object (no Feature wrapper)
        "GeometryCollection" => {
            let geoms = val["geometries"].as_array()
                .ok_or_else(|| anyhow!("GeometryCollection missing 'geometries'"))?;
            let children: Vec<GeoGeometry> = geoms.iter()
                .filter_map(parse_geometry)
                .collect();
            if !children.is_empty() {
                out.push(GeoFeature {
                    geometry:   GeoGeometry::Collection(children),
                    name:       String::new(),
                    properties: parent_props.clone(),
                });
            }
        }
        _ => {
            if let Some(geom) = parse_geometry(val) {
                out.push(GeoFeature {
                    geometry:   geom,
                    name:       String::new(),
                    properties: parent_props.clone(),
                });
            }
        }
    }
    Ok(())
}

/// Parse a GeoJSON geometry object from a `serde_json::Value`.
fn parse_geometry(val: &serde_json::Value) -> Option<GeoGeometry> {
    let typ = val["type"].as_str()?;
    match typ {
        "Point" => {
            let c = parse_coord(val["coordinates"].as_array()?)?;
            Some(GeoGeometry::Point(c))
        }
        "MultiPoint" => {
            let pts = val["coordinates"].as_array()?
                .iter().filter_map(|v| parse_coord(v.as_array()?)).collect();
            Some(GeoGeometry::MultiPoint(pts))
        }
        "LineString" => {
            let pts = parse_coord_list(val["coordinates"].as_array()?);
            if pts.len() >= 2 { Some(GeoGeometry::LineString(pts)) } else { None }
        }
        "MultiLineString" => {
            let lines = val["coordinates"].as_array()?
                .iter()
                .filter_map(|arr| {
                    let pts = parse_coord_list(arr.as_array()?);
                    if pts.len() >= 2 { Some(pts) } else { None }
                })
                .collect();
            Some(GeoGeometry::MultiLineString(lines))
        }
        "Polygon" => {
            let rings = val["coordinates"].as_array()?
                .iter()
                .filter_map(|arr| {
                    let pts = parse_coord_list(arr.as_array()?);
                    if pts.len() >= 3 { Some(pts) } else { None }
                })
                .collect();
            Some(GeoGeometry::Polygon(rings))
        }
        "MultiPolygon" => {
            let polys = val["coordinates"].as_array()?
                .iter()
                .filter_map(|poly_arr| {
                    let rings: Vec<Vec<Coord>> = poly_arr.as_array()?
                        .iter()
                        .filter_map(|arr| {
                            let pts = parse_coord_list(arr.as_array()?);
                            if pts.len() >= 3 { Some(pts) } else { None }
                        })
                        .collect();
                    if rings.is_empty() { None } else { Some(rings) }
                })
                .collect();
            Some(GeoGeometry::MultiPolygon(polys))
        }
        "GeometryCollection" => {
            let children: Vec<GeoGeometry> = val["geometries"].as_array()?
                .iter().filter_map(parse_geometry).collect();
            Some(GeoGeometry::Collection(children))
        }
        _ => None,
    }
}

/// Parse `[lon, lat]` or `[lon, lat, elev]` from a JSON array.
fn parse_coord(arr: &[serde_json::Value]) -> Option<Coord> {
    let lon = arr.get(0)?.as_f64()?;
    let lat = arr.get(1)?.as_f64()?;
    Some((lon, lat))
}

fn parse_coord_list(arr: &[serde_json::Value]) -> Vec<Coord> {
    arr.iter().filter_map(|v| parse_coord(v.as_array()?)).collect()
}

/// Extract a human-readable name from a feature's properties.
fn extract_name(props: &serde_json::Map<String, serde_json::Value>) -> String {
    for key in &["name", "Name", "NAME", "title", "label", "id"] {
        if let Some(v) = props.get(*key) {
            if let Some(s) = v.as_str() {
                if !s.is_empty() { return s.to_string(); }
            }
        }
    }
    String::new()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> GeoLayer {
        let val: serde_json::Value = serde_json::from_str(json).unwrap();
        let mut features = Vec::new();
        collect_features(&val, &serde_json::Map::new(), &mut features).unwrap();
        GeoLayer { source: "test".into(), features }
    }

    #[test]
    fn point_feature() {
        let layer = parse(r#"{
            "type": "Feature",
            "geometry": { "type": "Point", "coordinates": [-0.1278, 51.5074] },
            "properties": { "name": "London" }
        }"#);
        assert_eq!(layer.features.len(), 1);
        let feat = &layer.features[0];
        assert_eq!(feat.name, "London");
        if let GeoGeometry::Point((lon, lat)) = feat.geometry {
            assert!((lon - (-0.1278)).abs() < 1e-6);
            assert!((lat - 51.5074).abs() < 1e-6);
        } else {
            panic!("expected Point");
        }
    }

    #[test]
    fn feature_collection() {
        let layer = parse(r#"{
            "type": "FeatureCollection",
            "features": [
                {
                    "type": "Feature",
                    "geometry": { "type": "Point", "coordinates": [2.3522, 48.8566] },
                    "properties": { "name": "Paris" }
                },
                {
                    "type": "Feature",
                    "geometry": {
                        "type": "LineString",
                        "coordinates": [[0,0],[1,1],[2,2]]
                    },
                    "properties": {}
                }
            ]
        }"#);
        assert_eq!(layer.features.len(), 2);
        assert_eq!(layer.points().count(), 1);
        assert_eq!(layer.lines().count(), 1);
    }

    #[test]
    fn polygon_segments() {
        let layer = parse(r#"{
            "type": "Feature",
            "geometry": {
                "type": "Polygon",
                "coordinates": [[[0,0],[1,0],[1,1],[0,1],[0,0]]]
            },
            "properties": {}
        }"#);
        // A 5-point closed ring produces 4 window pairs + 1 close segment = 5
        let segs: Vec<_> = layer.segments().collect();
        assert_eq!(segs.len(), 5);
    }

    #[test]
    fn bare_geometry() {
        let layer = parse(r#"{"type":"Point","coordinates":[10.0,20.0]}"#);
        assert_eq!(layer.features.len(), 1);
    }
}
