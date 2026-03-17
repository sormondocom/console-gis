/// console-gis — Geographic Information System for the terminal.
///
/// A cross-platform GIS library targeting terminals from DEC VT-100 /
/// VT-220 (ASCII, monochrome) up to modern 24-bit true-colour terminals.
///
/// # Architecture
///
/// | Crate module | Responsibility |
/// |---|---|
/// | [`geo`]    | Coordinate types, projections, zoom/resolution system |
/// | [`data`]   | World-map polygon data, persistent marker store (sled) |
/// | [`render`] | Raycasting renderer, terminal capability tiers, compat tables |
/// | [`tui`]    | Interactive TUI: splash, menu, globe view, zoom explorer |
pub mod geo;
pub mod data;
pub mod render;
pub mod tui;
