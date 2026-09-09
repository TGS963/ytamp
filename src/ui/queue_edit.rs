//! Shared queue selection, keyboard and drag/drop behavior for both players.
use crate::core::{action::Action, queue::Queue};
use egui::{Id, Response, Ui};
use std::collections::BTreeSet;

#[derive(Clone, Default)]
pub struct Editor {
    pub selected: BTreeSet<usize>,
    anchor: Option<usize>,
    focus: Option<Id>,
    snapshot: Option<Queue>,
}
#[derive(Clone)]
struct DragSelection {
    selected: Vec<usize>,
    snapshot: Option<Queue>,
}
impl Editor {
    pub fn load(ctx: &egui::Context, queue: &Queue) -> Self {
        let mut editor = ctx.data_mut(|d| {
            d.get_temp::<Self>(Id::new("queue-editor"))
                .unwrap_or_default()
        });
        if editor.snapshot.as_ref() != Some(queue) {
            editor.selected.clear();
            editor.anchor = None;
            editor.snapshot = Some(queue.clone());
        }
        editor
    }
    pub fn store(self, ctx: &egui::Context) {
        ctx.data_mut(|d| d.insert_temp(Id::new("queue-editor"), self));
    }
    fn select(&mut self, index: usize, modifiers: egui::Modifiers) {
        if modifiers.shift {
            let anchor = self.anchor.unwrap_or(index);
            if !modifiers.command && !modifiers.ctrl {
                self.selected.clear();
            }
            self.selected.extend(anchor.min(index)..=anchor.max(index));
        } else if modifiers.command || modifiers.ctrl {
            if !self.selected.remove(&index) {
                self.selected.insert(index);
            }
            self.anchor = Some(index);
        } else {
            self.selected = BTreeSet::from([index]);
            self.anchor = Some(index);
        }
    }
    pub fn row(&mut self, ui: &Ui, response: &Response, index: usize, out: &mut Vec<Action>) {
        self.select_row(ui, response, index);
        if response.double_clicked() {
            out.push(Action::QueueJumped(index));
        }
        self.drag_row(ui, response, index, out);
    }

    fn select_row(&mut self, ui: &Ui, response: &Response, index: usize) {
        if response.clicked() {
            self.select(index, ui.input(|i| i.modifiers));
        }
        if (response.secondary_clicked() || response.drag_started())
            && !self.selected.contains(&index)
        {
            self.select(index, egui::Modifiers::NONE);
        }
        if response.clicked() || response.secondary_clicked() || response.drag_started() {
            response.request_focus();
            self.focus = Some(response.id);
        }
    }

    fn drag_row(&mut self, ui: &Ui, response: &Response, index: usize, out: &mut Vec<Action>) {
        response.dnd_set_drag_payload(DragSelection {
            selected: self.selected.iter().copied().collect(),
            snapshot: self.snapshot.clone(),
        });
        let after = ui
            .input(|i| i.pointer.interact_pos())
            .is_some_and(|p| p.y >= response.rect.center().y);
        if response.dnd_hover_payload::<DragSelection>().is_some() {
            let y = if after {
                response.rect.bottom()
            } else {
                response.rect.top()
            };
            ui.painter().hline(
                response.rect.x_range(),
                y,
                egui::Stroke::new(2., ui.visuals().selection.stroke.color),
            );
        }
        if let Some(payload) = response.dnd_release_payload::<DragSelection>()
            && payload.snapshot == self.snapshot
        {
            out.push(Action::QueueSelectionMoved {
                selected: payload.selected.clone(),
                before: index + usize::from(after),
            });
            self.selected.clear();
        }
    }
    pub fn keyboard(&mut self, ui: &Ui, count: usize, out: &mut Vec<Action>) {
        if !self.focus.is_some_and(|id| ui.memory(|m| m.has_focus(id))) {
            return;
        }
        ui.input_mut(|i| {
            if i.consume_key(egui::Modifiers::COMMAND, egui::Key::A) {
                self.selected.extend(0..count);
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)
            {
                self.remove(out);
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                && let Some(index) = self.selected.first()
            {
                out.push(Action::QueueJumped(*index));
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
                self.selected.clear();
            }
        });
    }
    pub fn remove(&mut self, out: &mut Vec<Action>) {
        if !self.selected.is_empty() {
            out.push(Action::QueueSelectionRemoved(
                self.selected.iter().copied().collect(),
            ));
            self.selected.clear();
        }
    }
    pub fn menu(&mut self, ui: &mut Ui, index: usize, out: &mut Vec<Action>) {
        if ui.button("Play now").clicked() {
            out.push(Action::QueueJumped(index));
            ui.close();
        }
        if ui.button("Play selected next").clicked() {
            out.push(Action::QueueSelectionMoved {
                selected: self.selected.iter().copied().collect(),
                before: 0,
            });
            ui.close();
        }
        if ui.button("Remove selected").clicked() {
            self.remove(out);
            ui.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shift_ranges_command_toggle_and_stale_selection() {
        let mut editor = Editor::default();
        editor.select(1, egui::Modifiers::NONE);
        editor.select(4, egui::Modifiers::SHIFT);
        assert_eq!(editor.selected, BTreeSet::from([1, 2, 3, 4]));
        editor.select(2, egui::Modifiers::COMMAND);
        assert_eq!(editor.selected, BTreeSet::from([1, 3, 4]));
        let mut out = vec![];
        editor.remove(&mut out);
        assert!(matches!(&out[0], Action::QueueSelectionRemoved(v) if v == &[1,3,4]));
        assert!(editor.selected.is_empty());
        let ctx = egui::Context::default();
        editor.select(10, egui::Modifiers::NONE);
        editor.store(&ctx);
        assert!(Editor::load(&ctx, &Queue::default()).selected.is_empty());
    }
}
