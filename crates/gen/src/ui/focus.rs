//! Whether a text field has the keyboard.
//!
//! The canvas's keys (`W A S D`, `Y`, `L`, `G`, the nudges) are the
//! world's unless a person is typing into a panel. egui answers that
//! question with `egui_wants_keyboard_input`; a `bevy_ui` text input
//! answers it through `bevy_input_focus`: the focused entity is an
//! `EditableText`. With the `feathers` feature both are asked; without it
//! only egui has text fields, and this says nothing.

use bevy::ecs::system::SystemParam;
#[cfg(feature = "feathers")]
use bevy::prelude::*;

/// The `bevy_ui` side of "is the person typing?", as a system parameter.
#[derive(SystemParam)]
pub struct TypingState<'w, 's> {
    #[cfg(feature = "feathers")]
    focus: Option<Res<'w, bevy::input_focus::InputFocus>>,
    #[cfg(feature = "feathers")]
    editable: Query<'w, 's, (), With<bevy::text::EditableText>>,
    #[cfg(not(feature = "feathers"))]
    _none: std::marker::PhantomData<(&'w (), &'s ())>,
}

impl TypingState<'_, '_> {
    /// A `bevy_ui` text field has the keyboard.
    pub fn typing(&self) -> bool {
        #[cfg(feature = "feathers")]
        {
            self.focus
                .as_ref()
                .and_then(|focus| focus.get())
                .is_some_and(|entity| self.editable.contains(entity))
        }
        #[cfg(not(feature = "feathers"))]
        {
            false
        }
    }
}

/// Run condition: no `bevy_ui` text field has the keyboard.
pub fn not_typing(state: TypingState) -> bool {
    !state.typing()
}
