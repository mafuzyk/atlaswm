// Layout definitions.
//
// Each layout controls how the ASCII art and panels are positioned relative
// to each other. `Centered` is the default and matches the original atlasfetch
// design. Other layouts offer progressively different arrangements.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum AppLayout {
    /// ASCII centered, panels on left and right (original atlasfetch)
    Centered,
    /// Smaller gap, tighter spacing
    Compact,
    /// Panels far apart, lots of breathing room
    Wide,
    /// No ASCII, panels only — for narrow terminals
    Minimal,
    /// Like Centered but with extra spacing around the logo
    Balanced,
}

impl AppLayout {
    pub fn variants() -> &'static [AppLayout] {
        &[
            AppLayout::Centered,
            AppLayout::Compact,
            AppLayout::Wide,
            AppLayout::Minimal,
            AppLayout::Balanced,
        ]
    }

    pub fn name(&self) -> &'static str {
        match self {
            AppLayout::Centered => "Centered",
            AppLayout::Compact => "Compact",
            AppLayout::Wide => "Wide",
            AppLayout::Minimal => "Minimal",
            AppLayout::Balanced => "Balanced",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            AppLayout::Centered => "ASCII centered, left/right powerline panels",
            AppLayout::Compact => "Tight spacing for smaller terminals",
            AppLayout::Wide => "Extra breathing room around elements",
            AppLayout::Minimal => "Panels only — no ASCII art",
            AppLayout::Balanced => "Like Centered with extra logo spacing",
        }
    }

    /// Returns the gap (spaces between ASCII edge and panel)
    pub fn gap(&self) -> usize {
        match self {
            AppLayout::Centered => 2,
            AppLayout::Compact => 1,
            AppLayout::Wide => 4,
            AppLayout::Minimal => 2,
            AppLayout::Balanced => 3,
        }
    }

    /// Returns the left/right padding around panels
    pub fn padding(&self) -> usize {
        match self {
            AppLayout::Centered => 3,
            AppLayout::Compact => 1,
            AppLayout::Wide => 4,
            AppLayout::Minimal => 2,
            AppLayout::Balanced => 3,
        }
    }
}
