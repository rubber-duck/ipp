//! Metadata, sparse hidden producer fields and temporary component activation values.
//!
//! During a mutation batch an affected component stages its producer (`base_value`)
//! or shared fallback (`fallback_value`) once. [`ComponentStagedInput`] records where
//! the effective input lives: an unlayered component reads its staged producer or
//! fallback directly, so an ordered write costs only that write; overlay
//! contributions resolve into a separate layered copy. Commit clears every staged
//! value; retained storage is then authoritative again.

use crate::{ComponentValue, world::ComponentStateInstance};
use std::collections::BTreeSet;

/// Location of a component's staged effective input within one mutation batch.
#[derive(Default)]
pub(in crate::world) enum ComponentStagedInput {
    /// Nothing is staged: retained storage is the effective input.
    #[default]
    Unstaged,
    /// No overlay contributes: the staged producer value is the effective input.
    Base,
    /// No overlay contributes: the staged shared fallback is the effective input.
    Fallback,
    /// Overlay contributions applied to a separate copy of the producer or fallback.
    Layered(Box<ComponentValue>),
}

#[derive(Default)]
pub(in crate::world) struct ComponentStateOverlayInputs {
    pub(in crate::world) base: Option<ComponentStateInstance<()>>,
    pub(in crate::world) overlay_handles: BTreeSet<u64>,
    pub(in crate::world) creating_overlay: Option<u64>,
    pub(in crate::world) fallback: Option<ComponentStateInstance<()>>,
    pub(in crate::world) required: bool,
    pub(in crate::world) resolved: Option<ComponentStateInstance<()>>,
    // These payloads exist only while preparing an affected component, or when
    // forced cleanup must retain an authoritative value that cannot activate.
    pub(in crate::world) base_value: Option<Box<ComponentValue>>,
    pub(in crate::world) fallback_value: Option<Box<ComponentValue>>,
    pub(in crate::world) staged: ComponentStagedInput,
    pub(in crate::world) hidden_fields: Vec<(u32, crate::components::schema::FieldValue)>,
}

impl ComponentStateOverlayInputs {
    #[cfg(feature = "surfaces")]
    pub(in crate::world) fn supports_in_place_authored_mutation(&self) -> bool {
        self.base.is_some()
            && self.resolved.as_ref().map(|value| value.incarnation)
                == self.base.as_ref().map(|value| value.incarnation)
            && self.overlay_handles.is_empty()
            && self.creating_overlay.is_none()
            && self.fallback.is_none()
            && self.base_value.is_none()
            && self.fallback_value.is_none()
            && matches!(self.staged, ComponentStagedInput::Unstaged)
            && self.hidden_fields.is_empty()
    }

    pub(in crate::world) fn authored_base(&self) -> Option<&ComponentStateInstance<()>> {
        if self.creating_overlay.is_some() {
            None
        } else {
            self.base()
        }
    }

    pub(in crate::world) fn base(&self) -> Option<&ComponentStateInstance<()>> {
        self.base.as_ref()
    }

    pub(in crate::world) fn replace_base(
        &mut self,
        value: Option<ComponentStateInstance<ComponentValue>>,
    ) {
        self.base = value.as_ref().map(|value| ComponentStateInstance {
            base: (),
            incarnation: value.incarnation,
        });
        self.base_value = value.map(|value| Box::new(value.base));
        self.creating_overlay = None;
    }

    pub(in crate::world) fn input(&self) -> Option<&ComponentStateInstance<()>> {
        self.resolved.as_ref()
    }

    /// Hydrate the effective input from retained storage once per batch. Without
    /// hidden producer fields the retained value is the producer or fallback
    /// itself and moves there without a copy.
    pub(in crate::world) fn stage(&mut self, live: Option<ComponentValue>) {
        if !self.is_staged()
            && self.resolved.is_some()
            && let Some(live) = live
        {
            let unlayered = self.hidden_fields.is_empty();
            if unlayered && self.base.is_some() && self.base_value.is_none() {
                self.base_value = Some(Box::new(live));
                self.staged = ComponentStagedInput::Base;
            } else if unlayered
                && self.base.is_none()
                && self.fallback.is_some()
                && self.fallback_value.is_none()
            {
                self.fallback_value = Some(Box::new(live));
                self.staged = ComponentStagedInput::Fallback;
            } else {
                self.staged = ComponentStagedInput::Layered(Box::new(live));
            }
        }
        if self.base_value.is_none() && self.base.is_some() {
            self.base_value = self.staged_value().cloned().map(Box::new);
            if let Some(mut value) = self.base_value.take() {
                self.restore_producer(&mut value);
                self.base_value = Some(value);
            }
        }
        if self.fallback_value.is_none() && self.fallback.is_some() {
            self.fallback_value = self.staged_value().cloned().map(Box::new);
            if let Some(mut value) = self.fallback_value.take() {
                self.restore_producer(&mut value);
                self.fallback_value = Some(value);
            }
        }
    }

    /// The staged effective value regardless of activation state.
    pub(in crate::world) fn staged_value(&self) -> Option<&ComponentValue> {
        match &self.staged {
            ComponentStagedInput::Unstaged => None,
            ComponentStagedInput::Base => self.base_value.as_deref(),
            ComponentStagedInput::Fallback => self.fallback_value.as_deref(),
            ComponentStagedInput::Layered(value) => Some(value),
        }
    }

    /// Whether this batch holds a staged effective value; otherwise retained
    /// storage is authoritative.
    pub(in crate::world) fn is_staged(&self) -> bool {
        self.staged_value().is_some()
    }

    /// Whether the effective input is exactly the staged producer value, so an
    /// in-place producer write is also the complete effective change.
    pub(in crate::world) fn stages_producer_directly(&self) -> bool {
        matches!(self.staged, ComponentStagedInput::Base)
            && self.base_value.is_some()
            && self.resolved.is_some()
            && self.overlay_handles.is_empty()
            && self.hidden_fields.is_empty()
    }

    /// The separately resolved copy, present only while overlays contribute.
    pub(in crate::world) fn layered_value_mut(&mut self) -> Option<&mut ComponentValue> {
        match &mut self.staged {
            ComponentStagedInput::Layered(value) => Some(value),
            _ => None,
        }
    }

    pub(in crate::world) fn base_value(&self) -> Option<&ComponentValue> {
        self.base_value.as_deref()
    }

    pub(in crate::world) fn base_value_mut(&mut self) -> Option<&mut ComponentValue> {
        self.base_value.as_deref_mut()
    }

    pub(in crate::world) fn input_value(&self) -> Option<&ComponentValue> {
        self.resolved.as_ref()?;
        self.staged_value()
    }

    pub(in crate::world) fn restore_producer(&self, value: &mut ComponentValue) {
        for (offset, field) in &self.hidden_fields {
            value
                .set_field(*offset, field.clone())
                .expect("validated hidden producer field");
        }
    }

    pub(in crate::world) fn retained_inputs(&self) -> impl Iterator<Item = &ComponentValue> {
        let layered = match &self.staged {
            ComponentStagedInput::Layered(value) => Some(value.as_ref()),
            _ => None,
        };
        [
            self.base_value.as_deref(),
            self.fallback_value.as_deref(),
            layered,
        ]
        .into_iter()
        .flatten()
    }

    pub(in crate::world) fn deactivate(&mut self) {
        self.resolved = None;
        self.staged = ComponentStagedInput::Unstaged;
    }

    pub(in crate::world) fn finish_commit(&mut self) {
        self.staged = ComponentStagedInput::Unstaged;
        if self.resolved.is_some() {
            self.base_value = None;
            self.fallback_value = None;
        } else {
            self.hidden_fields.clear();
        }
    }
}
