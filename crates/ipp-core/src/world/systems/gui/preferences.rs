//! Presentation preferences of a World's GUI: GUI System state that a Host or
//! client changes by System command at the mutation boundary, saved with the
//! World like other System state.

use crate::ErrorReason;

/// How a World's GUI presents itself to its viewer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GuiPreferences {
    /// Snap every skin transition to its destination, including transitions
    /// already under way.
    pub reduced_motion: bool,
}

/// Sparse GUI System command. Omitted preferences keep their current values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GuiPreferencesUpdate {
    /// Optional replacement for [`GuiPreferences::reduced_motion`].
    pub reduced_motion: Option<bool>,
}

impl GuiPreferencesUpdate {
    /// The preferences after this update.
    pub fn applied_to(self, current: GuiPreferences) -> GuiPreferences {
        GuiPreferences {
            reduced_motion: self.reduced_motion.unwrap_or(current.reduced_motion),
        }
    }
}

impl GuiPreferences {
    /// The persistent payload: one flags byte, or none for the defaults, so a
    /// World that never changed them saves nothing.
    pub(super) fn encode_persistent(self) -> Option<Vec<u8>> {
        (self != Self::default()).then(|| vec![u8::from(self.reduced_motion)])
    }

    /// Decode a persistent payload; any other length or flag fails the load.
    pub(super) fn decode_persistent(bytes: &[u8]) -> Result<Self, String> {
        match bytes {
            [flags] if *flags <= 1 => Ok(Self {
                reduced_motion: *flags == 1,
            }),
            _ => Err("Invalid GUI preferences".into()),
        }
    }
}

impl crate::WorldContext<'_> {
    /// Queue a sparse GUI preferences update at the ordered mutation boundary.
    pub fn enqueue_gui_preferences_update(
        &mut self,
        update: GuiPreferencesUpdate,
    ) -> Result<(), ErrorReason> {
        self.enqueue_system_command(super::GuiSystem::ID, 0, update)
    }

    /// `GuiPreferences` System query: the committed preferences; refused for
    /// a World that does not select the GUI System.
    pub fn gui_preferences(&self) -> Result<GuiPreferences, ErrorReason> {
        self.system::<super::GuiSystem>(super::GuiSystem::ID)
            .map(|gui| gui.preferences)
            .ok_or(ErrorReason::UnsupportedDependency)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_save_only_when_changed_and_reject_any_other_payload() {
        let reduced = GuiPreferences {
            reduced_motion: true,
        };
        assert_eq!(GuiPreferences::default().encode_persistent(), None);
        assert_eq!(reduced.encode_persistent(), Some(vec![1]));
        assert_eq!(GuiPreferences::decode_persistent(&[1]), Ok(reduced));
        assert_eq!(
            GuiPreferences::decode_persistent(&[0]),
            Ok(GuiPreferences::default())
        );
        for invalid in [&[][..], &[2], &[1, 0]] {
            assert!(GuiPreferences::decode_persistent(invalid).is_err());
        }
        assert_eq!(GuiPreferencesUpdate::default().applied_to(reduced), reduced);
    }
}
