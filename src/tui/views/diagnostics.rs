/// Terminal Diagnostics — capability, character metrics, rendering advice.
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier},
    widgets::Widget,
};
use crate::render::canvas::TerminalCapability;
use crate::geo::zoom::{ConsoleResolution, RenderMode};

pub struct DiagnosticsView {
    pub capability:  TerminalCapability,
    pub cols:        u16,
    pub rows:        u16,
    pub char_aspect: f64,
}

/// Write one line into the buffer at (area.x, row), advance row.
fn put_row(buf: &mut Buffer, area: Rect, row: &mut u16, text: &str, fg: Color, bold: bool) {
    if *row >= area.bottom() { *row += 1; return; }
    for (i, ch) in text.chars().enumerate() {
        let col = area.x + i as u16;
        if col >= area.right() { break; }
        let cell = buf.get_mut(col, *row);
        cell.set_char(ch);
        cell.set_fg(fg);
        if bold { cell.set_style(cell.style().add_modifier(Modifier::BOLD)); }
    }
    *row += 1;
}

impl Widget for DiagnosticsView {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let tc     = self.capability.supports_true_colour();
        let accent = if tc { Color::Rgb(30, 200, 240) } else { Color::Cyan };
        let dim    = if tc { Color::Rgb(50, 80, 100)  } else { Color::DarkGray };
        let ok     = if tc { Color::Rgb(0, 220, 100)  } else { Color::Green };
        let warn   = if tc { Color::Rgb(255, 180, 0)  } else { Color::Yellow };

        let mut row = area.y;
        macro_rules! wr {
            ($t:expr, $fg:expr, $bold:expr) => {
                put_row(buf, area, &mut row, $t, $fg, $bold)
            };
        }

        wr!(" Terminal Diagnostics                                      Esc back", accent, true);
        wr!("", Color::Reset, false);

        wr!(" ── Colour & Character Support ──────────────────────────────────", dim, false);
        wr!(&format!("  Colour capability : {}", self.capability.label()), Color::White, false);
        wr!(
            &format!("  Unicode support   : {}",
                if self.capability.supports_unicode() { "yes" } else { "no (VT-100 mode)" }),
            if self.capability.supports_unicode() { ok } else { warn }, false
        );
        wr!(
            &format!("  Half-block chars  : {}",
                if self.capability.supports_half_block() { "yes (▀ ▄ enabled)" } else { "no" }),
            if self.capability.supports_half_block() { ok } else { warn }, false
        );
        wr!(
            &format!("  True colour       : {}", if tc { "yes" } else { "no" }),
            if tc { ok } else { warn }, false
        );
        wr!("", Color::Reset, false);

        wr!(" ── Viewport Geometry ───────────────────────────────────────────", dim, false);
        wr!(&format!("  Terminal size     : {}  columns × {}  rows", self.cols, self.rows),
            Color::White, false);
        wr!(&format!("  Character aspect  : {:.2}  (width/height in display pixels)",
            self.char_aspect), Color::White, false);
        wr!(&format!("  HalfBlock canvas  : {}  CPEs wide × {}  CPEs tall  (square pixels)",
            self.cols, self.rows * 2), Color::White, false);
        wr!(&format!("  Braille canvas    : {}  CPEs wide × {}  CPEs tall",
            self.cols * 2, self.rows * 4), Color::White, false);
        wr!("", Color::Reset, false);

        wr!(" ── Zoom Level Capabilities (HalfBlock mode, equator) ───────────", dim, false);
        wr!("  zoom   m/CPE       viewport extent          label", dim, false);

        let res = ConsoleResolution::new(RenderMode::HalfBlock);
        for z in 0u8..=20 {
            if row >= area.bottom().saturating_sub(1) { break; }
            let mpp = res.metres_per_cpe(z);
            let (lon, lat) = res.viewport_extent_deg(self.cols, self.rows, z, 0.0);
            let label = ConsoleResolution::zoom_label(z);
            let mpp_str = if mpp >= 1000.0 {
                format!("{:>9.1} km", mpp / 1000.0)
            } else {
                format!("{:>9.2}  m", mpp)
            };
            let line = format!(
                "   {:>2}   {}    {:>9.3}° × {:>8.3}°    {}", z, mpp_str, lon, lat, label
            );
            wr!(&line, Color::White, false);
        }
    }
}
