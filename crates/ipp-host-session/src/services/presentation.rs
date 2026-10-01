//! One actual surface selection, bounded completion waiters and immutable CPU transfers.

use crate::{HostPresentationFailure, HostServices};
use ipp_core::{HostRuntime, WorldId};
use ipp_protocol::presentation::*;
use std::{collections::BTreeMap, sync::Arc, time::Duration};

/// Pending completion waiters plus retained captures one connection may hold.
///
/// Presentation is one selected draw per Host, so a client needs few outstanding
/// waits: one frame in flight, one being read and room to pipeline the next. The
/// per-connection allowances are primary; a connection over them receives
/// [`PresentationError::Capacity`] for that request only.
const PER_CONNECTION: usize = 4;

/// Pending and retained capture bytes one connection may hold: one RGBA capture
/// of the 4096 by 4096 surfaces the maintained hosts advertise.
const PER_CONNECTION_CAPTURE_BYTES: usize = 64 * 1024 * 1024;

/// Connections that may each hold their full allowance at once.
///
/// The Host figures are derived from the per-connection ones so that fewer than
/// this many connections at their allowance can never exhaust the Host pool for
/// another connection; the pool still bounds the Host's total capture memory.
const PRESENTING_CONNECTIONS: usize = 4;

/// Host-wide waiters and captures, derived from [`PER_CONNECTION`].
const MAX_REQUESTS: usize = PER_CONNECTION * PRESENTING_CONNECTIONS;

/// Host-wide capture bytes, derived from [`PER_CONNECTION_CAPTURE_BYTES`].
const MAX_CAPTURE_BYTES: usize = PER_CONNECTION_CAPTURE_BYTES * PRESENTING_CONNECTIONS;

/// Longest wait for an eligible draw, from ingress: a hidden page or a stalled
/// renderer fails the waiter with a timeout instead of holding its slot.
const FRAME_TIMEOUT: Duration = Duration::from_secs(5);

/// Unreleased capture lifetime: long enough to read a full capture in 64 KiB
/// chunks, short enough that a client that never releases one gets its bytes
/// back for further captures.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) fn frame_deadline(received_at: Duration) -> Duration {
    received_at.saturating_add(FRAME_TIMEOUT)
}

/// A successful draw only. Invalid-camera clears must return an error instead.
#[derive(Clone, Copy, Debug, Default)]
pub struct PresentationDrawSummary {
    /// Completed main-pass submissions.
    pub draw_calls: u32,
    /// Submitted triangles.
    pub triangles: u32,
    /// Failed individual draws; not whole-frame failure.
    pub failed_draw_calls: u32,
}

/// Borrowed destinations for this draw only; completion never retains these slots.
pub struct PresentationCompletion<'a> {
    /// Optional immutable capture destination.
    pub capture: Option<&'a mut [u8]>,
    /// Requested exact outputs; fill only sources actually included by this draw.
    pub outputs: &'a mut [ipp_core::OutputPublicationObservation],
}

struct PendingFrame {
    view: PresentationView,
    after: u64,
    publication: Option<PresentationIdentity>,
    capture_bytes: usize,
    expires: Duration,
    outputs: Vec<(ipp_core::OutputRef, u64)>,
}

struct Capture {
    bytes: Arc<Vec<u8>>,
    expires: Duration,
}

#[derive(Default)]
pub(crate) struct PresentationCoordinator {
    selected: Option<PresentationView>,
    selection: u64,
    sequence: u64,
    next_capture: u64,
    pending: BTreeMap<(u64, u64), PendingFrame>,
    captures: BTreeMap<(u64, u64), Capture>,
    completed: Vec<(u64, u64, PresentationResponse)>,
}

fn current_binding(host: &HostRuntime, binding: RootBinding) -> bool {
    binding
        .output
        .world
        .resolve(host)
        .ok()
        .and_then(|world| host.root_output_binding(world).ok().flatten())
        .is_some_and(|current| RootBinding::from(current) == binding)
}

impl PresentationCoordinator {
    fn validate<P: HostServices>(
        &self,
        host: &HostRuntime,
        services: &P,
        view: PresentationView,
    ) -> Result<(), PresentationError> {
        let surface = services.presentation_surface()?;
        if self.selected != Some(view)
            || surface != view.surface
            || !current_binding(host, view.binding)
        {
            return Err(PresentationError::StaleView);
        }
        view.binding
            .output
            .resolve(host)
            .map_err(|_| PresentationError::StaleView)?;
        Ok(())
    }

    fn used_bytes(&self) -> usize {
        self.pending
            .values()
            .map(|pending| pending.capture_bytes)
            .sum::<usize>()
            + self
                .captures
                .values()
                .map(|capture| capture.bytes.len())
                .sum::<usize>()
    }

    /// Waiters, captures and capture bytes one connection holds.
    fn connection_usage(&self, connection: u64) -> (usize, usize) {
        let pending = self
            .pending
            .iter()
            .filter(|((owner, _), _)| *owner == connection)
            .map(|(_, pending)| pending.capture_bytes);
        let captures = self
            .captures
            .iter()
            .filter(|((owner, _), _)| *owner == connection)
            .map(|(_, capture)| capture.bytes.len());
        pending
            .chain(captures)
            .fold((0, 0), |(count, bytes), item| (count + 1, bytes + item))
    }

    pub(crate) fn request<P: HostServices>(
        &mut self,
        host: &mut HostRuntime,
        services: &mut P,
        connection: u64,
        request: u64,
        body: PresentationRequest,
        received_at: Duration,
    ) -> Option<PresentationResponse> {
        match self.apply(host, services, connection, request, body, received_at) {
            Ok(response) => response,
            Err(error) => Some(PresentationResponse::Error(error)),
        }
    }

    fn apply<P: HostServices>(
        &mut self,
        host: &mut HostRuntime,
        services: &mut P,
        connection: u64,
        request: u64,
        body: PresentationRequest,
        received_at: Duration,
    ) -> Result<Option<PresentationResponse>, PresentationError> {
        use PresentationResponse as Reply;
        let response = match body {
            PresentationRequest::Surface => Reply::Surface(services.presentation_surface()?),
            PresentationRequest::Select {
                surface,
                binding,
            } => {
                if services.presentation_surface()? != surface || !current_binding(host, binding) {
                    return Err(PresentationError::StaleView);
                }
                binding
                    .output
                    .resolve(host)
                    .map_err(|_| PresentationError::StaleView)?;
                let viewport = binding.viewport;
                if viewport.width == 0
                    || viewport.height == 0
                    || viewport.width > surface.max_width
                    || viewport.height > surface.max_height
                    || !viewport.device_pixel_ratio.is_finite()
                    || viewport.device_pixel_ratio <= 0.0
                {
                    return Err(PresentationError::InvalidViewport);
                }
                let selection = self
                    .selection
                    .checked_add(1)
                    .ok_or(PresentationError::Capacity)?;
                services.configure_presentation(viewport)?;
                if services.presentation_surface()? != surface {
                    return Err(PresentationError::StaleView);
                }
                self.selection = selection;
                let view = PresentationView {
                    surface,
                    selection,
                    binding,
                };
                self.selected = Some(view);
                services.presentation_selection(host, self.selected);
                Reply::View(view)
            }
            PresentationRequest::Clear(view) => {
                if self.selected == Some(view) {
                    self.selected = None;
                    services.presentation_selection(host, None);
                }
                Reply::Complete
            }
            PresentationRequest::Frame {
                view,
                after_sequence,
                publication,
                capture,
                after_outputs,
            } => {
                self.validate(host, services, view)?;
                let mut outputs = after_outputs
                    .into_iter()
                    .map(|reference| {
                        let output = reference
                            .resolve(host)
                            .map_err(|_| PresentationError::Unavailable)?;
                        let tick = host
                            .output_evaluation_cut(output)
                            .map_err(|_| PresentationError::Unavailable)?;
                        Ok((output, tick))
                    })
                    .collect::<Result<Vec<_>, PresentationError>>()?;
                outputs.sort_unstable_by_key(|(output, _)| *output);
                outputs.dedup_by_key(|(output, _)| *output);
                let (held, held_bytes) = self.connection_usage(connection);
                if self.pending.contains_key(&(connection, request))
                    || held >= PER_CONNECTION
                    || self.pending.len() + self.captures.len() >= MAX_REQUESTS
                {
                    return Err(PresentationError::Capacity);
                }
                let capture_bytes = if capture {
                    let viewport = view.binding.viewport;
                    usize::try_from(viewport.width)
                        .ok()
                        .and_then(|width| width.checked_mul(viewport.height as usize))
                        .and_then(|pixels| pixels.checked_mul(4))
                        .filter(|bytes| {
                            *bytes <= PER_CONNECTION_CAPTURE_BYTES.saturating_sub(held_bytes)
                                && *bytes <= MAX_CAPTURE_BYTES.saturating_sub(self.used_bytes())
                        })
                        .ok_or(PresentationError::Capacity)?
                } else {
                    0
                };
                self.pending.insert(
                    (connection, request),
                    PendingFrame {
                        view,
                        after: after_sequence.unwrap_or(self.sequence).max(self.sequence),
                        publication,
                        capture_bytes,
                        expires: frame_deadline(received_at),
                        outputs,
                    },
                );
                return Ok(None);
            }
            PresentationRequest::ReadCapture {
                capture,
                offset,
            } => {
                let snapshot = self
                    .captures
                    .get(&(connection, capture))
                    .ok_or(PresentationError::Unavailable)?;
                let offset = usize::try_from(offset).map_err(|_| PresentationError::Capacity)?;
                if offset >= snapshot.bytes.len() {
                    return Err(PresentationError::Unavailable);
                }
                Reply::Chunk {
                    capture,
                    offset: offset as u64,
                    bytes: snapshot.bytes[offset
                        ..snapshot
                            .bytes
                            .len()
                            .min(offset.saturating_add(ipp_protocol::MAX_FIELD_BYTES))]
                        .to_vec(),
                }
            }
            PresentationRequest::ReleaseCapture(capture) => {
                self.captures.remove(&(connection, capture));
                Reply::Complete
            }
            PresentationRequest::CancelFrame {
                request,
            } => {
                if self.pending.remove(&(connection, request)).is_some() {
                    self.completed.push((
                        connection,
                        request,
                        Reply::Error(PresentationError::Unavailable),
                    ));
                }
                Reply::Complete
            }
        };
        Ok(Some(response))
    }

    pub(crate) fn expire(&mut self, now: Duration) {
        self.captures.retain(|_, capture| capture.expires > now);
        let expired: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, pending)| pending.expires <= now)
            .map(|(key, _)| *key)
            .collect();
        for (connection, request) in expired {
            self.pending.remove(&(connection, request));
            self.completed.push((
                connection,
                request,
                PresentationResponse::Error(PresentationError::Timeout),
            ));
        }
    }

    /// Configuration is Host-owned; disconnect revokes only the connection's requests/data.
    pub(crate) fn disconnect(&mut self, connection: u64) {
        self.pending.retain(|(owner, _), _| *owner != connection);
        self.captures.retain(|(owner, _), _| *owner != connection);
        self.completed.retain(|(owner, _, _)| *owner != connection);
    }

    pub(crate) fn take_completed(&mut self) -> Vec<(u64, u64, PresentationResponse)> {
        std::mem::take(&mut self.completed)
    }

    pub(crate) fn draw<P: HostServices>(
        &mut self,
        host: &mut HostRuntime,
        services: &mut P,
        time: f64,
        now: Duration,
    ) -> Option<(WorldId, HostPresentationFailure)> {
        self.expire(now);
        let rejected: Vec<_> = self
            .pending
            .iter()
            .filter_map(|(key, pending)| {
                self.validate(host, services, pending.view)
                    .and_then(|()| {
                        for (output, _) in &pending.outputs {
                            host.output_evaluation_cut(*output)
                                .map_err(|_| PresentationError::Unavailable)?;
                        }
                        Ok(())
                    })
                    .err()
                    .map(|error| (*key, error))
            })
            .collect();
        for ((connection, request), error) in rejected {
            self.pending.remove(&(connection, request));
            self.completed
                .push((connection, request, PresentationResponse::Error(error)));
        }
        let Some(view) = self
            .selected
            .filter(|view| self.validate(host, services, *view).is_ok())
        else {
            self.selected = None;
            services.presentation_selection(host, None);
            let _ = services.prepare_presentation(host, None);
            return None;
        };
        let world = WorldId(view.binding.output.world.id);
        let Some((output, viewport, publication)) = host.root_output(world) else {
            self.fail_pending(PresentationError::Unavailable);
            let _ = services.prepare_presentation(host, None);
            return None;
        };
        let (identity, revision) = publication.identity();
        let stamp = PresentationIdentity {
            host: identity,
            serial: revision,
        };
        let obsolete: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, pending)| {
                pending
                    .publication
                    .is_some_and(|expected| expected != stamp)
            })
            .map(|(key, _)| *key)
            .collect();
        for (connection, request) in obsolete {
            self.pending.remove(&(connection, request));
            self.completed.push((
                connection,
                request,
                PresentationResponse::Error(PresentationError::ObsoletePublication),
            ));
        }
        let Some(sequence) = self.sequence.checked_add(1) else {
            self.fail_pending(PresentationError::Capacity);
            return None;
        };
        let capture_bytes = self
            .pending
            .values()
            .filter(|pending| pending.after < sequence)
            .map(|pending| pending.capture_bytes)
            .max()
            .unwrap_or(0);
        let mut pixels = Vec::new();
        let mut observed: Vec<_> =
            self.pending
                .values()
                .flat_map(|pending| {
                    pending.outputs.iter().map(|(output, _)| {
                        ipp_core::OutputPublicationObservation {
                            output: *output,
                            publication: None,
                        }
                    })
                })
                .collect();
        observed.sort_unstable_by_key(|source| source.output);
        observed.dedup_by_key(|source| source.output);
        if pixels.try_reserve_exact(capture_bytes).is_err() {
            self.fail_pending(PresentationError::Capacity);
            return None;
        }
        pixels.resize(capture_bytes, 0);
        let result = services
            .prepare_presentation(host, Some((output, publication)))
            .and_then(|()| {
                services.present(
                    host,
                    output,
                    publication,
                    viewport,
                    time,
                    PresentationCompletion {
                        capture: (capture_bytes != 0).then_some(pixels.as_mut_slice()),
                        outputs: &mut observed,
                    },
                )
            });
        let summary = match result {
            Ok(summary) => summary,
            Err(error) => {
                self.fail_pending(PresentationError::DrawFailed);
                return Some((world, error));
            }
        };
        if self.validate(host, services, view).is_err()
            || host.root_output(world) != Some((output, viewport, publication))
        {
            self.fail_pending(PresentationError::StaleView);
            return None;
        }
        self.sequence = sequence;
        let frame = PresentedFrame {
            view,
            sequence,
            publication: stamp,
            draw_calls: summary.draw_calls,
            triangles: summary.triangles,
            failed_draw_calls: summary.failed_draw_calls,
            sources: Vec::new(),
        };
        let pixels = Arc::new(pixels);
        let ready: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, pending)| pending.after < sequence)
            .filter(|(_, pending)| {
                pending.outputs.iter().all(|(output, tick)| {
                    observed
                        .binary_search_by_key(output, |source| source.output)
                        .ok()
                        .and_then(|index| observed[index].publication)
                        .and_then(|publication| host.publication(publication))
                        .is_some_and(|publication| {
                            publication.world == output.world()
                                && publication.tick >= *tick
                                && host.output(publication.id, *output).is_some()
                        })
                })
            })
            .map(|(key, _)| *key)
            .collect();
        for (connection, request) in ready {
            let pending = self
                .pending
                .remove(&(connection, request))
                .expect("pending frame");
            let mut frame = frame.clone();
            frame.sources = pending
                .outputs
                .into_iter()
                .map(|(output, minimum_tick)| {
                    let index = observed
                        .binary_search_by_key(&output, |source| source.output)
                        .expect("observed output");
                    let publication = host
                        .publication(observed[index].publication.expect("included source"))
                        .expect("available source");
                    let (host, serial) = publication.id.identity();
                    PresentedSource {
                        output: output.into(),
                        minimum_tick,
                        publication: PresentationIdentity {
                            host,
                            serial,
                        },
                        tick: publication.tick,
                    }
                })
                .collect();
            let reply = if pending.capture_bytes != 0 {
                match self.next_capture.checked_add(1) {
                    Some(capture) => {
                        self.next_capture = capture;
                        self.captures.insert(
                            (connection, capture),
                            Capture {
                                bytes: pixels.clone(),
                                expires: now.saturating_add(CAPTURE_TIMEOUT),
                            },
                        );
                        PresentationResponse::Capture {
                            frame,
                            capture,
                            bytes: pixels.len() as u64,
                        }
                    }
                    None => PresentationResponse::Error(PresentationError::Capacity),
                }
            } else {
                PresentationResponse::Frame(frame)
            };
            self.completed.push((connection, request, reply));
        }
        None
    }

    fn fail_pending(&mut self, error: PresentationError) {
        for ((connection, request), _) in std::mem::take(&mut self.pending) {
            self.completed
                .push((connection, request, PresentationResponse::Error(error)));
        }
    }
}

#[cfg(test)]
#[path = "presentation_tests.rs"]
mod tests;
