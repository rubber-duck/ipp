//! Semantic System and World identities attached to measurements.

use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

use super::allocation::{
    CATEGORIES, CONTEXT_CATEGORIES, CONTEXT_CATEGORY_COUNTS, SYSTEM_CATEGORY_COUNTS,
};
use super::capture::{ACTIVE_GUARDS, CAPTURE_HOST_FILTER, SUPPRESSED_DEPTH, capture_id, measuring};
use super::counters::GrowingCounters;
use super::stages::{
    FIXED_COUNTERS, FixedStage, SYSTEM_COUNTERS, SYSTEM_PHASES, SYSTEM_SLOTS, fixed_stage_base,
};

#[derive(Clone, Copy)]
pub(super) struct SystemProfileEntry {
    pub(super) name: &'static str,
    pub(super) composition: u64,
    pub(super) world: usize,
}

pub(super) struct SystemProfileRegistry {
    pub(super) entries: Vec<SystemProfileEntry>,
}

impl SystemProfileRegistry {
    pub(super) const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// The stable profile index of one key, appended on first registration.
    pub(super) fn register(&mut self, name: &'static str, composition: u64, world: usize) -> usize {
        if let Some(index) = self.entries.iter().position(|entry| {
            entry.name == name && entry.composition == composition && entry.world == world
        }) {
            return index;
        }

        let entry = SystemProfileEntry {
            name,
            composition,
            world,
        };
        if let Some(index) = self.entries.iter().position(|entry| entry.name.is_empty()) {
            self.entries[index] = entry;
            index
        } else {
            self.entries.push(entry);
            self.entries.len() - 1
        }
    }
}

pub(super) static SYSTEM_PROFILES: std::sync::Mutex<SystemProfileRegistry> =
    std::sync::Mutex::new(SystemProfileRegistry::new());

/// Register one semantic profile key at World construction, before frame dispatch.
///
/// Returns its stable profile index; the key's counters exist from then on.
pub fn register_system(name: &'static str, composition: u64, world: usize) -> usize {
    let index = SYSTEM_PROFILES
        .lock()
        .unwrap()
        .register(name, composition, world);
    let slots = (index + 1) * SYSTEM_PHASES;
    SYSTEM_COUNTERS.reserve(slots * 4);
    SYSTEM_CATEGORY_COUNTS.reserve(slots * 2);
    PROFILE_CONTEXTS.reserve(slots);
    for phase in 0..SYSTEM_PHASES {
        let context = register_context(ProfileContext {
            system: name,
            phase: Some(ProfilePhase::ALL[phase]),
            ..world_context(world)
        });
        CONTEXT_ROOTS
            .get(context)
            .unwrap()
            .store(world as u64, Relaxed);
        PROFILE_CONTEXTS
            .get(index * SYSTEM_PHASES + phase)
            .unwrap()
            .store(context as u64, Relaxed);
    }
    SYSTEM_SLOTS.fetch_max(system_count() * SYSTEM_PHASES, Relaxed);
    index
}

/// Registered semantic profile keys, readable through [`system_name`].
pub fn system_count() -> usize {
    SYSTEM_PROFILES.lock().unwrap().entries.len()
}

/// Stable schedule label for the System at one timed position.
pub fn system_name(index: usize) -> &'static str {
    SYSTEM_PROFILES
        .lock()
        .unwrap()
        .entries
        .get(index)
        .map_or("", |entry| entry.name)
}

/// Composition identity paired with [`system_name`] at one profile slot.
pub fn system_composition(index: usize) -> u64 {
    SYSTEM_PROFILES
        .lock()
        .unwrap()
        .entries
        .get(index)
        .map_or(0, |entry| {
            if entry.name.is_empty() {
                0
            } else {
                entry.composition
            }
        })
}

/// Stable semantic phases in runtime dispatch order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ProfilePhase {
    /// Frame preflight.
    Check,
    /// Ingress acceptance.
    Accept,
    /// Evaluation preparation.
    Prepare,
    /// Evaluation.
    Evaluate,
    /// Update completion.
    Finish,
    /// Final observations.
    Observe,
}

impl ProfilePhase {
    /// Stable phase order, independent of selected Systems.
    pub const ALL: [Self; SYSTEM_PHASES] = [
        Self::Check,
        Self::Accept,
        Self::Prepare,
        Self::Evaluate,
        Self::Finish,
        Self::Observe,
    ];

    /// Stable export label.
    pub fn name(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Accept => "accept",
            Self::Prepare => "prepare",
            Self::Evaluate => "evaluate",
            Self::Finish => "finish",
            Self::Observe => "observe",
        }
    }
}

/// Semantic identity attached to a measurement. Zero identities and an empty
/// System explicitly identify shared/unassigned work, never an arbitrary World.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProfileContext {
    /// Runtime Host identity.
    pub host: u64,
    /// Host-local World handle.
    pub world: u64,
    /// Monotonic World lifetime identity.
    pub incarnation: u64,
    /// Exact selected composition.
    pub composition: u64,
    /// Stable System identity, empty outside System dispatch.
    pub system: &'static str,
    /// Dispatch phase, absent outside System dispatch.
    pub phase: Option<ProfilePhase>,
}

#[derive(Clone, Copy)]
pub(super) struct ContextEntry {
    pub(super) context: ProfileContext,
    pub(super) live: bool,
    pub(super) occupied: bool,
}

pub(super) static CONTEXTS: std::sync::Mutex<Vec<ContextEntry>> = std::sync::Mutex::new(Vec::new());

pub(super) static CONTEXT_ROOTS: GrowingCounters = GrowingCounters::new();
pub(super) static PROFILE_CONTEXTS: GrowingCounters = GrowingCounters::new();
pub(super) static CONTEXT_HOSTS: GrowingCounters = GrowingCounters::new();
pub(super) static ACTIVE_CONTEXT: AtomicUsize = AtomicUsize::new(0);

pub(super) fn register_context(context: ProfileContext) -> usize {
    let mut entries = CONTEXTS.lock().unwrap();
    if let Some(index) = entries
        .iter()
        .position(|entry| entry.occupied && entry.context == context)
    {
        return index;
    }
    let entry = ContextEntry {
        context,
        live: true,
        occupied: true,
    };
    let index = if let Some(index) = entries.iter().position(|entry| !entry.occupied) {
        entries[index] = entry;
        index
    } else {
        entries.push(entry);
        entries.len() - 1
    };
    CONTEXT_HOSTS.reserve(index + 1);
    CONTEXT_HOSTS
        .get(index)
        .unwrap()
        .store(context.host, Relaxed);
    CONTEXT_ROOTS.reserve(index + 1);
    FIXED_COUNTERS.reserve((index + 1) * FixedStage::ALL.len() * 4);
    CONTEXT_CATEGORY_COUNTS.reserve((index + 1) * CONTEXT_CATEGORIES * 2);
    index
}

/// Resolve a live World lifetime at asynchronous measurement issuance.
/// Copy the returned context into the issued record; never resolve it at completion.
/// Host matching is required because World handles are local to each Host.
pub fn world_profile_context(host: u64, reference: crate::WorldRef) -> Option<ProfileContext> {
    CONTEXTS.lock().unwrap().iter().find_map(|entry| {
        (entry.live
            && entry.occupied
            && entry.context.host == host
            && entry.context.world == reference.id().0
            && entry.context.incarnation == reference.incarnation()
            && entry.context.system.is_empty()
            && entry.context.phase.is_none())
        .then_some(entry.context)
    })
}

/// Prepare attribution before dispatch; construction is the only allocating path.
pub(crate) fn register_world(host: u64, world: u64, incarnation: u64, composition: u64) -> usize {
    register_context(ProfileContext::default());
    let index = register_context(ProfileContext {
        host,
        world,
        incarnation,
        composition,
        ..ProfileContext::default()
    });
    CONTEXT_ROOTS
        .get(index)
        .unwrap()
        .store(index as u64, Relaxed);
    index
}

pub(super) fn context_count() -> usize {
    CONTEXTS.lock().unwrap().len()
}

pub(super) fn context_occupied(index: usize) -> bool {
    CONTEXTS
        .lock()
        .unwrap()
        .get(index)
        .is_some_and(|entry| entry.occupied)
}

pub(super) fn world_context(index: usize) -> ProfileContext {
    CONTEXTS
        .lock()
        .unwrap()
        .get(index)
        .filter(|entry| entry.occupied)
        .map_or(ProfileContext::default(), |entry| entry.context)
}

/// Identity of one System profile.
pub fn system_context(index: usize) -> ProfileContext {
    let entry = SYSTEM_PROFILES.lock().unwrap().entries.get(index).copied();
    entry
        .filter(|entry| !entry.name.is_empty())
        .map_or(ProfileContext::default(), |entry| ProfileContext {
            system: entry.name,
            ..world_context(entry.world)
        })
}

/// Identity of a flat timed stage slot, including contextual fixed scopes.
pub fn stage_context(slot: usize) -> ProfileContext {
    let base = fixed_stage_base();
    if slot < base {
        if system_name(slot / SYSTEM_PHASES).is_empty() {
            return ProfileContext::default();
        }
        let context = PROFILE_CONTEXTS
            .get(slot)
            .map_or(0, |value| value.load(Relaxed) as usize);
        world_context(context)
    } else {
        world_context((slot - base) / FixedStage::ALL.len())
    }
}

/// Identity of an exclusive allocation category.
pub fn category_context(category: usize) -> ProfileContext {
    let base = fixed_stage_base();
    if category < CATEGORIES {
        ProfileContext::default()
    } else if category - CATEGORIES < base {
        stage_context(category - CATEGORIES)
    } else {
        world_context((category - CATEGORIES - base) / CONTEXT_CATEGORIES)
    }
}

/// Retire metadata only after the World has ceased dispatching. Captured history
/// stays readable until explicit release; live prepared indices never move.
pub(crate) fn retire_world(index: usize) {
    let identity = world_context(index);
    for entry in CONTEXTS.lock().unwrap().iter_mut() {
        if entry.context.host == identity.host && entry.context.incarnation == identity.incarnation
        {
            entry.live = false;
        }
    }
}

/// Single-thread Host attribution guard. It restores the enclosing semantic
/// context and uses no allocation or registry lookup in phase dispatch.
pub struct ContextScope {
    previous: usize,
    capture: u64,
    enabled: bool,
    suppressed: bool,
}

impl ContextScope {
    pub(crate) fn world(world: usize) -> Self {
        let active = ACTIVE_CONTEXT.load(Relaxed);
        let root = CONTEXT_ROOTS
            .get(active)
            .map_or(0, |value| value.load(Relaxed) as usize);
        Self::new(if root == world {
            active
        } else {
            world
        })
    }

    pub(crate) fn new(context: usize) -> Self {
        let active = measuring();
        let owner = CAPTURE_HOST_FILTER.load(Relaxed);
        let host = CONTEXT_HOSTS
            .get(context)
            .map_or(0, |value| value.load(Relaxed));
        let suppressed = active && owner != 0 && host != 0 && host != owner;
        if suppressed {
            SUPPRESSED_DEPTH.with(|depth| depth.set(depth.get() + 1));
        }
        let enabled = active && !suppressed;
        if enabled || suppressed {
            ACTIVE_GUARDS.fetch_add(1, Relaxed);
        }
        Self {
            previous: if enabled {
                ACTIVE_CONTEXT.swap(context, Relaxed)
            } else {
                0
            },
            capture: capture_id(),
            enabled,
            suppressed,
        }
    }
}

impl Drop for ContextScope {
    fn drop(&mut self) {
        if self.enabled || self.suppressed {
            ACTIVE_GUARDS.fetch_sub(1, Relaxed);
        }
        if self.suppressed && self.capture == capture_id() {
            SUPPRESSED_DEPTH.with(|depth| depth.set(depth.get() - 1));
        }
        if self.enabled && self.capture == capture_id() {
            ACTIVE_CONTEXT.store(self.previous, Relaxed);
        }
    }
}
