use super::*;
use crate::services::gui_input::{GuiInputError, GuiInputSession};
use std::sync::Arc;

/// Native edits name the exact focused control and transient edit generation.
///
/// The generation advances on every native edit, every focus change, every
/// write to the control's `text` that the native record did not make and
/// every commit, discard or other change of a numeric input's shown number,
/// so an edit prepared against older text never applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuiTextFence {
    /// Ordinary control lifetime, never a node-table identity.
    pub target: GuiEntityTarget,
    /// Monotonic local edit/focus generation, including selection and composition.
    pub generation: u64,
}

/// Provisional single-line composition, separate from the `text` field.
#[derive(Clone, Debug, PartialEq)]
pub struct GuiTextComposition {
    /// Provisional Unicode text, replaced on every change.
    pub text: Arc<str>,
    /// UTF-8 grapheme-boundary selection within the provisional string.
    pub selection: [u32; 2],
}

/// Immutable native-buffer synchronization; it never implies a completed frame.
#[derive(Clone, Debug, PartialEq)]
pub struct GuiNativeTextState {
    /// Exact state an asynchronous edit must still match.
    pub fence: GuiTextFence,
    /// The control's `text` field, shared with the component store, or a
    /// numeric input's edit text, which no field holds.
    pub text: Arc<str>,
    /// UTF-8 anchor and caret in `text`.
    pub selection: [u32; 2],
    /// Provisional composition, if any.
    pub composition: Option<Arc<GuiTextComposition>>,
}

/// Single-line native editing operations; the receiver owns all text and offsets.
#[derive(Clone, Debug, PartialEq)]
#[allow(missing_docs)]
pub enum GuiTextEdit {
    Insert(Arc<str>),
    Selection([u32; 2]),
    Compose(GuiTextComposition),
    CommitComposition,
    CancelComposition,
    Backspace,
    Delete,
    Left,
    Right,
    Home,
    End,
    SelectAll,
    Submit,
}

impl GuiTextEdit {
    /// Owned string capacity while this command awaits its existing ingress boundary.
    pub fn retained_bytes(&self) -> usize {
        match self {
            Self::Insert(text) => text.len(),
            Self::Compose(composition) => composition.text.len(),
            _ => 0,
        }
    }
}

pub(super) struct GuiNativeText {
    pub state: GuiNativeTextState,
    pub owner: GuiInputSession,
    /// A numeric input's committed number as it was formatted when the edit
    /// started or last committed; the edit is pending while the text differs.
    /// None for a text input without a number.
    pub basis: Option<Arc<str>>,
}

impl crate::WorldContext<'_> {
    /// Read a live native owner without copying committed or provisional strings.
    pub fn gui_native_text(
        &self,
        target: GuiEntityTarget,
        session: crate::services::gui_input::GuiInputSessionId,
    ) -> Option<&GuiNativeTextState> {
        let gui =
            self.system::<crate::systems::gui::GuiSystem>(crate::systems::gui::GuiSystem::ID)?;
        let native = gui.local.native_text.as_ref()?;
        if native.owner.id() != session {
            return None;
        }
        gui.local.native_text(target)
    }
}

impl GuiLocalState {
    /// The live native record of the focused `target`. Focus a command set has
    /// no owner, and the input session presenting it holds its record.
    pub(in crate::world::systems::gui) fn native_text(
        &self,
        target: GuiEntityTarget,
    ) -> Option<&GuiNativeTextState> {
        let native = self.native_text.as_ref()?;
        let (focused, owner) = self.focus.as_ref()?;
        (native.owner.is_live()
            && owner
                .as_ref()
                .is_none_or(|owner| owner.id() == native.owner.id())
            && *focused == target
            && native.state.fence.target == target)
            .then_some(&native.state)
    }

    /// Whether `session` holds the live native record of the focused `target`.
    pub(super) fn holds_native_text(
        &self,
        target: GuiEntityTarget,
        session: &GuiInputSession,
    ) -> bool {
        self.native_text(target).is_some()
            && self
                .native_text
                .as_ref()
                .is_some_and(|native| native.owner.id() == session.id())
    }
}

impl GuiNativeText {
    /// The control and text of the pending numeric edit this record holds:
    /// its text while it differs from its basis.
    pub(super) fn pending_number_edit(&self) -> Option<(GuiEntityTarget, Arc<str>)> {
        let basis = self.basis.as_ref()?;
        (*self.state.text != **basis).then(|| (self.state.fence.target, self.state.text.clone()))
    }

    /// A fresh record over `text` with the caret at its end.
    pub(super) fn state(
        target: GuiEntityTarget,
        text: Arc<str>,
        generation: u64,
    ) -> GuiNativeTextState {
        GuiNativeTextState {
            fence: GuiTextFence {
                target,
                generation,
            },
            selection: [text.len() as u32; 2],
            text,
            composition: None,
        }
    }

    /// A fresh record of `input`: over its text, or over a numeric input's
    /// formatted number, which is then its basis.
    pub(super) fn new(
        target: GuiEntityTarget,
        owner: GuiInputSession,
        input: &GuiTextInput,
        generation: u64,
    ) -> Self {
        let basis = input.numeric.then(|| input.formatted());
        GuiNativeText {
            state: Self::state(
                target,
                basis.clone().unwrap_or_else(|| input.text.clone()),
                generation,
            ),
            owner,
            basis,
        }
    }
}

impl GuiNativeTextState {
    /// The text shared as is, or a new value with the composition spliced in.
    pub(crate) fn display_text(&self) -> Arc<str> {
        if let Some(composition) = &self.composition {
            let [anchor, caret] = self.selection;
            format!(
                "{}{}{}",
                &self.text[..anchor.min(caret) as usize],
                composition.text,
                &self.text[anchor.max(caret) as usize..]
            )
            .into()
        } else {
            self.text.clone()
        }
    }

    pub(crate) fn display_selection(&self) -> [u32; 2] {
        if let Some(composition) = &self.composition {
            let start = self.selection[0].min(self.selection[1]);
            composition.selection.map(|offset| start + offset)
        } else {
            self.selection
        }
    }
}

/// A validated native edit, applied only after its terminal is prepared.
pub(super) struct GuiPreparedTextEdit {
    /// Resulting native record.
    pub state: GuiNativeTextState,
    /// Momentary effect, only for a submission.
    pub effect: Option<GuiLocalEffect>,
    /// Whether the `text` field changes.
    pub changed_text: bool,
    /// Whether the displayed text changes, which needs a remeasure.
    pub changed_display: bool,
}

impl GuiLocalState {
    /// Validate one native edit against the current record and `text` field.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn prepare_text_edit(
        &self,
        command: &GuiLocalCommand,
        control: super::control::GuiControl,
        text: Arc<str>,
        fence: GuiTextFence,
        edit: &GuiTextEdit,
        tick: u64,
        ancestry: Arc<[crate::EntityId]>,
    ) -> Result<GuiPreparedTextEdit, GuiInputError> {
        use super::text_edit;
        let invalid = GuiInputError::Local(GuiLocalActionError::InvalidValue);
        let native = self
            .native_text
            .as_ref()
            .filter(|native| {
                native.state.fence == fence
                    && native.owner.id() == command.input.session_lifetime().id()
            })
            .ok_or(GuiInputError::Unavailable)?;
        if self.native_text(control.target).is_none() || !Arc::ptr_eq(&native.state.text, &text) {
            return Err(GuiInputError::Unavailable);
        }
        let mut state = native.state.clone();
        let [anchor, caret] = state.selection;
        let mut edited = None;
        match edit {
            GuiTextEdit::Insert(payload) => {
                super::component::single_line_text(payload).map_err(|_| invalid)?;
                edited = Some(text_edit::insert_at(&text, anchor, caret, payload));
                state.composition = None;
            }
            GuiTextEdit::Selection(selection) => {
                if selection
                    .iter()
                    .any(|offset| !text_edit::is_boundary(&text, *offset))
                {
                    return Err(invalid);
                }
                state.selection = *selection;
                state.composition = None;
            }
            GuiTextEdit::Compose(composition) => {
                super::component::single_line_text(&composition.text).map_err(|_| invalid)?;
                if composition
                    .selection
                    .iter()
                    .any(|offset| !text_edit::is_boundary(&composition.text, *offset))
                {
                    return Err(invalid);
                }
                state.composition = Some(Arc::new(composition.clone()));
            }
            GuiTextEdit::CommitComposition => {
                if let Some(composition) = state.composition.take() {
                    edited = Some(text_edit::insert_at(
                        &text,
                        anchor,
                        caret,
                        &composition.text,
                    ));
                }
            }
            GuiTextEdit::CancelComposition => {
                state.composition = None;
            }
            GuiTextEdit::Backspace => {
                edited = text_edit::backspace(&text, caret, Some(anchor));
                state.composition = None;
            }
            GuiTextEdit::Delete => {
                edited = text_edit::delete_forward(&text, caret, Some(anchor));
                state.composition = None;
            }
            GuiTextEdit::Left | GuiTextEdit::Right | GuiTextEdit::Home | GuiTextEdit::End => {
                let offset = match edit {
                    GuiTextEdit::Left if anchor != caret => anchor.min(caret),
                    GuiTextEdit::Right if anchor != caret => anchor.max(caret),
                    GuiTextEdit::Left => text_edit::prev_boundary(&text, caret).unwrap_or(0),
                    GuiTextEdit::Right => {
                        text_edit::next_boundary(&text, caret).unwrap_or(text.len() as u32)
                    }
                    GuiTextEdit::Home => 0,
                    _ => text.len() as u32,
                };
                state.selection = [offset; 2];
                state.composition = None;
            }
            GuiTextEdit::SelectAll => {
                state.selection = [0, text.len() as u32];
                state.composition = None;
            }
            GuiTextEdit::Submit => {
                if state.composition.is_some() {
                    return Err(invalid);
                }
            }
        }
        let mut changed_text = false;
        if let Some(edited) = edited {
            super::component::single_line_text(&edited.text).map_err(|_| invalid)?;
            state.selection = [edited.caret; 2];
            if *edited.text != *text {
                changed_text = true;
                state.text = edited.text;
            }
        }
        let effect = matches!(edit, GuiTextEdit::Submit)
            .then(|| {
                let kind = GuiLocalEffectKind::Submitted(state.text.clone());
                Ok::<_, GuiInputError>(GuiLocalEffect {
                    id: self
                        .observations
                        .candidate_id(control.target.world, &kind)?,
                    target: control.target,
                    source: command.input.effect_source(),
                    tick,
                    ancestry,
                    kind,
                })
            })
            .transpose()?;
        state.fence.generation = self
            .native_generation
            .checked_add(1)
            .ok_or(GuiInputError::Capacity)?;

        // Layout measures the displayed text, which only the text and the
        // provisional run change; caret and selection moves only repaint.
        let changed_display = changed_text || state.composition != native.state.composition;
        Ok(GuiPreparedTextEdit {
            state,
            effect,
            changed_text,
            changed_display,
        })
    }
}
