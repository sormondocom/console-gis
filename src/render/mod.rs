pub mod globe;
pub mod canvas;
pub mod compat;
pub mod typography;

pub use canvas::{TerminalCapability, Canvas};
pub use typography::{GoldenLayout, PHI, PHI_INV};

/// Auto-detect terminal capability from environment variables.
/// Falls back gracefully toward VT-100 if nothing can be determined.
pub fn detect_capability() -> TerminalCapability {
    // Check $COLORTERM — set by many modern terminals
    if let Ok(ct) = std::env::var("COLORTERM") {
        let ct = ct.to_ascii_lowercase();
        if ct == "truecolor" || ct == "24bit" {
            return TerminalCapability::TrueColor;
        }
    }

    // Check $TERM_PROGRAM and $TERM for 256-colour support
    let term = std::env::var("TERM").unwrap_or_default();
    let term_program = std::env::var("TERM_PROGRAM").unwrap_or_default();

    if term.contains("256color")
        || term_program.contains("iTerm")
        || term_program.contains("vscode")
        || term_program == "Hyper"
    {
        return TerminalCapability::Color256;
    }

    // Windows: assume true-colour via Windows Terminal / ConPTY
    #[cfg(target_os = "windows")]
    {
        // Windows Terminal and modern PowerShell support true colour.
        // Older CMD falls back to ANSI-8.
        if std::env::var("WT_SESSION").is_ok()
            || std::env::var("ConEmuPID").is_ok()
        {
            return TerminalCapability::TrueColor;
        }
        return TerminalCapability::Ansi8;
    }

    // TERM values that indicate basic 8-colour ANSI
    if term.starts_with("xterm")
        || term.starts_with("rxvt")
        || term.starts_with("screen")
        || term.starts_with("tmux")
        || term == "linux"
    {
        return TerminalCapability::Ansi8;
    }

    // VT-100 / unknown — safest fallback
    TerminalCapability::Vt100
}
