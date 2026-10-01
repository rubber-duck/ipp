use super::output::{LifecycleMemberStatus, LifecycleOutputState};
use super::value_observation::LifecycleValueMember;
use super::*;
use crate::world::systems::SystemWorldView;
use std::collections::HashMap;
use std::rc::Rc;

/// World-owned sparse indexes; removal retires entries without retaining tombstones.
#[derive(Default)]
pub(super) struct LifecycleTargetIndex {
    /// Entity and component targets, selected by each applied transition.
    by_target: HashMap<LifecycleWatchTarget, BTreeMap<LifecycleWatchId, LifecycleWatchMember>>,
    /// Value members, each compared with the stored values at every frame end.
    values: BTreeMap<LifecycleWatchId, LifecycleValueMember>,
    by_session: BTreeMap<u64, BTreeMap<LifecycleWatchId, LifecycleWatchMember>>,
    #[cfg(any(test, feature = "diagnostics"))]
    pub work: LifecycleTargetWork,
}

impl LifecycleTargetIndex {
    pub fn add(&mut self, session: u64, member: LifecycleWatchMember) {
        member.0.status.set(LifecycleMemberStatus::Active);

        if let LifecycleWatchTarget::Value(..) = member.0.target {
            self.values
                .insert(member.id(), LifecycleValueMember::new(member.clone()));
        } else {
            self.by_target
                .entry(member.target())
                .or_default()
                .insert(member.id(), member.clone());
        }
        self.by_session
            .entry(session)
            .or_default()
            .insert(member.id(), member);
    }

    pub fn remove(&mut self, session: u64, member: &LifecycleWatchMember) {
        member.0.status.set(LifecycleMemberStatus::Removed);

        if let LifecycleWatchTarget::Value(..) = member.0.target {
            self.values.remove(&member.id());
        } else if let Some(members) = self.by_target.get_mut(&member.0.target) {
            members.remove(&member.id());
            if members.is_empty() {
                self.by_target.remove(&member.0.target);
                if self.by_target.is_empty() {
                    self.by_target = HashMap::new();
                }
            }
        }

        if let Some(members) = self.by_session.get_mut(&session) {
            members.remove(&member.id());
            if members.is_empty() {
                self.by_session.remove(&session);
            }
        }

        if let Some(output) = member.0.output.upgrade() {
            output.remove(member.id());
        }
    }

    pub fn matching<'a>(
        &'a mut self,
        observation: &'a LifecycleObservation,
    ) -> impl Iterator<Item = &'a LifecycleWatchMember> {
        let target = LifecycleWatchTarget::observation(observation);
        let members = target
            .as_ref()
            .and_then(|target| self.by_target.get(target));
        #[cfg(any(test, feature = "diagnostics"))]
        {
            self.work.record(
                usize::from(target.is_some()),
                members.map_or(0, BTreeMap::len),
            );
        }

        members
            .into_iter()
            .flat_map(BTreeMap::values)
            .filter(|member| member.0.kinds.matches(observation) && member.is_active())
    }

    /// Compare every value member with the final stored values of `tick`, in member order.
    /// The cost is one visit per value member; unchanged members allocate nothing.
    pub fn observe_values(&mut self, world: SystemWorldView<'_>, tick: u64) {
        for value in self.values.values_mut() {
            value.observe(world, tick);
        }
    }

    pub fn release_session(&mut self, session: u64) -> Vec<Rc<LifecycleOutputState>> {
        let Some(members) = self.by_session.remove(&session) else {
            return Vec::new();
        };

        let mut outputs = BTreeMap::new();
        for member in members.into_values() {
            if let Some(output) = member.0.output.upgrade() {
                outputs.insert(member.id().output, output);
            }
            self.remove(session, &member);
        }

        outputs.into_values().collect()
    }

    pub fn has_session(&self, session: u64) -> bool {
        self.by_session.contains_key(&session)
    }

    #[cfg(test)]
    pub fn counts(&self) -> (usize, usize, usize) {
        (
            self.by_target.len(),
            self.by_session.len(),
            self.by_target.values().map(BTreeMap::len).sum(),
        )
    }
}

impl Drop for LifecycleTargetIndex {
    fn drop(&mut self) {
        while let Some((&session, _)) = self.by_session.first_key_value() {
            for output in self.release_session(session) {
                output.retire();
            }
        }
    }
}
