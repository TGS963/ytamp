use super::*;
use crate::theme::DefaultTheme;

fn text_rect(shape: &egui::Shape, label: &str) -> Option<egui::Rect> {
    match shape {
        egui::Shape::Text(text) if text.galley.text() == label => {
            Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
        }
        egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| text_rect(shape, label)),
        _ => None,
    }
}

fn label_rect(output: &egui::FullOutput, label: &str) -> egui::Rect {
    output
        .shapes
        .iter()
        .find_map(|shape| text_rect(&shape.shape, label))
        .unwrap_or_else(|| panic!("missing label: {label}"))
}

fn frame(
    ctx: &egui::Context,
    state: &State,
    size: egui::Vec2,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, Vec<Action>) {
    let mut actions = vec![];
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ui| actions = view(ui, state, &DefaultTheme),
    );
    output.textures_delta.clear();
    (output, actions)
}

fn local_state() -> State {
    State {
        local_mode: true,
        ..Default::default()
    }
}

fn click(ctx: &egui::Context, state: &State, size: egui::Vec2, pos: egui::Pos2) -> Vec<Action> {
    let pointer = |pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    frame(
        ctx,
        state,
        size,
        vec![egui::Event::PointerMoved(pos), pointer(true)],
    );
    frame(ctx, state, size, vec![pointer(false)]).1
}

#[test]
fn errors_do_not_shift_the_page_or_cover_the_player() {
    for size in [egui::vec2(700., 480.), egui::vec2(1100., 720.)] {
        let ctx = egui::Context::default();
        let mut state = local_state();
        frame(&ctx, &state, size, vec![]);
        let (before, _) = frame(&ctx, &state, size, vec![]);
        let content = label_rect(&before, "Bring your music.");
        state.playback.error = Some("A long download error. ".repeat(100));
        state.notices = vec!["A file could not be read. ".repeat(100)];
        frame(&ctx, &state, size, vec![]);
        let (after, _) = frame(&ctx, &state, size, vec![]);
        assert_eq!(content, label_rect(&after, "Bring your music."));
        let heading = label_rect(&after, "Playback stopped");
        assert!(heading.top() >= 0. && heading.bottom() < size.y - 124.);
        assert!(heading.left() >= 0. && heading.right() <= size.x);
        assert!(!after.shapes.iter().any(|shape| {
            text_rect(&shape.shape, state.playback.error.as_ref().unwrap()).is_some()
        }));
    }
}

#[test]
fn notification_retry_and_dismiss_return_the_correct_actions() {
    let ctx = egui::Context::default();
    let size = egui::vec2(1100., 720.);
    let mut state = local_state();
    state.playback.error = Some("Download failed".into());
    state.notices = vec!["First notice".into(), "Second notice".into()];
    frame(&ctx, &state, size, vec![]);
    let (output, _) = frame(&ctx, &state, size, vec![]);
    let actions = click(&ctx, &state, size, label_rect(&output, "Retry").center());
    assert!(
        actions
            .iter()
            .any(|action| matches!(action, Action::PlaybackRetryRequested))
    );
    let (output, _) = frame(&ctx, &state, size, vec![]);
    let actions = click(&ctx, &state, size, label_rect(&output, "Dismiss").center());
    assert!(
        actions
            .iter()
            .any(|action| matches!(action, Action::NoticeDismissed(1)))
    );
}

#[test]
fn minimized_playback_error_reopens_after_recovery_and_copy_keeps_details() {
    let ctx = egui::Context::default();
    let size = egui::vec2(1100., 720.);
    let mut state = local_state();
    state.playback.error = Some("Download failed with diagnostic details".into());
    state.notices.push("A separate notice".into());
    frame(&ctx, &state, size, vec![]);
    let (output, _) = frame(&ctx, &state, size, vec![]);
    click(&ctx, &state, size, label_rect(&output, "Minimize").center());
    frame(&ctx, &state, size, vec![]);
    let (output, _) = frame(&ctx, &state, size, vec![]);
    assert!(
        !output
            .shapes
            .iter()
            .any(|s| text_rect(&s.shape, "Retry").is_some())
    );
    state.playback.error = None;
    frame(&ctx, &state, size, vec![]);
    state.playback.error = Some("Download failed with diagnostic details".into());
    frame(&ctx, &state, size, vec![]);
    let (output, _) = frame(&ctx, &state, size, vec![]);
    label_rect(&output, "Retry");
    click(
        &ctx,
        &state,
        size,
        label_rect(&output, "Technical details").center(),
    );
    for _ in 0..20 {
        frame(&ctx, &state, size, vec![]);
    }
    let (output, _) = frame(&ctx, &state, size, vec![]);
    let pos = label_rect(&output, "Copy details").center();
    frame(
        &ctx,
        &state,
        size,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
    );
    let (output, _) = frame(
        &ctx,
        &state,
        size,
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    assert!(output.platform_output.commands.iter().any(|cmd|
        matches!(cmd, egui::OutputCommand::CopyText(text) if Some(text) == state.playback.error.as_ref())));
}
