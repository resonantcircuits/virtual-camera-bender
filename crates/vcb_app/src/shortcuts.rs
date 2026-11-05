//! Keyboard shortcuts: which keys trigger which actions.
//!
//! Command shortcuts use the platform's command key (⌘ on macOS, Ctrl elsewhere) and
//! work everywhere. Single keys (F, 1, R, P, the arrows) act only while no text field
//! has the keyboard and no control holds the focus, so typing a value or nudging a
//! focused slider never steps the video. The space bar is held, not pressed: while it
//! is down the viewport shows the original ([`holding_original`]).

use eframe::egui::{self, Key, Modifiers};

/// Something the user can do from the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    /// Open an image or a video.
    Open,
    /// Save the preset to its file, asking for one if it has none.
    SavePreset,
    /// Save the preset to a new file.
    SavePresetAs,
    /// Export the full render as an image.
    ExportImage,
    /// Fit the image into the viewport.
    Fit,
    /// Show one image pixel per screen pixel.
    OneToOne,
    /// Draw a new noise seed.
    NewSeed,
    /// Show the previous video frame.
    PreviousFrame,
    /// Show the next video frame.
    NextFrame,
    /// Start or stop playing the video.
    Play,
}

/// Whether a shortcut needs the keyboard to be free of text fields and focused controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    /// Works even while a text field has the keyboard.
    Always,
    /// Works only while no widget has the keyboard focus.
    Free,
}

/// Every shortcut, the more specific before the less: ⌘⇧S before ⌘S.
const SHORTCUTS: [(Action, Modifiers, Key, Scope); 10] = [
    (Action::Open, Modifiers::COMMAND, Key::O, Scope::Always),
    (
        Action::SavePresetAs,
        Modifiers::COMMAND.plus(Modifiers::SHIFT),
        Key::S,
        Scope::Always,
    ),
    (
        Action::SavePreset,
        Modifiers::COMMAND,
        Key::S,
        Scope::Always,
    ),
    (
        Action::ExportImage,
        Modifiers::COMMAND,
        Key::E,
        Scope::Always,
    ),
    (Action::Fit, Modifiers::NONE, Key::F, Scope::Free),
    (Action::OneToOne, Modifiers::NONE, Key::Num1, Scope::Free),
    (Action::NewSeed, Modifiers::NONE, Key::R, Scope::Free),
    (
        Action::PreviousFrame,
        Modifiers::NONE,
        Key::ArrowLeft,
        Scope::Free,
    ),
    (
        Action::NextFrame,
        Modifiers::NONE,
        Key::ArrowRight,
        Scope::Free,
    ),
    (Action::Play, Modifiers::NONE, Key::P, Scope::Free),
];

impl Action {
    /// The shortcut's label for tooltips and the guide, such as "Shift+Cmd+S" or "F".
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "Cmd+O",
            Self::SavePreset => "Cmd+S",
            Self::SavePresetAs => "Shift+Cmd+S",
            Self::ExportImage => "Cmd+E",
            Self::Fit => "F",
            Self::OneToOne => "1",
            Self::NewSeed => "R",
            Self::PreviousFrame => "←",
            Self::NextFrame => "→",
            Self::Play => "P",
        }
    }
}

/// The action a key press triggers, if any; `free` says whether no widget has the focus.
///
/// Command shortcuts need the command key and allow nothing else but the listed
/// modifiers; single keys need no command key and ignore Shift and Alt.
#[must_use]
pub fn action(key: Key, modifiers: Modifiers, free: bool) -> Option<Action> {
    SHORTCUTS
        .iter()
        .find(|(_, wanted, shortcut_key, scope)| {
            *shortcut_key == key
                && (free || *scope == Scope::Always)
                && if wanted.command {
                    modifiers.command && modifiers.shift == wanted.shift && !modifiers.alt
                } else {
                    !modifiers.command && !modifiers.ctrl && !modifiers.mac_cmd
                }
        })
        .map(|&(action, ..)| action)
}

/// Takes the key presses of this frame that trigger actions, in the order pressed.
///
/// The presses are consumed, so no widget sees them as well.
#[must_use]
pub fn take(ctx: &egui::Context) -> Vec<Action> {
    let free = !ctx.egui_wants_keyboard_input() && ctx.memory(|memory| memory.focused().is_none());
    ctx.input_mut(|input| {
        let mut actions = Vec::new();
        input.events.retain(|event| {
            let egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = event
            else {
                return true;
            };
            match action(*key, *modifiers, free) {
                Some(found) => {
                    actions.push(found);
                    false
                }
                None => true,
            }
        });
        actions
    })
}

/// Whether the space bar is held down to show the original, outside text fields.
///
/// Consumes the press so that a focused button is not clicked by it.
#[must_use]
pub fn holding_original(ctx: &egui::Context) -> bool {
    !ctx.egui_wants_keyboard_input()
        && ctx.input_mut(|input| {
            input.consume_key(Modifiers::NONE, Key::Space);
            input.key_down(Key::Space)
        })
}
