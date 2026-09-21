//! Metadata, sparse hidden producer fields and temporary component activation values.

use crate::{ComponentValue, world::ComponentStateInstance};
use std::collections::BTreeSet;

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
    pub(in crate::world) resolved_value: Option<Box<ComponentValue>>,
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
            && self.resolved_value.is_none()
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

    pub(in crate::world) fn stage(&mut self, live: Option<ComponentValue>) {
        if self.resolved_value.is_none() && self.resolved.is_some() {
            self.resolved_value = live.map(Box::new);
        }
        if self.base_value.is_none() && self.base.is_some() {
            self.base_value = self.resolved_value.clone();
            if let Some(mut value) = self.base_value.take() {
                self.restore_producer(&mut value);
                self.base_value = Some(value);
            }
        }
        if self.fallback_value.is_none() && self.fallback.is_some() {
            self.fallback_value = self.resolved_value.clone();
            if let Some(mut value) = self.fallback_value.take() {
                self.restore_producer(&mut value);
                self.fallback_value = Some(value);
            }
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
        self.resolved_value.as_deref()
    }

    pub(in crate::world) fn restore_producer(&self, value: &mut ComponentValue) {
        for (offset, field) in &self.hidden_fields {
            value
                .set_field(*offset, field.clone())
                .expect("validated hidden producer field");
        }
    }

    pub(in crate::world) fn retained_inputs(&self) -> impl Iterator<Item = &ComponentValue> {
        [
            self.base_value.as_deref(),
            self.fallback_value.as_deref(),
            self.resolved_value.as_deref(),
        ]
        .into_iter()
        .flatten()
    }

    pub(in crate::world) fn deactivate(&mut self) {
        self.resolved = None;
        self.resolved_value = None;
    }

    pub(in crate::world) fn finish_commit(&mut self) {
        self.resolved_value = None;
        if self.resolved.is_some() {
            self.base_value = None;
            self.fallback_value = None;
        } else {
            self.hidden_fields.clear();
        }
    }
}
