//! Keyboard shortcuts map to their actions, and only while they should.

use std::collections::HashSet;

use eframe::egui::{self, Event, Key, Modifiers, RawInput};
use vcb_app::shortcuts::{self, Action};

const COMMAND_SHIFT: Modifiers = Modifiers::COMMAND.plus(Modifiers::SHIFT);

#[test]
fn every_shortcut_maps_to_its_action() {
    let table = [
        (Key::O, Modifiers::COMMAND, Action::Open),
        (Key::S, Modifiers::COMMAND, Action::SavePreset),
        (Key::S, COMMAND_SHIFT, Action::SavePresetAs),
        (Key::E, Modifiers::COMMAND, Action::ExportImage),
        (Key::F, Modifiers::NONE, Action::Fit),
        (Key::Num1, Modifiers::NONE, Action::OneToOne),
        (Key::R, Modifiers::NONE, Action::NewSeed),
        (Key::ArrowLeft, Modifiers::NONE, Action::PreviousFrame),
        (Key::ArrowRight, Modifiers::NONE, Action::NextFrame),
        (Key::P, Modifiers::NONE, Action::Play),
        (Key::G, Modifiers::NONE, Action::Gallery),
    ];
    for (key, modifiers, action) in table {
        assert_eq!(
            shortcuts::action(key, modifiers, true),
            Some(action),
            "{key:?} with {modifiers:?}"
        );
    }
    let labels: HashSet<&str> = table.iter().map(|(.., action)| action.label()).collect();
    assert_eq!(labels.len(), table.len(), "every action has its own label");
}

#[test]
fn single_keys_wait_while_a_field_has_the_keyboard() {
    assert_eq!(shortcuts::action(Key::F, Modifiers::NONE, false), None);
    assert_eq!(
        shortcuts::action(Key::ArrowRight, Modifiers::NONE, false),
        None
    );
    assert_eq!(
        shortcuts::action(Key::S, Modifiers::COMMAND, false),
        Some(Action::SavePreset),
        "command shortcuts work while typing"
    );
}

#[test]
fn modifiers_must_match() {
    assert_eq!(
        shortcuts::action(Key::S, Modifiers::NONE, true),
        None,
        "S alone saves nothing"
    );
    assert_eq!(
        shortcuts::action(Key::F, Modifiers::COMMAND, true),
        None,
        "⌘F is not fit"
    );
    assert_eq!(
        shortcuts::action(Key::O, Modifiers::COMMAND.plus(Modifiers::ALT), true),
        None
    );
    assert_eq!(
        shortcuts::action(Key::R, Modifiers::SHIFT, true),
        Some(Action::NewSeed),
        "Shift does not matter for single keys"
    );
}

/// The actions `take` finds in one frame of key presses.
fn run(events: Vec<Event>) -> (Vec<Action>, usize) {
    let ctx = egui::Context::default();
    let mut actions = Vec::new();
    let mut left = 0;
    let input = RawInput {
        events,
        ..RawInput::default()
    };
    let mut output = ctx.run_ui(input, |ui| {
        actions = shortcuts::take(ui.ctx());
        left = ui.ctx().input(|input| input.events.len());
    });
    // No renderer uploads the font atlas here.
    output.textures_delta.clear();
    (actions, left)
}

fn press(key: Key, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

#[test]
fn key_presses_of_a_frame_become_actions_in_order() {
    let (actions, left) = run(vec![
        press(Key::S, COMMAND_SHIFT),
        press(Key::F, Modifiers::NONE),
        press(Key::Q, Modifiers::NONE),
        press(Key::ArrowRight, Modifiers::NONE),
        Event::Key {
            key: Key::P,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: Modifiers::NONE,
        },
    ]);
    assert_eq!(
        actions,
        vec![Action::SavePresetAs, Action::Fit, Action::NextFrame],
        "releases and unbound keys trigger nothing"
    );
    assert_eq!(
        left, 2,
        "the unbound key and the release stay for the widgets"
    );
}
