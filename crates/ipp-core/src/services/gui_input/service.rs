use super::delivery::target_key;
use super::proof::GuiRoutedInputProof;
use super::ticket::{GuiCommandTicket, GuiPending};
use super::{
    GuiDeliveryPermit, GuiDeliveryTerminal, GuiInputCommand, GuiInputError,
    GuiInputReservationError, GuiRequestKey,
};
use crate::systems::gui::local::GuiEntityTarget;
use crate::{HostRuntime, RootOutputBinding, WorldAttachmentToken, WorldRef};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicU64, Ordering};

/// Opaque service-fenced session lifetime; never a caller-provided wire number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct GuiInputSessionId {
    service: u64,
    serial: u64,
}

impl GuiInputSessionId {
    /// Read-only identity for adapter correlation and diagnostics.
    pub fn identity(self) -> (u64, u64) {
        (self.service, self.serial)
    }
}

pub(super) struct GuiSessionState {
    pub id: GuiInputSessionId,
    pub live: Cell<bool>,
}

/// A delivery lifetime that may originate actions in multiple target Worlds.
#[derive(Clone)]
pub struct GuiInputSession(pub(super) Rc<GuiSessionState>);

impl GuiInputSession {
    /// Exact service-minted lifetime.
    pub fn id(&self) -> GuiInputSessionId {
        self.0.id
    }

    /// Also available to an adapter retaining this revocation handle.
    pub fn is_live(&self) -> bool {
        self.0.live.get()
    }
}

pub(super) struct GuiContextState {
    pub serial: u64,
    pub session: Rc<GuiSessionState>,
    pub root: RootOutputBinding,
    pub live: Cell<bool>,
    pub native: RefCell<GuiNativeState>,
}

#[derive(Default)]
pub(super) struct GuiNativeState {
    pub focus: Option<GuiEntityTarget>,
    pub captures: BTreeMap<u64, GuiEntityTarget>,
}

/// Opaque input ownership generation. Equal output/viewport rebinds still replace it.
#[derive(Clone)]
pub struct GuiInputContext(pub(super) Rc<GuiContextState>);

impl GuiInputContext {
    /// Bound root authority, not proof that it remains current.
    pub fn root(&self) -> RootOutputBinding {
        self.0.root
    }

    /// Session and service-local context generation.
    pub fn identity(&self) -> (GuiInputSessionId, u64) {
        (self.0.session.id, self.0.serial)
    }
}

/// Synchronous platform cancellation. Logical control focus remains GUI-owned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuiNativeCancellation {
    /// Revoked native focus target, if any.
    pub focus: Option<GuiEntityTarget>,
    /// Pointer captures to release, in pointer identity order.
    pub captures: Vec<(u64, GuiEntityTarget)>,
}

/// Result of explicitly replacing root input ownership.
pub struct GuiContextBinding {
    /// Fresh generation, even when the requested binding is unchanged.
    pub context: GuiInputContext,
    /// Old native state to cancel synchronously, not an outcome queue.
    pub cancelled: Vec<GuiNativeCancellation>,
}

/// Bounded service metadata, independent of adapter response capacity.
///
/// Each bound refuses the next lease, session, context, payload or capture with
/// `GuiInputError::Capacity` and leaves existing ones intact. The defaults give one Host
/// room for 64 sessions and contexts (one per connection's input surface at the WASM
/// connection cap), 1024 pointer leases and pending commands (sixteen per context), and
/// 8 MiB of retained text payloads; captures per context use [`super::GUI_INPUT_MAX_POINTERS`].
#[derive(Clone, Copy, Debug)]
pub struct GuiInputLimits {
    /// Candidate, active and cancelled-but-retained pointer identities.
    pub pointer_leases: usize,
    /// Actual owned action payload capacity retained until command Drop.
    pub payload_bytes: usize,
    /// Concurrent session lifetimes.
    pub sessions: usize,
    /// Concurrent root contexts.
    pub contexts: usize,
    /// Retained command payloads, including cancelled payloads awaiting ordinary queue Drop.
    pub pending: usize,
    /// Total path tokens in commands, including after cancellation.
    pub path_nodes: usize,
    /// Pointer captures per context.
    pub captures: usize,
}

impl Default for GuiInputLimits {
    fn default() -> Self {
        Self {
            pointer_leases: 1024,
            payload_bytes: 8 * 1024 * 1024,
            sessions: 64,
            contexts: 64,
            pending: 1024,
            path_nodes: 8192,
            captures: super::GUI_INPUT_MAX_POINTERS,
        }
    }
}

pub(super) struct GuiRegistry {
    pub pointer_leases: usize,
    pub pointer_slots:
        BTreeMap<super::pointer::GuiPointerKey, Weak<super::pointer::GuiPointerSlot>>,
    pub limits: GuiInputLimits,
    pub payload_bytes: usize,
    pub pending: BTreeMap<GuiRequestKey, Rc<GuiPending>>,
    pub retained: usize,
    pub path_nodes: usize,
}

/// Host-owned composition state; methods borrow the Host only for their duration.
/// This foundation reserves authority/delivery but never queues unsupported GUI commands.
pub struct GuiInputService {
    identity: u64,
    serial: Cell<u64>,
    limits: GuiInputLimits,
    sessions: RefCell<BTreeMap<GuiInputSessionId, Rc<GuiSessionState>>>,
    contexts: RefCell<BTreeMap<WorldRef, Rc<GuiContextState>>>,
    pub(super) registry: Rc<RefCell<GuiRegistry>>,
}

impl Default for GuiInputService {
    fn default() -> Self {
        Self::new(GuiInputLimits::default())
    }
}

impl GuiInputService {
    /// Construct a transport-independent service with explicit metadata budgets.
    pub fn new(limits: GuiInputLimits) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let identity = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .expect("GUI service identity exhausted");
        Self {
            identity,
            serial: Cell::new(0),
            limits,
            sessions: RefCell::new(BTreeMap::new()),
            contexts: RefCell::new(BTreeMap::new()),
            registry: Rc::new(RefCell::new(GuiRegistry {
                pointer_leases: 0,
                pointer_slots: BTreeMap::new(),
                limits,
                payload_bytes: 0,
                pending: BTreeMap::new(),
                retained: 0,
                path_nodes: 0,
            })),
        }
    }

    fn next_serial(&self) -> Result<u64, GuiInputError> {
        let serial = self
            .serial
            .get()
            .checked_add(1)
            .ok_or(GuiInputError::Capacity)?;
        self.serial.set(serial);
        Ok(serial)
    }

    /// Reserve inert pointer identity for a live routed ticket, without stealing its pointer.
    /// Keep this lease across publications while context, target and path stay unchanged.
    /// Failure rejects the existing ticket without changing any interaction authority.
    pub fn pointer_lease(
        &self,
        input: &GuiInputCommand,
        pointer: u64,
    ) -> Result<super::GuiPointerLease, GuiInputError> {
        let result = (|| {
            input.pointer_reservation_ready()?;
            let proof = input.routed_proof();
            self.context(&proof.context)?;
            super::GuiPointerLease::reserve(input, pointer, self.next_serial()?, &self.registry)
        })();
        if let Err(error) = &result {
            input.reject(*error);
        }
        result
    }

    /// Exact-generation safety revocation for the composed runtime owner.
    /// This neither delivers an action nor changes platform capture bookkeeping.
    pub fn cancel_pointer_lease(
        &self,
        lease: &super::GuiPointerLease,
    ) -> Result<(), GuiInputError> {
        if lease.session().service != self.identity {
            return Err(GuiInputError::SessionClosed);
        }
        lease.revoke();
        Ok(())
    }

    fn session(&self, session: &GuiInputSession) -> Result<(), GuiInputError> {
        if session.is_live() && session.id().service == self.identity {
            Ok(())
        } else {
            Err(GuiInputError::SessionClosed)
        }
    }

    /// Allocate a fresh internal lifetime, including after equal wire-session reuse.
    pub fn open_session(&self) -> Result<GuiInputSession, GuiInputError> {
        if self.sessions.borrow().len() >= self.limits.sessions {
            return Err(GuiInputError::Capacity);
        }
        let state = Rc::new(GuiSessionState {
            id: GuiInputSessionId {
                service: self.identity,
                serial: self.next_serial()?,
            },
            live: Cell::new(true),
        });
        self.sessions.borrow_mut().insert(state.id, state.clone());
        Ok(GuiInputSession(state))
    }

    /// Revoke delivery before cancelling tickets in every known target World.
    /// No callback or payload destructor runs while service maps are borrowed.
    pub fn close_session(&self, session: &GuiInputSession) -> Vec<GuiNativeCancellation> {
        if session.id().service != self.identity {
            return Vec::new();
        }
        session.0.live.set(false);
        self.sessions.borrow_mut().remove(&session.id());
        let contexts: Vec<_> = self
            .contexts
            .borrow()
            .values()
            .filter(|context| context.session.id == session.id())
            .cloned()
            .collect();
        let mut cancellations = Vec::new();
        for context in contexts {
            cancellations.extend(self.release_context(&GuiInputContext(context)));
        }
        let pending: Vec<_> = self
            .registry
            .borrow()
            .pending
            .values()
            .filter(|pending| pending.key.session == session.id())
            .cloned()
            .collect();
        for pending in pending {
            pending.finish(GuiDeliveryTerminal::Cancelled);
        }
        cancellations
    }

    /// Bind input ownership to the Host's current explicit root, replacing any old owner.
    pub fn bind_context(
        &self,
        host: &HostRuntime,
        session: &GuiInputSession,
        root: WorldRef,
    ) -> Result<GuiContextBinding, GuiInputError> {
        self.session(session)?;
        let binding = host
            .root_output_binding(root)
            .ok()
            .flatten()
            .ok_or(GuiInputError::StaleContext)?;
        if host.root_output(root.id()).is_none() {
            return Err(GuiInputError::StaleContext);
        }
        if !self.contexts.borrow().contains_key(&root)
            && self.contexts.borrow().len() >= self.limits.contexts
        {
            return Err(GuiInputError::Capacity);
        }
        let context = Rc::new(GuiContextState {
            serial: self.next_serial()?,
            session: session.0.clone(),
            root: binding,
            live: Cell::new(true),
            native: RefCell::new(GuiNativeState::default()),
        });
        let old = self.contexts.borrow_mut().insert(root, context.clone());
        let cancelled = old.map_or_else(Vec::new, |old| self.cancel_context(old));
        Ok(GuiContextBinding {
            context: GuiInputContext(context),
            cancelled,
        })
    }

    fn cancel_context(&self, context: Rc<GuiContextState>) -> Vec<GuiNativeCancellation> {
        context.live.set(false);
        let native = std::mem::take(&mut *context.native.borrow_mut());
        let pending: Vec<_> = self
            .registry
            .borrow()
            .pending
            .values()
            .filter(|pending| {
                pending
                    .context
                    .as_ref()
                    .is_some_and(|candidate| Rc::ptr_eq(candidate, &context))
            })
            .cloned()
            .collect();
        for pending in pending {
            pending.finish(GuiDeliveryTerminal::Cancelled);
        }
        vec![GuiNativeCancellation {
            focus: native.focus,
            captures: native.captures.into_iter().collect(),
        }]
    }

    /// Release native ownership and cancel all queued work carrying this generation.
    /// Adapters call this before physical selection, context-loss or viewport changes,
    /// even when the Host root binding is unchanged. Rebinding never revives this handle.
    pub fn release_context(&self, context: &GuiInputContext) -> Vec<GuiNativeCancellation> {
        if context.0.session.id.service != self.identity || !context.0.live.get() {
            return Vec::new();
        }
        {
            let mut contexts = self.contexts.borrow_mut();
            if contexts
                .get(&context.0.root.output.world())
                .is_some_and(|current| Rc::ptr_eq(current, &context.0))
            {
                contexts.remove(&context.0.root.output.world());
            }
        }
        self.cancel_context(context.0.clone())
    }

    /// Record platform ownership only; no local logical focus or value is changed.
    pub fn set_native_focus(
        &self,
        context: &GuiInputContext,
        target: Option<GuiEntityTarget>,
    ) -> Result<(), GuiInputError> {
        self.context(context)?;
        context.0.native.borrow_mut().focus = target;
        Ok(())
    }

    /// Record a bounded platform capture. The router owns hit/gesture decisions.
    pub fn set_pointer_capture(
        &self,
        context: &GuiInputContext,
        pointer: u64,
        target: Option<GuiEntityTarget>,
    ) -> Result<(), GuiInputError> {
        self.context(context)?;
        let mut native = context.0.native.borrow_mut();
        if let Some(target) = target {
            if !native.captures.contains_key(&pointer)
                && native.captures.len() >= self.limits.captures
            {
                return Err(GuiInputError::Capacity);
            }
            native.captures.insert(pointer, target);
        } else {
            native.captures.remove(&pointer);
        }
        Ok(())
    }

    pub(super) fn context(&self, context: &GuiInputContext) -> Result<(), GuiInputError> {
        if context.0.live.get()
            && context.0.session.live.get()
            && context.0.session.id.service == self.identity
        {
            Ok(())
        } else {
            Err(GuiInputError::StaleContext)
        }
    }

    /// Reserve a Host-selected path from its current completed presentation.
    /// This is not pointer hit testing or a client-supplied Routed provenance field.
    #[allow(clippy::too_many_arguments)]
    pub fn reserve_routed(
        &self,
        host: &HostRuntime,
        context: &GuiInputContext,
        target: GuiEntityTarget,
        request_id: u64,
        path: &[WorldAttachmentToken],
        permit: Box<dyn GuiDeliveryPermit>,
    ) -> Result<GuiInputCommand, GuiInputReservationError> {
        self.reserve_routed_source(host, context, target, request_id, path, None, permit)
    }

    /// Reserve an exact completed source selected by the physical presentation owner.
    /// A retained source never bypasses current context, path or target validation.
    #[allow(clippy::too_many_arguments)]
    pub fn reserve_routed_source(
        &self,
        host: &HostRuntime,
        context: &GuiInputContext,
        target: GuiEntityTarget,
        request_id: u64,
        path: &[WorldAttachmentToken],
        source: Option<crate::WorldPublicationId>,
        permit: Box<dyn GuiDeliveryPermit>,
    ) -> Result<GuiInputCommand, GuiInputReservationError> {
        let mut permit = Some(permit);
        let result = (|| {
            self.context(context)?;
            if path.len()
                > self
                    .limits
                    .path_nodes
                    .saturating_sub(self.registry.borrow().path_nodes)
            {
                return Err(GuiInputError::Capacity);
            }
            let proof = GuiRoutedInputProof::capture(host, context.clone(), target, path, source)?;
            self.reserve(
                host,
                &GuiInputSession(context.0.session.clone()),
                target,
                request_id,
                proof,
                &mut permit,
            )
        })();
        result.map_err(|reason| GuiInputReservationError {
            reason,
            permit: permit.take().expect("unaccepted GUI permit"),
        })
    }

    fn reserve(
        &self,
        host: &HostRuntime,
        session: &GuiInputSession,
        target: GuiEntityTarget,
        request_id: u64,
        proof: GuiRoutedInputProof,
        permit: &mut Option<Box<dyn GuiDeliveryPermit>>,
    ) -> Result<GuiInputCommand, GuiInputError> {
        self.session(session)?;
        if host.world_ref(target.world.id()) != Some(target.world)
            || !host
                .world_manifest(target.world.id())
                .is_some_and(|manifest| {
                    manifest
                        .systems()
                        .contains(&crate::systems::gui::GuiSystem::ID)
                })
        {
            return Err(GuiInputError::Unavailable);
        }
        let key = target_key(target, session.id(), request_id);
        let nodes = proof.path_nodes();
        let pending = {
            let mut registry = self.registry.borrow_mut();
            if registry.pending.contains_key(&key) {
                return Err(GuiInputError::DuplicateRequest);
            }
            if registry.retained >= self.limits.pending
                || nodes > self.limits.path_nodes.saturating_sub(registry.path_nodes)
            {
                return Err(GuiInputError::Capacity);
            }
            let pending = GuiPending::new(
                key,
                session.0.clone(),
                Some(proof.context.0.clone()),
                permit.take().expect("reserved GUI permit"),
                Rc::downgrade(&self.registry),
                nodes,
            );
            registry.path_nodes += nodes;
            registry.retained += 1;
            registry.pending.insert(key, pending.clone());
            pending
        };
        Ok(GuiInputCommand::new(
            GuiCommandTicket::new(pending),
            target,
            proof,
        ))
    }

    /// Count only unsettled metadata; terminal observations are never retained here.
    pub fn pending_count(&self) -> usize {
        self.registry.borrow().pending.len()
    }
}

impl Drop for GuiInputService {
    fn drop(&mut self) {
        let sessions: Vec<_> = self.sessions.borrow().values().cloned().collect();
        for session in sessions {
            self.close_session(&GuiInputSession(session));
        }
    }
}
