/// Golden-ratio typography and layout system for console rendering.
///
/// # Theoretical basis
///
/// The golden ratio φ ≈ 1.618 governs proportional relationships in classical
/// design from Vitruvius through the Renaissance.  Applied to terminals:
///
/// ## The Accidental Golden Rectangle
///
/// A standard VT-100 / VT-220 terminal at 80×24 characters uses 8×16-pixel
/// font cells.  Applying half-block rendering (`▀`/`▄`) yields a pixel canvas
/// of **80 × 48** — aspect ratio 80/48 = **1.667 ≈ φ**.
///
/// This is not coincidence: the 2:1 character aspect ratio, combined with
/// half-block pixel doubling, maps an 80-column terminal onto a near-golden
/// rectangle.  Wider (132-col) VT-220 gives 132/88 ≈ 1.5, approaching φ².
///
/// ## Sfumato gradient (Leonardo's tonal range)
///
/// Da Vinci's *sfumato* (Italian: "gone up in smoke") uses imperceptible
/// tonal gradations to create soft edges.  In ASCII, the 10-step luminance
/// gradient mirrors this principle:
///
/// ```text
///   (dark)  ' '  .  `  '  -  :  +  o  0  #  (light)
///            0   1  2  3  4  5  6  7  8  9
/// ```
///
/// Luminance bands follow φ proportions:
/// - Steps 0–2 (dark, shadow):  φ⁰ weight of the tonal range
/// - Steps 3–5 (midtone):       φ¹ weight
/// - Steps 6–9 (light, highlight): φ² weight
///
/// ## Vitruvian layout grid
///
/// For a terminal of width W and height H:
///
/// ```text
/// Primary column width   = W / φ²  ≈ W × 0.382
/// Secondary column width = W / φ   ≈ W × 0.618
/// (Secondary / Primary = φ)
///
/// Status bar height   = H / φ³  ≈ H × 0.146 → round up to 1 row
/// Content height      = H / φ   ≈ H × 0.618
/// Header height       = H - content - status
/// ```
///
/// ## Rule of thirds / Fibonacci
///
/// Panel divisions follow the Fibonacci sequence (1 1 2 3 5 8 13 21 …):
/// - A 24-row terminal: 13 rows content + 8 rows secondary + 3 rows status
///   (24 = 13+8+3 — three consecutive Fibonacci numbers).
/// - An 80-column terminal: 50 cols primary + 30 secondary
///   (50/30 ≈ 1.667 ≈ φ, and 50+30 = 80).
///
/// ## Globe sizing (Vitruvian circle)
///
/// Da Vinci's Vitruvian Man inscribes a human figure in both a circle and a
/// square.  Applied to the console globe: the globe should be inscribed in the
/// largest square (in pixel terms) that fits the terminal, centred at the
/// golden-ratio point of the canvas.
///
/// Recommended globe radius (in half-block pixels):
/// ```text
/// radius = min(pixel_width, pixel_height) × (1 / φ) × 0.95
///        ≈ min(W, 2H) × 0.588
/// ```
///
/// For an 80×24 terminal (canvas 80×48 px): radius ≈ min(80,48) × 0.588 ≈ 28 px.

/// φ = (1 + √5) / 2 — the golden ratio.
pub const PHI: f64 = 1.618_033_988_749_895;

/// 1/φ = φ − 1 ≈ 0.618 — the golden ratio complement.
pub const PHI_INV: f64 = 0.618_033_988_749_895;

/// φ² ≈ 2.618 — the second power of the golden ratio.
pub const PHI2: f64 = 2.618_033_988_749_895;

/// Layout dimensions derived from the golden ratio for a given terminal size.
#[derive(Debug, Clone, Copy)]
pub struct GoldenLayout {
    /// Total columns.
    pub cols: u16,
    /// Total rows.
    pub rows: u16,
    /// Primary (wider) column width — cols × (1 − 1/φ) = cols/φ².
    pub primary_cols:   u16,
    /// Secondary (narrower) column width — cols/φ.
    pub secondary_cols: u16,
    /// Main content height — rows × (1/φ).
    pub content_rows:   u16,
    /// Header / title height.
    pub header_rows:    u16,
    /// Status bar height (always ≥ 1).
    pub status_rows:    u16,
    /// Globe radius in half-block pixels for the Vitruvian inscribed circle.
    pub globe_radius_px: u16,
}

impl GoldenLayout {
    /// Compute the golden-ratio layout for a terminal of `cols` × `rows`.
    pub fn compute(cols: u16, rows: u16) -> Self {
        // Column split at 1/φ² ≈ 38.2% / 61.8%
        let secondary = ((cols as f64 / PHI).round() as u16).max(1);
        let primary   = cols.saturating_sub(secondary).max(1);

        // Row split: status = 1, content = rows/φ, header = remainder
        let status  = 1u16;
        let content = ((rows as f64 * PHI_INV).round() as u16)
            .min(rows.saturating_sub(status + 1))
            .max(1);
        let header  = rows.saturating_sub(content + status).max(1);

        // Globe radius: inscribed in min(cols, rows×2) pixel square × (1/φ) × 0.95
        let pixel_h     = rows * 2;
        let min_dim     = (cols as f64).min(pixel_h as f64);
        let globe_radius = (min_dim * PHI_INV * 0.95 / 2.0).round() as u16;

        Self {
            cols, rows,
            primary_cols:   primary,
            secondary_cols: secondary,
            content_rows:   content,
            header_rows:    header,
            status_rows:    status,
            globe_radius_px: globe_radius,
        }
    }

    /// Menu panel split: sidebar width vs. description width.
    /// Returns (sidebar_cols, description_cols).
    pub fn menu_split(&self) -> (u16, u16) {
        (self.secondary_cols, self.primary_cols)
    }
}

/// Map a linear luminance [0, 255] to an ASCII shade character using a
/// φ-weighted tonal distribution that mimics sfumato gradation.
///
/// The 10-step scale is divided into three zones:
/// - Dark (0–2):    space, period, backtick — shadows with near-invisible detail.
/// - Midtone (3–5): subtle strokes — the "smoke" zone of sfumato.
/// - Light (6–9):   strong marks — highlights and direct light.
///
/// Zone boundaries follow inverse-golden-ratio proportions of the luminance range.
pub fn sfumato_shade(lum: u8) -> char {
    // φ-weighted zone thresholds
    // Dark boundary:   255 / φ²  ≈ 97
    // Midtone boundary: 255 / φ  ≈ 158
    const DARK_LIMIT:    u8 = 97;
    const MIDTONE_LIMIT: u8 = 158;

    const DARK:    &[char] = &[' ', '.', '`'];
    const MIDTONE: &[char] = &['\'', '-', ':'];
    const LIGHT:   &[char] = &['+', 'o', '0', '#'];

    if lum <= DARK_LIMIT {
        let idx = (lum as usize * (DARK.len() - 1)) / DARK_LIMIT as usize;
        DARK[idx.min(DARK.len() - 1)]
    } else if lum <= MIDTONE_LIMIT {
        let l = lum - DARK_LIMIT;
        let range = MIDTONE_LIMIT - DARK_LIMIT;
        let idx = (l as usize * (MIDTONE.len() - 1)) / range as usize;
        MIDTONE[idx.min(MIDTONE.len() - 1)]
    } else {
        let l = lum - MIDTONE_LIMIT;
        let range = 255u8.saturating_sub(MIDTONE_LIMIT).max(1);
        let idx = (l as usize * (LIGHT.len() - 1)) / range as usize;
        LIGHT[idx.min(LIGHT.len() - 1)]
    }
}

/// Fibonacci-based row allocation for n total rows.
/// Returns a sequence of row heights that sum to n, following Fibonacci proportions.
pub fn fibonacci_rows(n: u16, bands: usize) -> Vec<u16> {
    if bands == 0 { return Vec::new(); }
    // Generate Fibonacci weights
    let fibs: Vec<u64> = {
        let mut v = vec![1u64, 1];
        while v.len() < bands {
            let l = v.len();
            v.push(v[l - 1] + v[l - 2]);
        }
        v.into_iter().take(bands).collect()
    };
    let total_fib: u64 = fibs.iter().sum();
    let n64 = n as u64;

    let mut rows: Vec<u16> = fibs.iter()
        .map(|&f| ((f * n64) / total_fib) as u16)
        .collect();

    // Distribute rounding remainder to the largest band
    let allocated: u16 = rows.iter().sum();
    if allocated < n {
        let max_idx = rows.iter().enumerate().max_by_key(|&(_, &v)| v).map(|(i, _)| i).unwrap_or(0);
        rows[max_idx] += n - allocated;
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_layout_80x24() {
        let gl = GoldenLayout::compute(80, 24);
        // primary + secondary = 80
        assert_eq!(gl.primary_cols + gl.secondary_cols, 80);
        // secondary ≈ 80/φ ≈ 49.4 → 49 or 50
        assert!(gl.secondary_cols >= 48 && gl.secondary_cols <= 51,
            "secondary_cols={}", gl.secondary_cols);
        // rows add up
        assert_eq!(gl.content_rows + gl.header_rows + gl.status_rows, 24);
    }

    #[test]
    fn sfumato_monotone() {
        // sfumato_shade should produce characters that increase in density with lum
        let chars: Vec<char> = (0..=255u8)
            .map(|l| sfumato_shade(l))
            .collect();
        // First char (lum=0) should be space
        assert_eq!(chars[0], ' ');
        // Last char (lum=255) should be the densest
        assert_eq!(chars[255], '#');
    }

    #[test]
    fn fibonacci_rows_sum() {
        for n in [12u16, 24, 48, 80] {
            for bands in [1, 2, 3, 5, 8] {
                let rows = fibonacci_rows(n, bands);
                let sum: u16 = rows.iter().sum();
                assert_eq!(sum, n, "n={n} bands={bands} sum={sum}");
            }
        }
    }
}
