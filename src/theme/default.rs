//! The built-in dark theme: theme one of the seam.

use egui::{Color32, FontFamily, FontId};

use super::{ColorRole, MetricRole, TextRole, Theme};

pub struct DefaultTheme;

impl Theme for DefaultTheme {
    fn color(&self, role: ColorRole) -> Color32 {
        match role {
            ColorRole::Surface => Color32::from_rgb(30, 31, 37),
            ColorRole::Border => Color32::from_rgb(48, 49, 58),
            ColorRole::AccentSoft => Color32::from_rgb(65, 30, 37),
            ColorRole::OnAccent => Color32::WHITE,
            ColorRole::PageBackground => Color32::from_rgb(17, 18, 22),
            ColorRole::PanelBackground => Color32::from_rgb(22, 23, 28),
            ColorRole::TextPrimary => Color32::from_rgb(235, 235, 235),
            ColorRole::TextSecondary => Color32::from_rgb(160, 160, 165),
            ColorRole::Accent => Color32::from_rgb(244, 77, 94),
            ColorRole::Danger => Color32::from_rgb(255, 110, 90),
            ColorRole::RowHover => Color32::from_rgb(38, 38, 44),
            ColorRole::ArtPlaceholder => Color32::from_rgb(40, 40, 46),
        }
    }

    fn font(&self, role: TextRole) -> FontId {
        let size = match role {
            TextRole::Hero => 30.0,
            TextRole::Title => 26.0,
            TextRole::Heading => 17.0,
            TextRole::Body => 14.0,
            TextRole::Caption => 12.0,
        };
        FontId::new(size, FontFamily::Proportional)
    }

    fn metric(&self, role: MetricRole) -> f32 {
        match role {
            MetricRole::PagePadding => 16.0,
            MetricRole::GapSmall => 6.0,
            MetricRole::GapLarge => 14.0,
            MetricRole::RowHeight => 48.0,
            MetricRole::PlayerBarHeight => 108.0,
            MetricRole::SidebarWidth => 196.0,
            MetricRole::RowArtSize => 34.0,
            MetricRole::PlayerArtSize => 56.0,
            MetricRole::CornerRadius => 8.0,
            MetricRole::HoverPrefetchDelay => 0.4,
        }
    }
}
