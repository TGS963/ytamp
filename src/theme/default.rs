//! The built-in dark theme: theme one of the seam.

use egui::{Color32, FontFamily, FontId};

use super::{ColorRole, MetricRole, TextRole, Theme};

pub struct DefaultTheme;

impl Theme for DefaultTheme {
    fn color(&self, role: ColorRole) -> Color32 {
        match role {
            ColorRole::PageBackground => Color32::from_rgb(18, 18, 20),
            ColorRole::PanelBackground => Color32::from_rgb(26, 26, 30),
            ColorRole::TextPrimary => Color32::from_rgb(235, 235, 235),
            ColorRole::TextSecondary => Color32::from_rgb(160, 160, 165),
            ColorRole::Accent => Color32::from_rgb(255, 64, 64),
            ColorRole::Danger => Color32::from_rgb(255, 110, 90),
            ColorRole::RowHover => Color32::from_rgb(38, 38, 44),
            ColorRole::ArtPlaceholder => Color32::from_rgb(40, 40, 46),
        }
    }

    fn font(&self, role: TextRole) -> FontId {
        let size = match role {
            TextRole::Title => 24.0,
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
            MetricRole::RowHeight => 36.0,
            MetricRole::PlayerBarHeight => 72.0,
            MetricRole::SidebarWidth => 200.0,
            MetricRole::RowArtSize => 24.0,
            MetricRole::PlayerArtSize => 56.0,
            MetricRole::CornerRadius => 4.0,
        }
    }
}
