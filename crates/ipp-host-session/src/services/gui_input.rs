//! Physical input plugin. Its queue is the bounded connection ingress transferred
//! to the Host's existing post-frame routing boundary, not a terminal/effect queue.

use crate::QueuedResponse;
use crate::reliable_output::outbox::{ReplySlot, SessionOutbox};
use crate::reliable_output::{ReplyReservation, SharedReplyBudget, SharedReplyReservation};
use ipp_core::services::gui_input::router::{
    GuiInputRouter, GuiRoutingContext, GuiRoutingDelivery, GuiRoutingDisposition,
};
use ipp_core::services::gui_input::{
    GuiDeliveryError, GuiDeliveryPermit, GuiDeliveryTerminal, GuiInputError,
};
use ipp_core::services::reliable_output::OutputClass;
use ipp_core::systems::gui::local::{GuiLocalEffect, GuiScrollChain};
use ipp_core::{HostRuntime, ViewQueryTarget};
use ipp_protocol::host::gui_input::{GuiPhysicalRequest, GuiPhysicalResponse};
use ipp_protocol::host::presentation::{PresentationSurface, PresentationView};
use ipp_protocol::host::{HostResponse, HostResponseBody};
use std::{cell::RefCell, collections::LinkedList, rc::Rc};

#[cfg(test)]
#[path = "gui_input_delivery_tests.rs"]
mod delivery_tests;

struct Owner {
    native_presentation: Option<(ipp_core::systems::gui::local::GuiTextFence, bool)>,
    connection: u64,
    id: u64,
    view: PresentationView,
    context: GuiRoutingContext,
    revocation: ReplySlot,
    reservation: SharedReplyReservation,
    outbox: SessionOutbox,
    budget: SharedReplyBudget,
}

struct Pending {
    connection: u64,
    context: u64,
    input: ipp_core::services::gui_input::router::GuiPhysicalInput,
    reply: Rc<RefCell<Reply>>,
}

struct Reply {
    native: Option<Vec<u8>>,
    connection: u64,
    request: u64,
    slot: Option<ReplySlot>,
    reservation: SharedReplyReservation,
    building: bool,
    pending: usize,
    disposition: u8,
    applied: u32,
    rejected: u32,
    cancelled: u32,
    error: Option<String>,
    scroll: Option<GuiScrollChain>,
}

fn finish(reply: &Rc<RefCell<Reply>>) {
    let completion = {
        let mut reply = reply.borrow_mut();
        if reply.building || reply.pending != 0 {
            return;
        }
        let Some(slot) = reply.slot.take() else {
            return;
        };
        if !slot.is_live() {
            return;
        }
        let response = HostResponse {
            connection: reply.connection,
            request_id: reply.request,
            body: HostResponseBody::GuiInput(GuiPhysicalResponse::Routed {
                disposition: reply.disposition,
                applied: reply.applied,
                rejected: reply.rejected,
                cancelled: reply.cancelled,
                error: reply.error.take(),
                remaining: reply
                    .scroll
                    .as_ref()
                    .and_then(|scroll| scroll.remaining().ok()),
                native: reply.native.take(),
            }),
        };
        let bytes = ipp_protocol::host::encode_host_response(&response)
            .expect("reserved bounded physical terminal");
        drop(response);
        reply.reservation.borrow_mut().shrink(bytes.capacity());
        (
            slot,
            QueuedResponse {
                bytes,
                reservation: reply.reservation.clone(),
            },
        )
    };
    completion.0.settle(completion.1);
}

struct Child {
    native: Option<Vec<u8>>,
    reply: Rc<RefCell<Reply>>,
    settled: bool,
}

impl GuiDeliveryPermit for Child {
    fn prepare_native(
        &mut self,
        state: &ipp_core::systems::gui::local::GuiNativeTextState,
    ) -> Result<(), GuiDeliveryError> {
        self.prepare(None)?;
        let size = ipp_protocol::host::gui_input::native_state_size(state)
            .map_err(|_| GuiDeliveryError::Capacity)?;
        self.reply
            .borrow()
            .reservation
            .borrow_mut()
            .reserve_bytes(512 + 2 * size)
            .map_err(|_| GuiDeliveryError::Capacity)?;
        self.native = Some(
            ipp_protocol::host::gui_input::encode_native_state(state)
                .map_err(|_| GuiDeliveryError::Capacity)?,
        );
        Ok(())
    }

    fn prepare(&mut self, _: Option<&GuiLocalEffect>) -> Result<(), GuiDeliveryError> {
        let reply = self.reply.borrow();
        if !reply.slot.as_ref().is_some_and(ReplySlot::is_live) {
            return Err(GuiDeliveryError::SessionClosed);
        }
        reply
            .reservation
            .borrow_mut()
            .reserve_bytes(512)
            .map_err(|_| GuiDeliveryError::Capacity)
    }

    fn settle(mut self: Box<Self>, terminal: GuiDeliveryTerminal) {
        self.settled = true;
        {
            let mut reply = self.reply.borrow_mut();
            match terminal {
                GuiDeliveryTerminal::NativeApplied(_) => {
                    reply.applied += 1;
                    reply.native = self.native.take();
                }
                GuiDeliveryTerminal::Applied(_)
                | GuiDeliveryTerminal::Written {
                    ..
                } => reply.applied += 1,
                GuiDeliveryTerminal::Rejected(reason) => {
                    reply.rejected += 1;
                    reply.error.get_or_insert_with(|| reason.to_string());
                }
                GuiDeliveryTerminal::Cancelled => reply.cancelled += 1,
            }
        }
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        {
            let mut reply = self.reply.borrow_mut();
            if !self.settled {
                reply.cancelled += 1;
            }
            reply.pending -= 1;
        }
        finish(&self.reply);
    }
}

struct Delivery(Rc<RefCell<Reply>>);

impl GuiRoutingDelivery for Delivery {
    fn command(&mut self) -> Result<Box<dyn GuiDeliveryPermit>, GuiInputError> {
        {
            let mut reply = self.0.borrow_mut();
            let retained = std::mem::size_of::<Reply>()
                + std::mem::size_of::<Pending>()
                + (reply.pending + 1)
                    * (std::mem::size_of::<Child>() + 4 * std::mem::size_of::<usize>());
            reply
                .reservation
                .borrow_mut()
                .reserve_retained(retained)
                .map_err(|_| GuiInputError::Capacity)?;
            reply.pending += 1;
        }
        Ok(Box::new(Child {
            native: None,
            reply: self.0.clone(),
            settled: false,
        }))
    }
}

/// Shared by native and WASM HostServices; one physical surface has one input owner.
#[derive(Default)]
pub struct GuiHostInputService {
    router: GuiInputRouter,
    owner: Option<Owner>,
    selected: Option<PresentationView>,
    next: u64,
    pending: LinkedList<Pending>,
}

impl GuiHostInputService {
    fn release_owner(&mut self, host: &mut HostRuntime) {
        let Some(owner) = self.owner.take() else {
            return;
        };
        self.router.release(host, owner.context);
        if !owner.revocation.is_live() {
            return;
        }
        let bytes = ipp_protocol::host::encode_host_response(&HostResponse {
            connection: owner.connection,
            request_id: 0,
            body: HostResponseBody::GuiInput(GuiPhysicalResponse::Revoked(owner.id)),
        })
        .expect("bounded physical revocation");
        owner.reservation.borrow_mut().encoded(bytes.capacity());
        owner.revocation.settle(QueuedResponse {
            bytes,
            reservation: owner.reservation,
        });
    }

    /// Selection changes revoke input, never authoring sessions or independent Worlds.
    pub fn selection(&mut self, host: &mut HostRuntime, selected: Option<PresentationView>) {
        if self.selected != selected {
            self.selected = selected;
            self.release_owner(host);
        }
    }

    /// Cancel this endpoint's pending ingress and exact physical owner.
    pub fn close_connection(&mut self, host: &mut HostRuntime, connection: u64) {
        if self
            .owner
            .as_ref()
            .is_some_and(|owner| owner.connection == connection)
        {
            self.release_owner(host);
        }
        let mut retained = LinkedList::new();
        while let Some(pending) = self.pending.pop_front() {
            if pending.connection != connection {
                retained.push_back(pending);
            }
        }
        self.pending = retained;
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn request(
        &mut self,
        host: &mut HostRuntime,
        connection: u64,
        request: u64,
        input: GuiPhysicalRequest,
        outbox: &SessionOutbox,
        budget: SharedReplyBudget,
        reservation: SharedReplyReservation,
    ) -> Result<Option<GuiPhysicalResponse>, String> {
        match input {
            GuiPhysicalRequest::Text {
                context,
                target,
                generation,
                edit,
            } => {
                let target = match target.resolve(host) {
                    Ok(target) => target,
                    Err(_) => return Ok(Some(GuiPhysicalResponse::Rejected("StaleTarget".into()))),
                };
                self.request(
                    host,
                    connection,
                    request,
                    GuiPhysicalRequest::Event {
                        context,
                        input: ipp_core::services::gui_input::router::GuiPhysicalInput::Text {
                            fence: ipp_core::systems::gui::local::GuiTextFence {
                                target,
                                generation,
                            },
                            edit,
                        },
                    },
                    outbox,
                    budget,
                    reservation,
                )
            }
            GuiPhysicalRequest::Open {
                view,
                blockers,
            } => {
                if self.selected != Some(view) {
                    return Ok(Some(GuiPhysicalResponse::Rejected("StaleContext".into())));
                }
                let world = view
                    .binding
                    .output
                    .world
                    .resolve(host)
                    .map_err(|error| error.to_string())?;
                if host
                    .root_output_binding(world)
                    .ok()
                    .flatten()
                    .map(ipp_protocol::host::presentation::RootBinding::from)
                    != Some(view.binding)
                {
                    return Ok(Some(GuiPhysicalResponse::Rejected("StaleContext".into())));
                }
                let next = self
                    .next
                    .checked_add(1)
                    .ok_or("input context identity exhausted")?;
                let revocation = outbox.reserve().map_err(|error| error.to_string())?;
                let mut credit = ReplyReservation::new(budget.clone(), OutputClass::Ordinary, 256)
                    .map_err(|error| error.to_string())?;
                credit
                    .reserve_retained(
                        std::mem::size_of::<Owner>()
                            + blockers.capacity()
                                * std::mem::size_of::<
                                    ipp_protocol::host::gui_input::GuiPickingBlocker,
                                >()
                            + blockers.len()
                                * std::mem::size_of::<
                                    ipp_core::services::gui_input::query::GuiPickingBlocker,
                                >(),
                    )
                    .map_err(|error| error.to_string())?;
                let blockers = blockers
                    .into_iter()
                    .map(|blocker| {
                        Ok(ipp_core::services::gui_input::query::GuiPickingBlocker {
                            world: blocker
                                .world
                                .resolve(host)
                                .map_err(|error| error.to_string())?,
                            entity: blocker.entity,
                            incarnation: blocker.incarnation,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                let credit = Rc::new(RefCell::new(credit));
                let queue_owner = crate::session::ingress::next_ingress_id()?;
                if queue_owner >= (1 << 63) {
                    return Err("Host ingress identity exhausted".into());
                }
                self.release_owner(host);
                let (context, _) = self
                    .router
                    .bind(host, world, (1 << 63) | queue_owner, blockers)
                    .map_err(|error| error.to_string())?;
                self.next = next;
                self.owner = Some(Owner {
                    native_presentation: None,
                    connection,
                    id: next,
                    view,
                    context,
                    revocation,
                    reservation: credit,
                    outbox: outbox.clone(),
                    budget,
                });
                Ok(Some(GuiPhysicalResponse::Opened(next)))
            }
            GuiPhysicalRequest::Close(id) => {
                if self
                    .owner
                    .as_ref()
                    .is_some_and(|owner| owner.connection == connection && owner.id == id)
                {
                    self.release_owner(host);
                }
                Ok(Some(GuiPhysicalResponse::Closed))
            }
            GuiPhysicalRequest::Event {
                context,
                input,
            } => {
                let admission = (|| {
                    let owner = self
                        .owner
                        .as_ref()
                        .filter(|owner| {
                            owner.connection == connection
                                && owner.id == context
                                && Some(owner.view) == self.selected
                        })
                        .ok_or(GuiInputError::StaleContext)?;
                    let output = owner
                        .view
                        .binding
                        .output
                        .resolve(host)
                        .map_err(|_| GuiInputError::StaleContext)?;
                    let view = host
                        .resolve_view(ViewQueryTarget::RootView {
                            output,
                            expected_viewport: owner.view.binding.viewport,
                        })
                        .map_err(|_| GuiInputError::Unavailable)?;
                    self.router.admission(host, &owner.context, view, &input)
                })();
                if let Err(error) = admission {
                    self.synchronize(host);
                    return Ok(Some(GuiPhysicalResponse::Rejected(error.to_string())));
                }
                reservation
                    .borrow_mut()
                    .reserve_bytes(512)
                    .map_err(|error| error.to_string())?;
                reservation
                    .borrow_mut()
                    .reserve_retained(
                        std::mem::size_of::<Pending>()
                            + std::mem::size_of::<Reply>()
                            + match &input {
                                ipp_core::services::gui_input::router::GuiPhysicalInput::Text {
                                    edit,
                                    ..
                                } => edit.retained_bytes(),
                                _ => 0,
                            }
                            + 8 * std::mem::size_of::<usize>(),
                    )
                    .map_err(|error| error.to_string())?;
                let slot = outbox.reserve().map_err(|error| error.to_string())?;
                let reply = Rc::new(RefCell::new(Reply {
                    connection,
                    request,
                    slot: Some(slot),
                    reservation,
                    building: true,
                    pending: 0,
                    disposition: 3,
                    applied: 0,
                    rejected: 0,
                    cancelled: 0,
                    error: None,
                    scroll: None,
                    native: None,
                }));
                self.pending.push_back(Pending {
                    connection,
                    context,
                    input,
                    reply,
                });
                Ok(None)
            }
        }
    }

    /// Called once at the existing HostServices routing boundary, without stepping Worlds.
    pub fn route(&mut self, host: &mut HostRuntime, surface: Option<PresentationSurface>) {
        if self
            .selected
            .is_some_and(|view| Some(view.surface) != surface)
        {
            self.selection(host, None);
        }
        if self.owner.as_ref().is_some_and(|owner| {
            owner
                .view
                .binding
                .output
                .world
                .resolve(host)
                .ok()
                .and_then(|world| host.root_output_binding(world).ok().flatten())
                .map(ipp_protocol::host::presentation::RootBinding::from)
                != Some(owner.view.binding)
        }) {
            self.release_owner(host);
        }
        self.synchronize(host);
        while let Some(pending) = self.pending.pop_front() {
            let result: Result<GuiRoutingDisposition, GuiInputError> = (|| {
                let owner = self
                    .owner
                    .as_mut()
                    .filter(|owner| {
                        owner.connection == pending.connection
                            && owner.id == pending.context
                            && Some(owner.view) == self.selected
                    })
                    .ok_or(GuiInputError::StaleContext)?;
                let output = owner
                    .view
                    .binding
                    .output
                    .resolve(host)
                    .map_err(|_| GuiInputError::StaleContext)?;
                let view = host
                    .resolve_view(ViewQueryTarget::RootView {
                        output,
                        expected_viewport: owner.view.binding.viewport,
                    })
                    .map_err(|_| GuiInputError::Unavailable)?;
                let result = self.router.route(
                    host,
                    &mut owner.context,
                    view,
                    pending.input,
                    &mut Delivery(pending.reply.clone()),
                )?;
                pending.reply.borrow_mut().scroll = self.router.scroll_remainder(&owner.context);
                Ok(result)
            })();
            {
                let mut reply = pending.reply.borrow_mut();
                reply.building = false;
                match result {
                    Ok(disposition) => {
                        reply.disposition = match disposition {
                            GuiRoutingDisposition::Routed {
                                ..
                            } => 0,
                            GuiRoutingDisposition::Miss => 1,
                            GuiRoutingDisposition::Blocked => 2,
                            GuiRoutingDisposition::Unhandled => 3,
                        }
                    }
                    Err(error) => {
                        reply.error = Some(error.to_string());
                        reply.rejected += 1;
                    }
                }
            }
            finish(&pending.reply);
            self.synchronize(host);
        }
    }

    fn synchronize(&mut self, host: &mut HostRuntime) {
        let Some(owner) = &mut self.owner else {
            return;
        };
        let view = owner
            .view
            .binding
            .output
            .resolve(host)
            .ok()
            .and_then(|output| {
                host.resolve_view(ViewQueryTarget::RootView {
                    output,
                    expected_viewport: owner.view.binding.viewport,
                })
                .ok()
            });
        let cancellation = self.router.synchronize(host, &mut owner.context, view);
        self.router.with_native_text(host, &owner.context, |state| {
            let presentation = state.map(|state| (state.fence, state.masked));
            if presentation == owner.native_presentation {
                return;
            }
            let result = (|| {
                let size = state
                    .map(ipp_protocol::host::gui_input::native_state_size)
                    .transpose()
                    .map_err(|_| ())?
                    .unwrap_or(0);
                let mut reservation = ReplyReservation::new(
                    owner.budget.clone(),
                    OutputClass::Ordinary,
                    512 + 2 * size,
                )
                .map_err(|_| ())?;
                let slot = owner.outbox.reserve().map_err(|_| ())?;
                let response = HostResponse {
                    connection: owner.connection,
                    request_id: 0,
                    body: HostResponseBody::GuiInput(GuiPhysicalResponse::Native {
                        context: owner.id,
                        state: state
                            .map(ipp_protocol::host::gui_input::encode_native_state)
                            .transpose()
                            .map_err(|_| ())?,
                    }),
                };
                let bytes = ipp_protocol::host::encode_host_response(&response).map_err(|_| ())?;
                drop(response);
                reservation.encoded(bytes.capacity());
                slot.settle(QueuedResponse {
                    bytes,
                    reservation: Rc::new(RefCell::new(reservation)),
                });
                owner.native_presentation = presentation;
                Ok::<(), ()>(())
            })();
            if result.is_err() {
                owner
                    .budget
                    .0
                    .fail(ipp_core::services::reliable_output::OutputFailure::Capacity);
            }
        });
        if cancellation.is_empty() {
            return;
        }
        let result = (|| {
            let slot = owner.outbox.reserve().map_err(|_| ())?;
            let mut reservation =
                ReplyReservation::new(owner.budget.clone(), OutputClass::Ordinary, 512)
                    .map_err(|_| ())?;
            reservation
                .reserve_retained(
                    ipp_core::services::gui_input::GUI_INPUT_MAX_POINTERS
                        * std::mem::size_of::<u64>(),
                )
                .map_err(|_| ())?;
            let response = HostResponse {
                connection: owner.connection,
                request_id: 0,
                body: HostResponseBody::GuiInput(GuiPhysicalResponse::Cancelled {
                    context: owner.id,
                    pointers: cancellation.pointers().to_vec(),
                    focus: cancellation.focus,
                }),
            };
            let bytes = ipp_protocol::host::encode_host_response(&response).map_err(|_| ())?;
            drop(response);
            reservation.encoded(bytes.capacity());
            slot.settle(QueuedResponse {
                bytes,
                reservation: Rc::new(RefCell::new(reservation)),
            });
            Ok::<(), ()>(())
        })();
        if result.is_err() {
            owner
                .budget
                .0
                .fail(ipp_core::services::reliable_output::OutputFailure::Capacity);
        }
    }
}
