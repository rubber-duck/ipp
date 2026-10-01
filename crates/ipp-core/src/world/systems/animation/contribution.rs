//! Contributions: what an additive driver adds to a field.
//!
//! A field of a linear type (floats, float vectors and matrices) or a rotation
//! holds its base plus the contributions of the controllers that drive it. A
//! controller remembers only its own contribution (`applied`) and moves the
//! field by the change of that contribution; nothing remembers the base.
//! Rotations compose on the right: the field is `base ⊗ c₁ ⊗ c₂ …`. Other value
//! types have no delta and are written absolutely.

use super::{
    AnimationValue,
    driver::{AnimationDriverBinding, AnimationTargetIdentity},
    math,
};
use crate::{ErrorReason, components::schema::FieldValue};

/// Whether values of this type are contributions rather than absolute writes.
pub(in crate::world) fn contributes(value: &AnimationValue) -> bool {
    match value {
        AnimationValue::Field(FieldValue::F32(_)) | AnimationValue::Rotation(_) => true,
        AnimationValue::Field(FieldValue::Dynamic(value)) => value.floats().is_some(),
        AnimationValue::Pose(_) => true,
        _ => false,
    }
}

/// The empty contribution of a value's type.
pub(in crate::world) fn identity_value(value: &AnimationValue) -> AnimationValue {
    match value {
        AnimationValue::Field(FieldValue::F32(_)) => AnimationValue::Field(FieldValue::F32(0.0)),
        AnimationValue::Rotation(_) => AnimationValue::Rotation(IDENTITY_ROTATION),
        AnimationValue::Field(FieldValue::Dynamic(value)) => {
            AnimationValue::Field(FieldValue::Dynamic(
                crate::DynamicValue::weighted(&[value], &[0.0]).unwrap_or_else(|_| value.clone()),
            ))
        }
        AnimationValue::Pose(values) => AnimationValue::Pose(vec![IDENTITY_JOINT; values.len()]),
        value => value.clone(),
    }
}

/// Whether a contribution adds nothing.
pub(in crate::world) fn is_identity(value: &AnimationValue) -> bool {
    *value == identity_value(value)
}

const IDENTITY_ROTATION: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// Joint contribution that adds nothing: zero translation and scale change,
/// identity rotation.
pub(in crate::world) const IDENTITY_JOINT: crate::components::Transform =
    crate::components::Transform {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        qx: 0.0,
        qy: 0.0,
        qz: 0.0,
        qw: 1.0,
        sx: 0.0,
        sy: 0.0,
        sz: 0.0,
    };

/// The weighted change from `reference` to `sample`.
pub(in crate::world) fn delta(
    sample: &AnimationValue,
    reference: &AnimationValue,
    weight: f64,
) -> Result<AnimationValue, ErrorReason> {
    math::additive(&identity_value(sample), sample, reference, weight)
}

/// `a` followed by `b`.
pub(in crate::world) fn compose(
    a: &AnimationValue,
    b: &AnimationValue,
) -> Result<AnimationValue, ErrorReason> {
    Ok(match (a, b) {
        (AnimationValue::Field(FieldValue::F32(a)), AnimationValue::Field(FieldValue::F32(b))) => {
            AnimationValue::Field(FieldValue::F32(a + b))
        }
        (AnimationValue::Rotation(a), AnimationValue::Rotation(b)) => {
            AnimationValue::Rotation(math::multiply(*a, *b))
        }
        (
            AnimationValue::Field(FieldValue::Dynamic(a)),
            AnimationValue::Field(FieldValue::Dynamic(b)),
        ) => AnimationValue::Field(FieldValue::Dynamic(
            crate::DynamicValue::weighted(&[a, b], &[1.0, 1.0])
                .map_err(|_| ErrorReason::InvalidValue)?,
        )),
        (AnimationValue::Pose(a), AnimationValue::Pose(b)) if a.len() == b.len() => {
            AnimationValue::Pose(a.iter().zip(b).map(|(a, b)| compose_joint(a, b)).collect())
        }
        _ => return Err(ErrorReason::InvalidField),
    })
}

/// Move `current` from holding `applied` to holding `total`.
pub(in crate::world) fn reapply(
    current: &AnimationValue,
    applied: &AnimationValue,
    total: &AnimationValue,
) -> Result<AnimationValue, ErrorReason> {
    Ok(match (current, applied, total) {
        (
            AnimationValue::Field(FieldValue::F32(current)),
            AnimationValue::Field(FieldValue::F32(applied)),
            AnimationValue::Field(FieldValue::F32(total)),
        ) => AnimationValue::Field(FieldValue::F32(
            (f64::from(*current) + f64::from(*total) - f64::from(*applied)) as f32,
        )),
        (
            AnimationValue::Rotation(current),
            AnimationValue::Rotation(applied),
            AnimationValue::Rotation(total),
        ) => AnimationValue::Rotation(math::multiply(
            math::multiply(*current, inverse(*applied)),
            *total,
        )),
        (
            AnimationValue::Field(FieldValue::Dynamic(current)),
            AnimationValue::Field(FieldValue::Dynamic(applied)),
            AnimationValue::Field(FieldValue::Dynamic(total)),
        ) => AnimationValue::Field(FieldValue::Dynamic(
            crate::DynamicValue::weighted(&[current, applied, total], &[1.0, -1.0, 1.0])
                .map_err(|_| ErrorReason::InvalidValue)?,
        )),
        _ => return Err(ErrorReason::InvalidField),
    })
}

fn inverse([x, y, z, w]: [f32; 4]) -> [f32; 4] {
    [-x, -y, -z, w]
}

/// A joint's local transform with `contribution` applied.
pub(in crate::world) fn compose_joint(
    current: &crate::components::Transform,
    contribution: &crate::components::Transform,
) -> crate::components::Transform {
    let [qx, qy, qz, qw] = math::multiply(
        [current.qx, current.qy, current.qz, current.qw],
        [
            contribution.qx,
            contribution.qy,
            contribution.qz,
            contribution.qw,
        ],
    );
    crate::components::Transform {
        x: current.x + contribution.x,
        y: current.y + contribution.y,
        z: current.z + contribution.z,
        qx,
        qy,
        qz,
        qw,
        sx: current.sx + contribution.sx,
        sy: current.sy + contribution.sy,
        sz: current.sz + contribution.sz,
    }
}

/// One controller's contribution to one field.
///
/// `total` is the contribution the controller last applied. For float fields,
/// `lanes` holds exactly what the field contains of it: the sum of the changes
/// the controller's writes made, kept in f64 so that rounding each written f32
/// never accumulates and withdrawing returns the field to its base. Rotations
/// keep only their total.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::world) struct AnimationApplied {
    total: AnimationValue,
    lanes: Vec<f64>,
}

impl AnimationApplied {
    /// A contribution the field is known to hold, such as a saved one.
    pub(in crate::world) fn new(total: AnimationValue) -> Self {
        let lanes = float_lanes(&total)
            .map(|lanes| lanes.map(f64::from).collect())
            .unwrap_or_default();
        Self {
            total,
            lanes,
        }
    }

    /// The contribution the field holds, rounded to its type.
    pub(in crate::world) fn value(&self) -> AnimationValue {
        if self.lanes.is_empty() {
            return self.total.clone();
        }
        with_lanes(&self.total, |index| self.lanes[index]).unwrap_or_else(|_| self.total.clone())
    }

    /// Whether the field holds nothing of this contribution.
    pub(in crate::world) fn is_empty(&self) -> bool {
        if self.lanes.is_empty() {
            is_identity(&self.total)
        } else {
            self.lanes.iter().all(|lane| *lane == 0.0)
        }
    }

    /// The field's value once it holds `total` instead of this contribution.
    pub(in crate::world) fn moved(
        &self,
        current: &AnimationValue,
        total: &AnimationValue,
    ) -> Result<AnimationValue, ErrorReason> {
        if self.lanes.is_empty() {
            return reapply(current, &self.total, total);
        }
        let (Some(current_lanes), Some(total_lanes)) = (float_lanes(current), float_lanes(total))
        else {
            return Err(ErrorReason::InvalidField);
        };
        let current_lanes: Vec<f64> = current_lanes.map(f64::from).collect();
        let total_lanes: Vec<f64> = total_lanes.map(f64::from).collect();
        if current_lanes.len() != self.lanes.len() || total_lanes.len() != self.lanes.len() {
            return Err(ErrorReason::InvalidField);
        }
        with_lanes(current, |index| {
            current_lanes[index] + total_lanes[index] - self.lanes[index]
        })
    }

    /// The field's value without this contribution.
    pub(in crate::world) fn withdrawn(
        &self,
        current: &AnimationValue,
    ) -> Result<AnimationValue, ErrorReason> {
        self.moved(current, &identity_value(&self.total))
    }

    /// A write moved the field from `before` to `after` to hold `total`.
    pub(in crate::world) fn landed(
        &mut self,
        before: &AnimationValue,
        after: &AnimationValue,
        total: &AnimationValue,
    ) {
        if let (false, Some(before), Some(after)) = (
            self.lanes.is_empty(),
            float_lanes(before),
            float_lanes(after),
        ) {
            for ((lane, before), after) in self.lanes.iter_mut().zip(before).zip(after) {
                *lane += f64::from(after) - f64::from(before);
            }
        }
        self.total.clone_from(total);
    }

    /// Another writer replaced the field: it holds nothing of this contribution.
    fn forget(&mut self) {
        self.total = identity_value(&self.total);
        self.lanes.fill(0.0);
    }
}

/// The f32 lanes of a float value: a float, or a float vector or matrix.
fn float_lanes(value: &AnimationValue) -> Option<impl Iterator<Item = f32> + '_> {
    let lanes: &[f32] = match value {
        AnimationValue::Field(FieldValue::F32(value)) => std::slice::from_ref(value),
        AnimationValue::Field(FieldValue::Dynamic(value)) => value.floats()?,
        _ => return None,
    };
    Some(lanes.iter().copied())
}

/// A float value of `template`'s type with each lane rounded from `lane`.
fn with_lanes(
    template: &AnimationValue,
    lane: impl Fn(usize) -> f64,
) -> Result<AnimationValue, ErrorReason> {
    use crate::DynamicValue;

    fn lanes<const N: usize>(lane: impl Fn(usize) -> f64) -> [f32; N] {
        std::array::from_fn(|index| lane(index) as f32)
    }

    Ok(match template {
        AnimationValue::Field(FieldValue::F32(_)) => {
            AnimationValue::Field(FieldValue::F32(lane(0) as f32))
        }
        AnimationValue::Field(FieldValue::Dynamic(value)) => {
            AnimationValue::Field(FieldValue::Dynamic(match value {
                DynamicValue::F32(_) => DynamicValue::F32(lane(0) as f32),
                DynamicValue::Vec2(_) => DynamicValue::Vec2(lanes(lane)),
                DynamicValue::Vec3(_) => DynamicValue::Vec3(lanes(lane)),
                DynamicValue::Vec4(_) => DynamicValue::Vec4(lanes(lane)),
                DynamicValue::Mat2(_) => DynamicValue::Mat2(lanes(lane)),
                DynamicValue::Mat3(_) => DynamicValue::Mat3(lanes(lane)),
                DynamicValue::Mat4(_) => DynamicValue::Mat4(lanes(lane)),
                _ => return Err(ErrorReason::InvalidField),
            }))
        }
        _ => return Err(ErrorReason::InvalidField),
    })
}

/// The contributions one controller currently has in its fields.
///
/// Entries are keyed by exact target identity and kept in target order. An
/// evaluation stages a total per entry; a total becomes applied only when the
/// write carrying it lands, so a rejected write is retried with the same change.
#[derive(Debug, Default)]
pub(in crate::world) struct AnimationContributions {
    applied: Vec<(AnimationTargetIdentity, AnimationApplied)>,
    totals: Vec<AnimationValue>,
    /// Each staged entry's field value before and after this evaluation's write.
    staged: Vec<Option<(AnimationValue, AnimationValue)>>,
    empty: Vec<AnimationValue>,
    used: Vec<bool>,
    /// Entry of each driver's field, parallel to the controller's drivers.
    slots: Vec<Option<usize>>,
    prepared: bool,
}

impl AnimationContributions {
    /// What this controller has added to each field, in target order.
    pub(in crate::world) fn entries(&self) -> &[(AnimationTargetIdentity, AnimationApplied)] {
        &self.applied
    }

    pub(in crate::world) fn get(
        &self,
        identity: &AnimationTargetIdentity,
    ) -> Option<&AnimationApplied> {
        self.applied
            .binary_search_by(|(key, _)| key.cmp(identity))
            .ok()
            .map(|index| &self.applied[index].1)
    }

    /// Record that `identity`'s field holds `value` of this controller.
    pub(in crate::world) fn set(
        &mut self,
        identity: &AnimationTargetIdentity,
        value: AnimationValue,
    ) {
        self.insert(identity, AnimationApplied::new(value));
    }

    fn insert(&mut self, identity: &AnimationTargetIdentity, value: AnimationApplied) -> usize {
        match self.applied.binary_search_by(|(key, _)| key.cmp(identity)) {
            Ok(index) => {
                self.applied[index].1 = value;
                index
            }
            Err(index) => {
                self.applied.insert(index, (identity.clone(), value));
                self.prepared = false;
                index
            }
        }
    }

    /// A crossfade moved `identity`'s field from `before` to `after` to hold
    /// `total`.
    pub(in crate::world) fn landed(
        &mut self,
        identity: &AnimationTargetIdentity,
        before: &AnimationValue,
        after: &AnimationValue,
        total: &AnimationValue,
    ) {
        let index = match self.applied.binary_search_by(|(key, _)| key.cmp(identity)) {
            Ok(index) => index,
            Err(_) => self.insert(identity, AnimationApplied::new(identity_value(total))),
        };
        self.applied[index].1.landed(before, after, total);
    }

    /// Take every entry, for the caller to withdraw.
    pub(in crate::world) fn take(&mut self) -> Vec<(AnimationTargetIdentity, AnimationApplied)> {
        self.prepared = false;
        std::mem::take(&mut self.applied)
    }

    /// Replace every entry, for a controller taking over another's fields.
    pub(in crate::world) fn replace(
        &mut self,
        entries: Vec<(AnimationTargetIdentity, AnimationApplied)>,
    ) {
        debug_assert!(entries.windows(2).all(|pair| pair[0].0 < pair[1].0));
        self.applied = entries;
        self.prepared = false;
    }

    /// Drop the entries of fields that departed with their property.
    pub(in crate::world) fn retain(
        &mut self,
        mut keep: impl FnMut(&AnimationTargetIdentity) -> bool,
    ) {
        let before = self.applied.len();
        self.applied.retain(|(identity, _)| keep(identity));
        if self.applied.len() != before {
            self.prepared = false;
        }
    }

    /// Drop entries that add nothing.
    pub(in crate::world) fn prune_empty(&mut self) {
        let before = self.applied.len();
        self.applied.retain(|(_, value)| !value.is_empty());
        if self.applied.len() != before {
            self.prepared = false;
        }
    }

    /// Another writer is about to overwrite `offset` of `entity`'s `component`
    /// absolutely: every contribution to it is gone and applies in full again.
    pub(in crate::world) fn forget(
        &mut self,
        entity: crate::EntityId,
        component: u16,
        offset: u32,
    ) {
        for (identity, value) in &mut self.applied {
            if identity.entity == entity
                && identity.property.property().is_some_and(|property| {
                    property.component == component && property.offsets.contains(&offset)
                })
            {
                value.forget();
            }
        }
    }

    /// The driver list changed; slots are rebuilt before the next evaluation.
    pub(in crate::world) fn invalidate(&mut self) {
        self.prepared = false;
    }

    pub(in crate::world) fn prepared(&self) -> bool {
        self.prepared
    }

    /// Give every contributing property driver an entry and a slot.
    pub(in crate::world) fn prepare(&mut self, drivers: &[Box<dyn AnimationDriverBinding>]) {
        for driver in drivers {
            if let Some(identity) = contributing_property(driver.as_ref())
                && self.get(identity).is_none()
            {
                self.set(identity, identity_value(driver.template()));
            }
        }
        self.slots.clear();
        self.used.clear();
        self.used.resize(self.applied.len(), false);
        self.empty.clear();
        self.empty.extend(
            self.applied
                .iter()
                .map(|(_, value)| identity_value(&value.total)),
        );
        for driver in drivers {
            let slot = contributing_property(driver.as_ref()).map(|identity| {
                self.applied
                    .binary_search_by(|(key, _)| key.cmp(identity))
                    .expect("entry inserted above")
            });
            if let Some(slot) = slot {
                self.used[slot] = true;
            }
            self.slots.push(slot);
        }
        self.totals.clear();
        self.totals
            .extend(self.applied.iter().map(|(_, value)| value.total.clone()));
        self.staged.clear();
        self.staged.resize(self.applied.len(), None);
        self.prepared = true;
    }

    /// Start an evaluation: fields the drivers use total nothing yet.
    pub(in crate::world) fn begin(&mut self) {
        debug_assert!(self.prepared);
        for index in 0..self.applied.len() {
            self.totals[index].clone_from(if self.used[index] {
                &self.empty[index]
            } else {
                &self.applied[index].1.total
            });
            self.staged[index] = None;
        }
    }

    /// Add driver `driver`'s contribution to its field's total.
    pub(in crate::world) fn add(
        &mut self,
        driver: usize,
        contribution: &AnimationValue,
    ) -> Result<(), ErrorReason> {
        let slot = self.slots[driver].ok_or(ErrorReason::InvalidField)?;
        self.totals[slot] = compose(&self.totals[slot], contribution)?;
        Ok(())
    }

    /// Whether entry `index`'s field must move in this evaluation.
    pub(in crate::world) fn moves(&self, index: usize) -> bool {
        self.used[index] && self.totals[index] != self.applied[index].1.total
    }

    /// Target of entry `index` and its field's value once it holds the staged
    /// total instead of what it holds now.
    pub(in crate::world) fn moved(
        &self,
        index: usize,
        current: &AnimationValue,
    ) -> Result<AnimationValue, ErrorReason> {
        self.applied[index].1.moved(current, &self.totals[index])
    }

    pub(in crate::world) fn identity(&self, index: usize) -> &AnimationTargetIdentity {
        &self.applied[index].0
    }

    /// Entry `index`'s write, from `before` to `after`, is staged.
    pub(in crate::world) fn stage(
        &mut self,
        index: usize,
        before: AnimationValue,
        after: AnimationValue,
    ) {
        self.staged[index] = Some((before, after));
    }

    /// The write of `key` landed: its staged totals are applied now.
    pub(in crate::world) fn commit(&mut self, key: (crate::EntityId, u16)) {
        for index in 0..self.applied.len() {
            let (identity, applied) = &mut self.applied[index];
            if (identity.entity, identity.property.component_target()) == key
                && let Some((before, after)) = self.staged[index].take()
            {
                applied.landed(&before, &after, &self.totals[index]);
            }
        }
    }
}

/// The target of a driver whose contribution the controller keeps. Joint
/// contributions are not kept: the Skeleton rebuilds its pose every frame.
fn contributing_property(driver: &dyn AnimationDriverBinding) -> Option<&AnimationTargetIdentity> {
    (driver.contributes() && driver.identity().property.property().is_some())
        .then(|| driver.identity())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f32(value: f32) -> AnimationValue {
        AnimationValue::Field(FieldValue::F32(value))
    }

    #[test]
    fn linear_contributions_move_by_their_change() {
        let total = delta(&f32(7.0), &f32(3.0), 0.5).unwrap();
        assert_eq!(total, f32(2.0));
        assert_eq!(reapply(&f32(10.0), &f32(0.0), &total).unwrap(), f32(12.0));
        assert_eq!(reapply(&f32(12.0), &total, &f32(0.0)).unwrap(), f32(10.0));
        assert_eq!(compose(&total, &f32(1.0)).unwrap(), f32(3.0));
        assert!(is_identity(&identity_value(&total)));
    }

    #[test]
    fn float_contributions_withdraw_exactly_after_many_rounded_writes() {
        let base = f32(1234.567);
        let mut field = base.clone();
        let mut applied = AnimationApplied::new(f32(0.0));
        let mut time = 0.0_f64;
        for step in 0..10_000 {
            time += 0.013 + f64::from(step % 7) * 0.001;
            let total = f32((10.0 * (time * 1.7).sin()) as f32);
            let next = applied.moved(&field, &total).unwrap();
            applied.landed(&field, &next, &total);
            field = next;
        }
        assert_eq!(applied.withdrawn(&field).unwrap(), base);
    }

    #[test]
    fn rotation_contributions_compose_and_withdraw() {
        let half = std::f32::consts::FRAC_1_SQRT_2;
        let quarter = AnimationValue::Rotation([0.0, 0.0, half, half]);
        let base = AnimationValue::Rotation([half, 0.0, 0.0, half]);
        let total = delta(&quarter, &AnimationValue::Rotation(IDENTITY_ROTATION), 1.0).unwrap();
        let moved = reapply(&base, &identity_value(&total), &total).unwrap();
        let back = reapply(&moved, &total, &identity_value(&total)).unwrap();
        let (AnimationValue::Rotation(back), AnimationValue::Rotation(base)) = (back, base) else {
            unreachable!();
        };
        for (back, base) in back.iter().zip(base) {
            assert!((back - base).abs() < 1.0e-6);
        }
    }

    #[test]
    fn dynamic_float_values_contribute_and_others_do_not() {
        let vector =
            AnimationValue::Field(FieldValue::Dynamic(crate::DynamicValue::Vec2([1.0, 2.0])));
        assert!(contributes(&vector));
        assert_eq!(
            reapply(&vector, &identity_value(&vector), &vector).unwrap(),
            AnimationValue::Field(FieldValue::Dynamic(crate::DynamicValue::Vec2([2.0, 4.0])))
        );
        for value in [
            AnimationValue::Field(FieldValue::Dynamic(crate::DynamicValue::U32(1))),
            AnimationValue::Field(FieldValue::Dynamic(crate::DynamicValue::Bool(true))),
            AnimationValue::Field(FieldValue::U32(1)),
            AnimationValue::Field(FieldValue::Bool(true)),
        ] {
            assert!(!contributes(&value));
        }
    }
}
