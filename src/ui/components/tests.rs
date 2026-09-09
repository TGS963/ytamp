use crate::theme::{ColorRole, Theme};
use egui::vec2;

use super::*;
use crate::theme::DefaultTheme;
#[test]
fn slider_trail_stops_at_solid_thumb_and_keyboard_changes_value() {
    let ctx = egui::Context::default();
    let mut value = 0.5_f64;
    let mut run = |events, focus| {
        let mut response = None;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    vec2(400., 200.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                crate::ui::apply_page_style(ui, &DefaultTheme);
                response = Some(slider(ui, &mut value, 0.0..=1.0, false, true));
                if focus {
                    response.as_ref().unwrap().request_focus();
                }
            },
        );
        output.textures_delta.clear();
        (output, response.unwrap())
    };
    run(vec![], false);
    let (output, response) = run(vec![], true);
    let thumb = output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Circle(c) if c.fill == DefaultTheme.color(ColorRole::TextPrimary) => {
                Some(c)
            }
            _ => None,
        })
        .unwrap();
    let trail = output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Rect(r) if r.fill == DefaultTheme.color(ColorRole::Accent) => Some(r),
            _ => None,
        })
        .unwrap();
    assert!((trail.rect.right() - thumb.center.x).abs() < 0.01);
    assert!((thumb.radius - 5.).abs() < 0.01);
    assert!(response.rect.contains(thumb.center));
    let (_, changed) = run(
        vec![egui::Event::Key {
            key: egui::Key::ArrowRight,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
        false,
    );
    assert!(changed.changed());
    assert!(value > 0.5);
}
#[test]
fn section_reserves_action_space_and_keyboard_activation_keeps_layout() {
    for width in [260., 494., 850., 1460.] {
        let ctx = egui::Context::default();
        let frame = |events, focus, enabled| {
            let mut result = None;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        vec2(width, 300.),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    crate::ui::apply_page_style(ui, &DefaultTheme);
                    let available = ui.available_rect_before_wrap();
                    let response = section(
                        ui,
                        &DefaultTheme,
                        "A very long collection heading that needs to make room for its action",
                        "View library →",
                        enabled,
                    );
                    if focus {
                        response.request_focus();
                    }
                    focus_outline(ui, &DefaultTheme);
                    let below = ui.label("Content").rect;
                    result = Some((response.rect, below, response.clicked(), available));
                },
            );
            output.textures_delta.clear();
            result.unwrap()
        };
        frame(vec![], false, true);
        let (button, below, _, available) = frame(vec![], false, true);
        assert!(
            available.contains_rect(button),
            "action escaped at width {width}"
        );
        assert!(
            (button.right() - available.right()).abs() < 1.,
            "action must align to right edge"
        );
        let hovered = frame(
            vec![egui::Event::PointerMoved(button.center())],
            false,
            true,
        );
        assert_eq!((hovered.0, hovered.1), (button, below));
        frame(vec![egui::Event::PointerGone], true, true);
        let activated = frame(
            vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
            false,
            true,
        );
        assert!(activated.2, "Enter must activate focused action");
        assert_eq!((activated.0, activated.1), (button, below));
        let disabled = frame(vec![], false, false);
        assert_eq!((disabled.0, disabled.1), (button, below));
    }
}
