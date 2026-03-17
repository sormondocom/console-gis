/// Terminal capability conversion tables — "upshifting" and "downshifting".
///
/// # Concept
///
/// A rendering pipeline produces output for a target capability tier.  When
/// the actual terminal is *less* capable ("downshift") or *more* capable
/// ("upshift") than the compiled-in default, these tables translate colours,
/// characters, and attributes so that output always looks as good as possible
/// on the actual terminal.
///
/// ```text
///  TrueColor ──downshift──► Color256 ──► Ansi8 ──► Vt220 ──► Vt100
///  Vt100     ──upshift────►  Vt220  ──► Ansi8 ──► Color256 ──► TrueColor
/// ```
///
/// # VT-100 / VT-220 colour model
///
/// VT-100 has **no colour** — only character attributes: bold, underline,
/// blink, reverse-video.  `Vt220` is identical for rendering purposes (132-col
/// mode is a viewport concern, not a colour concern).
///
/// The downshift chain converts an RGB colour to a VT attribute pair:
///
/// | Luminance range | Attribute |
/// |-----------------|-----------|
/// | 0–30            | dim (nothing — terminal default) |
/// | 31–100          | normal intensity                 |
/// | 101–200         | normal intensity                 |
/// | 201–255         | **bold** (increases perceived brightness) |
///
/// Additionally, "reverse video" (`ESC[7m`) can be used for selected /
/// highlighted cells to invert foreground and background.

use super::canvas::{Color, TerminalCapability};

// ── xterm-256 palette ─────────────────────────────────────────────────────────

/// RGB values for the 256-colour xterm palette, indexed 0–255.
///
/// Slots 0–7: standard ANSI.
/// Slots 8–15: high-intensity ANSI.
/// Slots 16–231: 6×6×6 RGB cube (`i = 16 + 36r + 6g + b`; level → 0 or 55+40l).
/// Slots 232–255: grayscale ramp (`i = 232+k`, RGB = 8+10k).
pub fn xterm256_rgb(index: u8) -> (u8, u8, u8) {
    // Standard 16 colours
    const STD: [(u8, u8, u8); 16] = [
        (0,   0,   0),   // 0  Black
        (128, 0,   0),   // 1  Dark Red
        (0,   128, 0),   // 2  Dark Green
        (128, 128, 0),   // 3  Dark Yellow
        (0,   0,   128), // 4  Dark Blue
        (128, 0,   128), // 5  Dark Magenta
        (0,   128, 128), // 6  Dark Cyan
        (192, 192, 192), // 7  Light Gray
        (128, 128, 128), // 8  Dark Gray
        (255, 0,   0),   // 9  Red
        (0,   255, 0),   // 10 Green
        (255, 255, 0),   // 11 Yellow
        (0,   0,   255), // 12 Blue
        (255, 0,   255), // 13 Magenta
        (0,   255, 255), // 14 Cyan
        (255, 255, 255), // 15 White
    ];
    if (index as usize) < STD.len() {
        return STD[index as usize];
    }
    if index >= 232 {
        let v = 8 + 10 * (index - 232) as u16;
        let v = v.min(255) as u8;
        return (v, v, v);
    }
    // 6×6×6 cube
    let i = index - 16;
    let cube_val = |n: u8| if n == 0 { 0u8 } else { 55 + 40 * n };
    let b = cube_val(i % 6);
    let g = cube_val((i / 6) % 6);
    let r = cube_val(i / 36);
    (r, g, b)
}

// ── Colour distance (squared Euclidean in RGB space) ─────────────────────────

fn rgb_dist2(a: (u8, u8, u8), b: (u8, u8, u8)) -> u32 {
    let dr = (a.0 as i32 - b.0 as i32).pow(2) as u32;
    let dg = (a.1 as i32 - b.1 as i32).pow(2) as u32;
    let db = (a.2 as i32 - b.2 as i32).pow(2) as u32;
    dr + dg + db
}

// ── TrueColor → 256 ─────────────────────────────────────────────────────────

/// Map a 24-bit RGB colour to the nearest xterm-256 palette index.
///
/// Uses the 6×6×6 cube and grayscale ramp; the standard 0–15 ANSI colours
/// are excluded from the search to avoid inconsistent terminal interpretations.
pub fn rgb_to_256(r: u8, g: u8, b: u8) -> u8 {
    // Try 6×6×6 cube
    let cube_idx = |v: u8| {
        if v < 28 { 0u8 }
        else { ((v as u16 - 35) / 40).min(5) as u8 }
    };
    let ri = cube_idx(r);
    let gi = cube_idx(g);
    let bi = cube_idx(b);
    let cube_color = 16 + 36 * ri + 6 * gi + bi;
    let cube_rgb   = xterm256_rgb(cube_color);

    // Try grayscale ramp (indices 232–255)
    let lum = (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) as u16;
    let gray_idx: u8 = ((lum.saturating_sub(8)) / 10).min(23) as u8;
    let gray_color = 232 + gray_idx;
    let gray_rgb   = xterm256_rgb(gray_color);

    let query = (r, g, b);
    if rgb_dist2(query, cube_rgb) <= rgb_dist2(query, gray_rgb) {
        cube_color
    } else {
        gray_color
    }
}

// ── 256 → 8 ─────────────────────────────────────────────────────────────────

/// Map an xterm-256 index to an ANSI 8-colour index (0–7) + bold flag.
///
/// Returns `(ansi_index, bold)`.
pub fn idx256_to_ansi8(idx: u8) -> (u8, bool) {
    if idx < 8  { return (idx, false); }
    if idx < 16 { return (idx - 8, true); }

    let rgb = xterm256_rgb(idx);
    // Find nearest in the 8-standard palette (indices 0–7)
    let best = (0u8..8).min_by_key(|&i| {
        rgb_dist2(rgb, xterm256_rgb(i))
    }).unwrap_or(7);

    // Bold if the source is significantly brighter than the standard colour
    let src_lum = luminance(rgb);
    let dst_lum = luminance(xterm256_rgb(best));
    (best, src_lum > dst_lum + 30)
}

// ── TrueColor → 8 ────────────────────────────────────────────────────────────

/// Map a 24-bit RGB colour directly to ANSI 8-colour + bold.
pub fn rgb_to_ansi8(r: u8, g: u8, b: u8) -> (u8, bool) {
    idx256_to_ansi8(rgb_to_256(r, g, b))
}

// ── TrueColor / 8-colour → VT-100 attributes ─────────────────────────────────

/// VT-100 text attribute set (no colour — only bold, underline, reverse).
#[derive(Debug, Clone, Copy, Default)]
pub struct Vt100Attr {
    pub bold:    bool,
    pub underline: bool,
    pub reverse: bool,
}

impl Vt100Attr {
    /// Emit the minimal escape sequence to apply this attribute set.
    /// Always resets first so states don't accumulate.
    pub fn escape_str(self) -> String {
        let mut s = String::from("\x1b[0");
        if self.bold      { s.push_str(";1"); }
        if self.underline { s.push_str(";4"); }
        if self.reverse   { s.push_str(";7"); }
        s.push('m');
        s
    }
}

/// Map a 24-bit RGB colour to VT-100 attributes for monochrome rendering.
///
/// Uses luminance alone:
/// - Very dark (< 20) → normal (barely visible; trust background)
/// - Dark (20–140)    → normal
/// - Bright (141–255) → bold
pub fn rgb_to_vt100(r: u8, g: u8, b: u8) -> Vt100Attr {
    let lum = luminance((r, g, b));
    Vt100Attr {
        bold:      lum > 140,
        underline: false,
        reverse:   false,
    }
}

/// Map a colour to VT-100 attributes for a *selected* (highlighted) cell.
/// Applies reverse video in addition to normal luminance mapping.
pub fn rgb_to_vt100_selected(r: u8, g: u8, b: u8) -> Vt100Attr {
    let mut attr = rgb_to_vt100(r, g, b);
    attr.reverse = true;
    attr
}

// ── Character downshift ───────────────────────────────────────────────────────

/// Map a Unicode box-drawing character to its DEC line-drawing equivalent
/// (sent in DEC Special Character mode, `ESC(0`).
///
/// Returns `None` if no mapping exists (caller should fall back to ASCII).
pub fn unicode_box_to_dec(ch: char) -> Option<char> {
    Some(match ch {
        '─' | '━' => 'q',
        '│' | '┃' => 'x',
        '┌' | '┏' => 'l',
        '┐' | '┓' => 'k',
        '└' | '┗' => 'm',
        '┘' | '┛' => 'j',
        '┼' | '╋' => 'n',
        '├' | '┣' => 't',
        '┤' | '┫' => 'u',
        '┬' | '┳' => 'w',
        '┴' | '┻' => 'v',
        _ => return None,
    })
}

/// Map a Unicode box-drawing character to an ASCII fallback.
pub fn unicode_box_to_ascii(ch: char) -> char {
    match ch {
        '─' | '━' | '═' => '-',
        '│' | '┃' | '║' => '|',
        '┌' | '┏' | '╔' | '╭' => '+',
        '┐' | '┓' | '╗' | '╮' => '+',
        '└' | '┗' | '╚' | '╰' => '+',
        '┘' | '┛' | '╝' | '╯' => '+',
        '┼' | '╋' | '╬' => '+',
        '├' | '┣' | '╠' => '+',
        '┤' | '┫' | '╣' => '+',
        '┬' | '┳' | '╦' => '+',
        '┴' | '┻' | '╩' => '+',
        '▀' | '▄' | '█' => '#',
        '◉' | '●' | '◎' => 'O',
        '▸' | '►' => '>',
        '·' | '•' => '.',
        _ => ch,
    }
}

/// Downshift a character for a target capability tier.
pub fn downshift_char(ch: char, target: TerminalCapability) -> String {
    if target >= TerminalCapability::Ansi8 {
        return ch.to_string(); // Unicode supported
    }
    // VT-100 / VT-220: try DEC line drawing first, then ASCII
    if let Some(dec) = unicode_box_to_dec(ch) {
        // Caller must wrap in ESC(0 / ESC(B
        return dec.to_string();
    }
    unicode_box_to_ascii(ch).to_string()
}

// ── Colour capability shift (high-level) ─────────────────────────────────────

/// Render a foreground colour as the best ANSI escape for `target`.
///
/// This is the main colour downshift entry point used by the renderer.
pub fn color_fg_escape(color: Color, target: TerminalCapability) -> String {
    match target {
        TerminalCapability::TrueColor => color.ansi_fg(),
        TerminalCapability::Color256 => {
            let idx = rgb_to_256(color.r, color.g, color.b);
            format!("\x1b[38;5;{}m", idx)
        }
        TerminalCapability::Ansi8 => {
            let (ansi, bold) = rgb_to_ansi8(color.r, color.g, color.b);
            if bold {
                format!("\x1b[1;{}m", 30 + ansi)
            } else {
                format!("\x1b[{}m",   30 + ansi)
            }
        }
        TerminalCapability::Vt100 | TerminalCapability::Vt220 => {
            rgb_to_vt100(color.r, color.g, color.b).escape_str()
        }
    }
}

/// Render a background colour as the best ANSI escape for `target`.
pub fn color_bg_escape(color: Color, target: TerminalCapability) -> String {
    match target {
        TerminalCapability::TrueColor => color.ansi_bg(),
        TerminalCapability::Color256 => {
            let idx = rgb_to_256(color.r, color.g, color.b);
            format!("\x1b[48;5;{}m", idx)
        }
        TerminalCapability::Ansi8 => {
            let (ansi, bold) = rgb_to_ansi8(color.r, color.g, color.b);
            if bold {
                format!("\x1b[1;{}m", 40 + ansi)
            } else {
                format!("\x1b[{}m",   40 + ansi)
            }
        }
        TerminalCapability::Vt100 | TerminalCapability::Vt220 => {
            // No background colour on VT-100; use reverse video for dark BG
            let lum = luminance((color.r, color.g, color.b));
            if lum < 50 { "\x1b[0m".to_string() } else { "\x1b[7m".to_string() }
        }
    }
}

// ── Utilities ─────────────────────────────────────────────────────────────────

fn luminance(rgb: (u8, u8, u8)) -> u8 {
    (0.299 * rgb.0 as f64 + 0.587 * rgb.1 as f64 + 0.114 * rgb.2 as f64) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xterm256_spot_checks() {
        assert_eq!(xterm256_rgb(0),   (0, 0, 0));
        assert_eq!(xterm256_rgb(15),  (255, 255, 255));
        assert_eq!(xterm256_rgb(16),  (0, 0, 0));
        assert_eq!(xterm256_rgb(231), (255, 255, 255));
        assert_eq!(xterm256_rgb(232), (8, 8, 8));
        assert_eq!(xterm256_rgb(255), (238, 238, 238));
    }

    #[test]
    fn rgb_to_256_white() {
        let idx = rgb_to_256(255, 255, 255);
        let rgb = xterm256_rgb(idx);
        let dist = rgb_dist2((255, 255, 255), rgb);
        assert!(dist < 1000, "white mapping dist={dist}");
    }

    #[test]
    fn vt100_bold_for_bright() {
        let attr = rgb_to_vt100(255, 255, 255);
        assert!(attr.bold);
        let attr = rgb_to_vt100(10, 10, 10);
        assert!(!attr.bold);
    }
}
