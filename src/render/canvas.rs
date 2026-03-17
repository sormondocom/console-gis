/// Terminal capability tiers — from VT-100 monochrome up to 24-bit true colour.
///
/// # Tier overview
///
/// | Tier      | Colour   | Unicode | Block graphics | Notes                       |
/// |-----------|----------|---------|----------------|-----------------------------|
/// | `Vt100`   | none     | no      | no             | DEC line-drawing available  |
/// | `Vt220`   | none     | no      | no             | + 132-col mode              |
/// | `Ansi8`   | 8-colour | yes     | ▀ ▄ █          | ECMA-48 standard            |
/// | `Color256` | 256     | yes     | ▀ ▄ █          | xterm 256-colour palette    |
/// | `TrueColor`| 24-bit  | yes     | ▀ ▄ █          | Modern terminals            |
///
/// # VT-100 / VT-220 rendering
///
/// In `Vt100`/`Vt220` mode the renderer uses a 10-step luminance gradient
/// mapped to ASCII characters: `' ' . : + o 0 # @ █ ■`.  Bold is asserted for
/// luminance > 180 (equivalent of ≥ 70 % brightness) to give extra contrast.
///
/// DEC Special Character Set (line-drawing) is used for UI borders — available
/// on all DEC-compatible terminals including VT-100 and later.
///
/// # VT-220 extras
///
/// `Vt220` additionally supports:
/// - 132-column mode (`ESC [ ? 3 h` / `ESC [ ? 3 l`)
/// - ISO Latin-1 supplemental character set (access code points 0xA0–0xFF)
/// - Slightly richer line-drawing via G1/G2/G3 character sets
///
/// In practice, for *rendering* the globe the only user-visible difference
/// between `Vt100` and `Vt220` is that a 132-column terminal gives a much
/// wider globe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TerminalCapability {
    /// DEC VT-100: ASCII + bold/reverse/underline, no colour.
    /// Uses 10-step ASCII luminance gradient.  DEC line-drawing for borders.
    Vt100,
    /// DEC VT-220: VT-100 + 132-column mode + ISO Latin-1.
    /// Rendering is identical to Vt100 but wider viewport possible.
    Vt220,
    /// ANSI 8-colour (`ESC[3Nm` / `ESC[4Nm`). Unicode block graphics enabled.
    Ansi8,
    /// xterm 256-colour (`ESC[38;5;Nm`).
    Color256,
    /// 24-bit true colour (`ESC[38;2;R;G;Bm`).
    TrueColor,
}

impl TerminalCapability {
    pub fn is_vt_legacy(self) -> bool {
        matches!(self, TerminalCapability::Vt100 | TerminalCapability::Vt220)
    }

    pub fn supports_unicode(self) -> bool {
        self >= TerminalCapability::Ansi8
    }

    pub fn supports_half_block(self) -> bool {
        self >= TerminalCapability::Ansi8
    }

    pub fn supports_true_colour(self) -> bool {
        self == TerminalCapability::TrueColor
    }

    /// Whether this terminal can switch to 132-column mode.
    pub fn supports_132col(self) -> bool {
        matches!(self, TerminalCapability::Vt220)
    }

    /// ESC sequence to enter 132-column mode (VT-220 only).
    pub fn enter_132col() -> &'static str { "\x1b[?3h" }
    /// ESC sequence to leave 132-column mode.
    pub fn leave_132col() -> &'static str { "\x1b[?3l" }

    pub fn label(self) -> &'static str {
        match self {
            TerminalCapability::Vt100     => "VT-100 (ASCII, monochrome)",
            TerminalCapability::Vt220     => "VT-220 (ASCII, 132-col, ISO Latin-1)",
            TerminalCapability::Ansi8     => "ANSI 8-colour",
            TerminalCapability::Color256  => "xterm 256-colour",
            TerminalCapability::TrueColor => "24-bit true colour",
        }
    }
}

// ── DEC line-drawing character set helpers ────────────────────────────────────
//
// Activated with ESC(0, restored with ESC(B.
// These work on VT-100, VT-220, xterm, and most POSIX terminals.

pub mod dec_line {
    pub const ENTER: &str = "\x1b(0";
    pub const EXIT:  &str = "\x1b(B";

    // Characters in the DEC Special Graphics set (send the lowercase ASCII
    // equivalent — the terminal remaps them in line-drawing mode):
    pub const HORIZONTAL: char = 'q'; // ─
    pub const VERTICAL:   char = 'x'; // │
    pub const TOP_LEFT:   char = 'l'; // ┌
    pub const TOP_RIGHT:  char = 'k'; // ┐
    pub const BOT_LEFT:   char = 'm'; // └
    pub const BOT_RIGHT:  char = 'j'; // ┘
    pub const CROSS:      char = 'n'; // ┼
    pub const TEE_LEFT:   char = 't'; // ├
    pub const TEE_RIGHT:  char = 'u'; // ┤
    pub const TEE_TOP:    char = 'w'; // ┬
    pub const TEE_BOT:    char = 'v'; // ┴
}

// ── Color type ────────────────────────────────────────────────────────────────

/// A colour value that degrades gracefully to any terminal capability.
#[derive(Debug, Clone, Copy)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const fn new(r: u8, g: u8, b: u8) -> Self { Self { r, g, b } }

    pub const BLACK: Color = Color::new(0,   0,   0);
    pub const WHITE: Color = Color::new(255, 255, 255);
    pub const OCEAN: Color = Color::new(20,  60,  170);
    pub const LAND:  Color = Color::new(60,  140, 40);
    pub const GRID:  Color = Color::new(30,  200, 240);
    pub const GOLD:  Color = Color::new(255, 220, 0);
    pub const BG:    Color = Color::new(4,   4,   18);

    /// Scale brightness by `factor` in [0, 1].
    pub fn shade(self, factor: f64) -> Self {
        let f = factor.clamp(0.0, 1.0);
        Color::new(
            (self.r as f64 * f) as u8,
            (self.g as f64 * f) as u8,
            (self.b as f64 * f) as u8,
        )
    }

    /// Perceptual luminance [0, 255].
    pub fn luminance(self) -> u8 {
        (0.299 * self.r as f64 + 0.587 * self.g as f64 + 0.114 * self.b as f64) as u8
    }

    /// ANSI true-colour foreground escape.
    pub fn ansi_fg(self) -> String {
        format!("\x1b[38;2;{};{};{}m", self.r, self.g, self.b)
    }
    /// ANSI true-colour background escape.
    pub fn ansi_bg(self) -> String {
        format!("\x1b[48;2;{};{};{}m", self.r, self.g, self.b)
    }

    /// Nearest ANSI 8-colour foreground escape.
    pub fn ansi8_fg(self) -> &'static str {
        let lum = self.r as u16 + self.g as u16 + self.b as u16;
        if lum < 60  { return "\x1b[30m"; }
        if self.r > 150 && self.g < 100 && self.b < 100 { return "\x1b[31m"; }
        if self.g > 150 && self.r < 100 && self.b < 100 { return "\x1b[32m"; }
        if self.r > 150 && self.g > 150 && self.b < 100 { return "\x1b[33m"; }
        if self.b > 150 && self.r < 100 && self.g < 100 { return "\x1b[34m"; }
        if self.r > 150 && self.b > 150 && self.g < 100 { return "\x1b[35m"; }
        if self.g > 150 && self.b > 150 && self.r < 100 { return "\x1b[36m"; }
        "\x1b[37m"
    }

    /// 10-step ASCII luminance character — the VT-100 / VT-220 rendering path.
    pub fn ascii_shade(self) -> char {
        const S: &[char] = &[' ', '.', '`', '\'', '-', ':', '+', 'o', '0', '#'];
        let lum = self.luminance() as usize;
        S[(lum * (S.len() - 1)) / 255]
    }
}

// ── Canvas ────────────────────────────────────────────────────────────────────

/// Pixel canvas backed by half-block chars in colour mode, or ASCII shading
/// in VT-100 / VT-220 mode.
pub struct Canvas {
    pub pixel_width:  usize,
    pub pixel_height: usize,
    pixels: Vec<Color>,
    pub capability: TerminalCapability,
}

impl Canvas {
    pub fn new(cols: usize, rows: usize, capability: TerminalCapability) -> Self {
        let (pw, ph) = if capability.supports_half_block() {
            (cols, rows * 2)
        } else {
            (cols, rows) // VT-100/220: 1:1 char:pixel
        };
        Self {
            pixel_width:  pw,
            pixel_height: ph,
            pixels: vec![Color::BG; pw * ph],
            capability,
        }
    }

    pub fn set_pixel(&mut self, x: usize, y: usize, color: Color) {
        if x < self.pixel_width && y < self.pixel_height {
            self.pixels[y * self.pixel_width + x] = color;
        }
    }

    pub fn get_pixel(&self, x: usize, y: usize) -> Color {
        if x < self.pixel_width && y < self.pixel_height {
            self.pixels[y * self.pixel_width + x]
        } else {
            Color::BG
        }
    }

    /// Render to `Vec<String>`, one per terminal row.
    pub fn render_rows(&self) -> Vec<String> {
        let char_rows = if self.capability.supports_half_block() {
            self.pixel_height / 2
        } else {
            self.pixel_height
        };

        let mut rows = Vec::with_capacity(char_rows);
        for row in 0..char_rows {
            let mut line = String::with_capacity(self.pixel_width * 20);

            if self.capability.supports_half_block() {
                for col in 0..self.pixel_width {
                    let top = self.get_pixel(col, row * 2);
                    let bot = self.get_pixel(col, row * 2 + 1);
                    match self.capability {
                        TerminalCapability::TrueColor => {
                            line.push_str(&top.ansi_fg());
                            line.push_str(&bot.ansi_bg());
                            line.push('▀');
                        }
                        TerminalCapability::Color256 | TerminalCapability::Ansi8 => {
                            line.push_str(top.ansi8_fg());
                            line.push('▀');
                        }
                        _ => unreachable!(),
                    }
                }
            } else {
                // VT-100 / VT-220: ASCII shade, bold for bright pixels
                for col in 0..self.pixel_width {
                    let px = self.get_pixel(col, row);
                    let lum = px.luminance();
                    if lum > 180 { line.push_str("\x1b[1m"); }
                    line.push(px.ascii_shade());
                    if lum > 180 { line.push_str("\x1b[22m"); }
                }
            }

            line.push_str("\x1b[0m");
            rows.push(line);
        }
        rows
    }
}
