/// Splash screen — a procedural ASCII/Unicode rendering of the SVG icon.
///
/// The design mirrors `assets/icon.svg` exactly: orthographic globe projection,
/// same graticule spacing (15°), same special parallels (equator / tropics),
/// same terminal-cursor element.  In VT-100 mode the colours drop out; the
/// geometry is identical.
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};
use std::f64::consts::PI;

/// Pixel record written by the projector.
#[derive(Clone, Copy)]
enum GlobePixel {
    Background,
    Ocean(f64),          // diffuse light factor
    Graticule(f64),
    Equator,
    Tropic(f64),
    Cursor,
}

fn classify_globe(lat: f64, lon: f64, rotation: f64) -> GlobePixel {
    // Cursor position: ~40°N, 0°E after rotation
    let cursor_lat = 40.0_f64;
    let cursor_lon = 0.0_f64 - rotation.to_degrees();
    if (lat - cursor_lat).abs() < 3.0 && (lon - cursor_lon).abs() < 4.0 {
        return GlobePixel::Cursor;
    }

    // Equator
    if lat.abs() < 1.5 { return GlobePixel::Equator; }

    // Tropics / polar circles
    for &sl in &[23.5_f64, -23.5, 66.5, -66.5] {
        if (lat - sl).abs() < 1.5 { return GlobePixel::Tropic(0.85); }
    }

    // Graticule (15°)
    let lat_mod = lat.rem_euclid(15.0);
    let lon_mod = lon.rem_euclid(15.0);
    if lat_mod < 1.5 || lat_mod > 13.5 || lon_mod < 1.5 || lon_mod > 13.5 {
        // Compute light for this point
        let lat_r = lat.to_radians();
        let lon_r = lon.to_radians();
        let nx = lat_r.cos() * lon_r.sin();
        let ny = lat_r.sin();
        let nz = -(lat_r.cos() * lon_r.cos());
        let light = (0.8081 * nx + 0.5051 * ny - 0.3030 * nz).max(0.0);
        return GlobePixel::Graticule(0.08 + 0.92 * light);
    }

    // Ocean
    let lat_r = lat.to_radians();
    let lon_r = lon.to_radians();
    let nx = lat_r.cos() * lon_r.sin();
    let ny = lat_r.sin();
    let nz = -(lat_r.cos() * lon_r.cos());
    let light = (0.8081 * nx + 0.5051 * ny - 0.3030 * nz).max(0.0);
    GlobePixel::Ocean(0.08 + 0.92 * light)
}

/// Render the splash globe into a ratatui Buffer.
pub struct SplashWidget {
    pub rotation: f64,
    pub supports_true_colour: bool,
}

impl Widget for SplashWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let cols = area.width  as usize;
        let rows = area.height as usize;

        // Reserve bottom 4 rows for title / tagline
        let globe_rows = rows.saturating_sub(4).max(4);

        let pw = cols;
        let ph = globe_rows * 2; // half-block pixel height
        let cx = pw as f64 / 2.0;
        let cy = ph as f64 / 2.0;
        let scale = cx.min(cy) * 0.90;

        // Eye at z=−2.5
        const EYE_Z: f64 = -2.5;

        for row in 0..globe_rows {
            for col in 0..cols {
                // Compute top and bottom half-block pixel colours
                let mut pixels = [GlobePixel::Background; 2];
                for (k, prow) in [row * 2, row * 2 + 1].iter().enumerate() {
                    let ndx = (col as f64 - cx) / scale;
                    let ndy = -(*prow as f64 - cy) / scale;

                    // Ray–sphere intersection
                    let (dx, dy, dz) = {
                        let ex = 0.0_f64;
                        let ey = 0.0_f64;
                        let ez = EYE_Z;
                        let rx = ndx - ex;
                        let ry = ndy - ey;
                        let rz = -ez;
                        let n = (rx * rx + ry * ry + rz * rz).sqrt();
                        (rx / n, ry / n, rz / n)
                    };
                    let (ex, ey, ez) = (0.0_f64, 0.0_f64, EYE_Z);
                    let b = 2.0 * (ex * dx + ey * dy + ez * dz);
                    let c = ex * ex + ey * ey + ez * ez - 1.0;
                    let disc = b * b - 4.0 * c;

                    pixels[k] = if disc >= 0.0 {
                        let t = (-b - disc.sqrt()) * 0.5;
                        if t > 0.0 {
                            let (hx, hy, hz) = (ex + t * dx, ey + t * dy, ez + t * dz);
                            // Un-rotate to world
                            let (s, co) = (self.rotation.sin(), self.rotation.cos());
                            let wx = hx * co + hz * s;
                            let wy = hy;
                            let wz = -hx * s + hz * co;
                            let lat = wy.clamp(-1.0, 1.0).asin().to_degrees();
                            let lon = wx.atan2(-wz).to_degrees();
                            classify_globe(lat, lon, self.rotation)
                        } else {
                            GlobePixel::Background
                        }
                    } else {
                        GlobePixel::Background
                    };
                }

                // Map pixels to ratatui cell
                let cell = buf.get_mut(area.x + col as u16, area.y + row as u16);

                if self.supports_true_colour {
                    let top_rgb = pixel_to_rgb(pixels[0]);
                    let bot_rgb = pixel_to_rgb(pixels[1]);
                    cell.set_char('▀');
                    cell.set_fg(Color::Rgb(top_rgb.0, top_rgb.1, top_rgb.2));
                    cell.set_bg(Color::Rgb(bot_rgb.0, bot_rgb.1, bot_rgb.2));
                } else {
                    // Fallback: ASCII shade from luminance of top pixel
                    let lum = pixel_luminance(pixels[0]);
                    cell.set_char(ascii_shade(lum));
                    cell.set_fg(Color::White);
                    cell.set_bg(Color::Black);
                }
            }
        }

        // ── Title banner (derived from SVG text elements) ────────────────────
        let title_row = area.y + globe_rows as u16;

        // Blank separator
        for col in 0..cols {
            let cell = buf.get_mut(area.x + col as u16, title_row);
            cell.set_char(' ');
            cell.set_bg(Color::Black);
        }

        let title_style = if self.supports_true_colour {
            Style::default().fg(Color::Rgb(125, 196, 228))
        } else {
            Style::default().fg(Color::Cyan)
        };

        let tag_style = if self.supports_true_colour {
            Style::default().fg(Color::Rgb(45, 90, 122))
        } else {
            Style::default().fg(Color::DarkGray)
        };

        // "CONSOLE-GIS" centred
        let title = "C O N S O L E - G I S";
        let tag   = "v0.1.0  —  geographic information system for the terminal";

        let title_x = area.x + ((cols.saturating_sub(title.len())) / 2) as u16;
        let tag_x   = area.x + ((cols.saturating_sub(tag.len()))   / 2) as u16;

        for (i, ch) in title.chars().enumerate() {
            if title_x as usize + i < area.x as usize + cols {
                let cell = buf.get_mut(title_x + i as u16, title_row + 1);
                cell.set_char(ch);
                cell.set_style(title_style);
            }
        }
        for (i, ch) in tag.chars().enumerate() {
            if tag_x as usize + i < area.x as usize + cols {
                let cell = buf.get_mut(tag_x + i as u16, title_row + 2);
                cell.set_char(ch);
                cell.set_style(tag_style);
            }
        }

        // "Press any key…" hint
        let hint = "[ press any key to continue ]";
        let hint_x = area.x + ((cols.saturating_sub(hint.len())) / 2) as u16;
        for (i, ch) in hint.chars().enumerate() {
            if hint_x as usize + i < area.x as usize + cols {
                let cell = buf.get_mut(hint_x + i as u16, title_row + 3);
                cell.set_char(ch);
                cell.set_style(tag_style);
            }
        }
    }
}

// ── Colour helpers ────────────────────────────────────────────────────────────

fn pixel_to_rgb(px: GlobePixel) -> (u8, u8, u8) {
    match px {
        GlobePixel::Background => (4, 4, 18),
        GlobePixel::Ocean(l)   => shade(20, 60, 170, l),
        GlobePixel::Graticule(l) => shade(30, 200, 240, l),
        GlobePixel::Equator    => (255, 220, 0),
        GlobePixel::Tropic(l)  => shade(255, 176, 48, l),
        GlobePixel::Cursor     => (0, 255, 136),
    }
}

fn shade(r: u8, g: u8, b: u8, f: f64) -> (u8, u8, u8) {
    let f = f.clamp(0.0, 1.0);
    ((r as f64 * f) as u8, (g as f64 * f) as u8, (b as f64 * f) as u8)
}

fn pixel_luminance(px: GlobePixel) -> u8 {
    let (r, g, b) = pixel_to_rgb(px);
    (0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64) as u8
}

fn ascii_shade(lum: u8) -> char {
    const S: &[char] = &[' ', '.', '`', '\'', '-', ':', '+', 'o', '0', '#'];
    S[(lum as usize * (S.len() - 1)) / 255]
}
