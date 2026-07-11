// Color and theme management.
//
// Colors are stored as RGB triples and serialized as hex strings (#RRGGBB)
// for human readability. The theme list is the source of truth for presets;
// the config stores whichever theme the user chose (or a custom palette).

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

// ── Color ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Color { r, g, b }
    }

    /// Parse a hex color like "#FF6692" or "#ff6692"
    pub fn from_hex(hex: &str) -> Self {
        Self::from_hex_opt(hex).unwrap_or(Color::new(255, 255, 255))
    }

    pub fn from_hex_opt(hex: &str) -> Option<Self> {
        let hex = hex.strip_prefix('#').unwrap_or(hex);
        if hex.len() != 6 {
            return None;
        }
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        Some(Color { r, g, b })
    }

    #[allow(dead_code)]
    pub fn to_hex_string(&self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    /// ANSI true-color foreground escape sequence
    pub fn fg_escape(&self) -> String {
        format!("\x1b[38;2;{};{};{}m", self.r, self.g, self.b)
    }

    /// ANSI true-color background escape sequence
    #[allow(dead_code)]
    pub fn bg_escape(&self) -> String {
        format!("\x1b[48;2;{};{};{}m", self.r, self.g, self.b)
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
}

impl FromStr for Color {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_hex_opt(s).ok_or_else(|| format!("Invalid color: {}", s))
    }
}

// ── Theme ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Theme {
    pub name: &'static str,
    pub colors: Vec<Color>,
    pub description: &'static str,
}

/// All built-in themes.
/// The first 7 colors are used for ASCII art rendering; additional colors
/// beyond 7 are ignored but kept for future use.
macro_rules! theme {
    ($name:expr, $desc:expr, [$($c:expr),+ $(,)?]) => {
        Theme {
            name: $name,
            description: $desc,
            colors: vec![$(Color::from_hex($c)),+],
        }
    };
}

pub fn all_themes() -> Vec<Theme> {
    vec![
        // LGBTQ+ flags
        theme!("xenogender", "Pink-orange-yellow flag palette", ["#FF6692", "#FF9A98", "#FFB883", "#FBFFA8", "#85BCFF", "#9D85FF", "#A510FF"]),
        theme!("trans", "Light blue, pink, and white trans flag", ["#55CDFC", "#F7A8B8", "#FFFFFF", "#F7A8B8", "#55CDFC"]),
        theme!("nb", "Yellow, white, purple, black nonbinary flag", ["#FFF430", "#FFFFFF", "#9C59D1", "#2C2C2C"]),
        theme!("genderfluid", "Pink, white, purple, black, blue flag", ["#FF75A2", "#FFFFFF", "#C011D7", "#2C2C2C", "#3170D0"]),
        theme!("pan", "Pink, yellow, blue pansexual flag", ["#FF218C", "#FFD800", "#21B1FF"]),
        theme!("bi", "Pink, purple, blue bisexual flag", ["#D60270", "#9B4F96", "#0038A8"]),
        theme!("ace", "Black, gray, white, purple asexual flag", ["#000000", "#A4A4A4", "#FFFFFF", "#810081"]),
        theme!("lesbian", "Orange, white, pink lesbian flag", ["#D52D00", "#FF9A56", "#FFFFFF", "#D362A4", "#A30262"]),
        theme!("gay", "Green, white, blue, purple gay flag", ["#078D70", "#26CEAA", "#98E8C1", "#FFFFFF", "#7BADE2", "#5049CC", "#3D1A78"]),
        theme!("intersex", "Yellow and purple intersex flag", ["#FFD700", "#7902AA"]),
        theme!("aromantic", "Green, white, gray, black aromantic flag", ["#3DA542", "#A8D47A", "#FFFFFF", "#A8D47A", "#3DA542", "#000000"]),
        theme!("agender", "Black, gray, white, green agender flag", ["#000000", "#BABABA", "#FFFFFF", "#BABABA", "#000000"]),

        // Themes
        theme!("arch", "Arch Linux blue palette", ["#1793D1", "#1793D1", "#1793D1", "#1793D1", "#1793D1"]),
        theme!("catppuccin-mocha", "Warm dark theme with pastel accents", ["#f5c2e7", "#cba6f7", "#94e2d5", "#a6e3a1", "#f9e2af", "#fab387", "#89b4fa"]),
        theme!("catppuccin-latte", "Light theme with soft pastel accents", ["#dd7878", "#8839ef", "#40a02b", "#fe640b", "#df8e1d", "#04a5e5", "#209fb5"]),
        theme!("dracula", "Dark theme with vibrant neon accents", ["#ff5555", "#ff79c6", "#bd93f9", "#50fa7b", "#f1fa8c", "#ffb86c", "#8be9fd"]),
        theme!("gruvbox", "Earthy retro palette with warm tones", ["#cc241d", "#98971a", "#d79921", "#458588", "#b16286", "#689d6a", "#fb4934"]),
        theme!("tokyonight", "Deep blue theme inspired by VSCode", ["#f7768e", "#bb9af7", "#7dcfff", "#9ece6a", "#e0af68", "#73daca", "#ff9e64"]),
        theme!("nord", "Arctic, bluish pastel theme", ["#bf616a", "#d08770", "#ebcb8b", "#a3be8c", "#b48ead", "#88c0d0", "#81a1c1"]),
        theme!("everforest", "Warm green-toned theme", ["#e67e80", "#e69875", "#dbbc7f", "#a7c080", "#7fbbb3", "#83c092", "#d3c6aa"]),
        theme!("solarized-dark", "Earthy dark theme with muted accents", ["#dc322f", "#cb4b16", "#b58900", "#859900", "#6c71c4", "#268bd2", "#2aa198"]),
        theme!("monokai", "High-contrast dark theme", ["#f92672", "#fd971f", "#e6db74", "#a6e22e", "#66d9ef", "#ae81ff", "#f8f8f2"]),
        theme!("one-dark", "Atom-inspired dark theme", ["#e06c75", "#d19a66", "#e5c07b", "#98c379", "#56b6c2", "#61afef", "#c678dd"]),
        theme!("rose-pine", "Soft pine-green dark theme", ["#eb6f92", "#f6c177", "#ebbcba", "#31748f", "#9ccfd8", "#c4a7e7", "#e0def4"]),
        theme!("synthwave", "Retro synthwave neon palette", ["#ff7edb", "#ff7edb", "#36f9f6", "#36f9f6", "#ffe066", "#ffe066", "#b4a0ff"]),
    ]
}

#[allow(dead_code)]
pub fn find_theme(name: &str) -> Option<Theme> {
    all_themes().into_iter().find(|t| t.name == name)
}

/// The default theme name used for new configs.
#[allow(dead_code)]
pub const DEFAULT_THEME: &str = "xenogender";

/// Pre-computed list of theme names for the TUI.
#[allow(dead_code)]
pub const PRESET_THEMES: &[&str] = &[
    "xenogender", "trans", "nb", "genderfluid", "pan", "bi", "ace",
    "lesbian", "gay", "intersex", "aromantic", "agender",
    "arch", "catppuccin-mocha", "catppuccin-latte", "dracula",
    "gruvbox", "tokyonight", "nord", "everforest", "solarized-dark",
    "monokai", "one-dark", "rose-pine", "synthwave",
];
