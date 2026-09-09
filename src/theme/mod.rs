//! The theme seam.
//!
//! Views never name a color, a size, or a font. They name a role, and
//! the active theme resolves the role. This is the contract that later
//! lets a Winamp skin engine replace the built-in look without touching
//! a view.

mod default;

pub use default::DefaultTheme;

use egui::{Color32, FontId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorRole {
    Surface,
    Border,
    AccentSoft,
    OnAccent,
    PageBackground,
    PanelBackground,
    TextPrimary,
    TextSecondary,
    Accent,
    Danger,
    RowHover,
    ArtPlaceholder,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextRole {
    Hero,
    Title,
    Heading,
    Body,
    Caption,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricRole {
    PagePadding,
    GapSmall,
    GapLarge,
    RowHeight,
    PlayerBarHeight,
    SidebarWidth,
    RowArtSize,
    PlayerArtSize,
    CornerRadius,
    /// Seconds the pointer must rest on a track row before the row
    /// starts a hover prefetch.
    HoverPrefetchDelay,
}

pub trait Theme {
    fn color(&self, role: ColorRole) -> Color32;
    fn font(&self, role: TextRole) -> FontId;
    fn metric(&self, role: MetricRole) -> f32;
}

impl dyn Theme + '_ {
    /// A ready egui text widget for a role, in the theme's font and color.
    pub fn label(&self, role: TextRole, text: impl Into<String>) -> egui::RichText {
        egui::RichText::new(text.into())
            .font(self.font(role))
            .color(self.color(ColorRole::TextPrimary))
    }

    pub fn secondary_label(&self, role: TextRole, text: impl Into<String>) -> egui::RichText {
        egui::RichText::new(text.into())
            .font(self.font(role))
            .color(self.color(ColorRole::TextSecondary))
    }
}
