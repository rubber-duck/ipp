//! Authority owns no source bytes. Accepted operations own async readers/private output.

use super::*;
use crate::services::{bulk_read::BulkOutputAllocation, task_scheduler::TaskHandle};
use ipp_core::services::asset_management::{
    AssetKey, AssetPublicationId, AssetSource, AssetTypeId,
    export::{
        AssetExportFormat, AssetExportFuture, AssetOutputObserver, cpu_formats, encode_cpu_snapshot,
    },
};
use ipp_core::services::io::{IoCancellation, IoReader, IoSourceRegistrationId};
use ipp_protocol::host::asset_export::{
    AssetExportRequest, AssetExportResponse, AssetReadAccess, AssetReadCapability,
    AssetReadRepresentation,
};
use std::{
    future::{Future, poll_fn},
    pin::Pin,
    sync::Arc,
    task::Poll,
};

/// Trusted Host policy identity; clients receive separate connection grants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PublicAssetSourceId(u64);

struct Authorization {
    source: AssetSource,
    identifier: String,
    registration: IoSourceRegistrationId,
    source_available: IoCancellation,
    access: AssetReadAccess,
}

struct Grant {
    authorization: Rc<Authorization>,
    cancelled: IoCancellation,
    policy: Option<PublicAssetSourceId>,
    _metadata: ipp_core::services::reliable_output::ReliableOutputLease,
}

impl Grant {
    fn check(&self) -> Result<(), String> {
        if self.cancelled.is_cancelled() {
            return Err("Asset read grant revoked".into());
        }
        if self.authorization.source_available.is_cancelled() {
            return Err("Asset source registration revoked".into());
        }
        Ok(())
    }
}

type AssetExportCompletion = ((u64, u64), Result<ExportOutput, String>);

type AssetExportCompletionQueue = Rc<RefCell<VecDeque<AssetExportCompletion>>>;

#[derive(Default)]
pub(super) struct AssetExportService {
    next_grant: u64,
    next_public: u64,
    grants: BTreeMap<(u64, u64), Rc<Grant>>,
    public: BTreeMap<u64, Rc<Authorization>>,
    pending: BTreeMap<(u64, u64), PendingExport>,
    completed: AssetExportCompletionQueue,
}

impl AssetExportService {
    pub(super) fn contains_pending_request(&self, connection: u64, request: u64) -> bool {
        self.pending.contains_key(&(connection, request))
    }
}

struct PendingExport {
    grant: Rc<Grant>,
    reservation: SharedReplyReservation,
    task: TaskHandle<()>,
    retention: Option<(AssetPublicationId, AssetKey)>,
    allocation: Option<Rc<BulkOutputAllocation>>,
    required_available: Option<IoCancellation>,
    representation: AssetReadRepresentation,
    format: Option<AssetExportFormat>,
}

enum ExportOutput {
    Reader(Box<dyn IoReader>),
    Encoded(Vec<u8>),
}

struct OperationOutput {
    grant: Rc<Grant>,
    allocation: Rc<BulkOutputAllocation>,
    available: IoCancellation,
}

impl AssetOutputObserver for OperationOutput {
    fn reserve(&self, bytes: usize) -> Result<(), String> {
        self.check()?;
        self.allocation.resize(
            bytes
                .checked_add(512)
                .ok_or("Asset export allocation overflow")?,
        )
    }

    fn check(&self) -> Result<(), String> {
        self.grant.check()?;
        if self.available.is_cancelled() {
            return Err("Asset working representation unloaded".into());
        }
        if self.allocation.cancellation().is_cancelled() {
            return Err("Asset export revoked by Host memory pressure".into());
        }
        Ok(())
    }
}

async fn revocable<F: Future<Output = Result<ExportOutput, String>>>(
    future: F,
    fences: Vec<IoCancellation>,
) -> Result<ExportOutput, String> {
    let mut future = std::pin::pin!(future);
    let mut cancelled: Vec<_> = fences.iter().map(IoCancellation::cancelled).collect();
    poll_fn(|cx| {
        for cancellation in &mut cancelled {
            if Pin::new(cancellation).poll(cx).is_ready() {
                return Poll::Ready(Err("Asset export operation revoked".into()));
            }
        }
        future.as_mut().poll(cx)
    })
    .await
}

impl<P: HostServices> Host<P> {
    fn asset_authorization(
        &self,
        source: AssetSource,
        access: AssetReadAccess,
    ) -> Result<Rc<Authorization>, String> {
        if source.kind.0 == 0 || source.uri.is_empty() {
            return Err("Invalid immutable asset source".into());
        }
        let identifier = self
            .runtime
            .asset_resources()
            .find(&source)
            .and_then(|key| self.runtime.asset_resources().get(key))
            .map(|provider| provider.read_identifier())
            .unwrap_or(&source.uri)
            .to_owned();
        if identifier.starts_with("asset-memory:") && !source.uri.starts_with("client://") {
            return Err("Internal mutable names cannot grant asset read authority".into());
        }
        let registration = self
            .runtime
            .io()
            .registration_id(&identifier)
            .ok_or("Asset source is unavailable")?;
        let source_available = self
            .runtime
            .io()
            .registration_cancellation(&identifier, registration)
            .ok_or("Asset source registration was replaced")?;
        Ok(Rc::new(Authorization {
            source,
            identifier,
            registration,
            source_available,
            access,
        }))
    }

    fn issue_asset_grant(
        &mut self,
        connection: u64,
        authorization: Rc<Authorization>,
        policy: Option<PublicAssetSourceId>,
    ) -> Result<AssetReadCapability, String> {
        if authorization.source_available.is_cancelled() {
            return Err("Asset source registration revoked".into());
        }
        let grant = self
            .connections
            .exports
            .next_grant
            .checked_add(1)
            .ok_or("Asset grant identity exhausted")?;
        let metadata = self.reserve_connection_output_bytes(
            connection,
            authorization
                .identifier
                .len()
                .checked_add(authorization.source.uri.len())
                .and_then(|bytes| bytes.checked_add(512))
                .ok_or("Asset grant metadata overflow")?,
        )?;
        self.connections.exports.next_grant = grant;
        self.connections.exports.grants.insert(
            (connection, grant),
            Rc::new(Grant {
                authorization,
                cancelled: Default::default(),
                policy,
                _metadata: metadata,
            }),
        );
        Ok(AssetReadCapability {
            connection,
            grant,
        })
    }

    /// Explicit trusted Host sharing policy, scoped to this exact source registration.
    /// Calling this API asserts that the supplied source identity is immutable.
    pub fn grant_asset_source(
        &mut self,
        connection: u64,
        source: AssetSource,
        access: AssetReadAccess,
    ) -> Result<AssetReadCapability, String> {
        let authorization = self.asset_authorization(source, access)?;
        self.issue_asset_grant(connection, authorization, None)
    }

    /// Explicitly expose immutable content for clients to resolve into connection grants.
    /// This policy retains authority only; it does not acquire or pin source bytes.
    pub fn expose_asset_source(
        &mut self,
        source: AssetSource,
        access: AssetReadAccess,
    ) -> Result<PublicAssetSourceId, String> {
        let authorization = self.asset_authorization(source, access)?;
        let id = self
            .connections
            .exports
            .next_public
            .checked_add(1)
            .ok_or("Public asset policy identity exhausted")?;
        self.connections.exports.next_public = id;
        self.connections.exports.public.insert(id, authorization);
        Ok(PublicAssetSourceId(id))
    }

    /// Revoke one exact public policy and every connection grant it issued.
    /// Already detached complete semantic outputs remain independent.
    pub fn revoke_public_asset_source(&mut self, policy: PublicAssetSourceId) -> bool {
        if self.connections.exports.public.remove(&policy.0).is_none() {
            return false;
        }
        self.connections.exports.grants.retain(|_, grant| {
            if grant.policy == Some(policy) {
                grant.cancelled.cancel();
                false
            } else {
                true
            }
        });
        true
    }

    /// Revoke one connection capability without changing producer ownership.
    pub fn revoke_asset_grant(&mut self, capability: AssetReadCapability) -> bool {
        let Some(grant) = self
            .connections
            .exports
            .grants
            .remove(&(capability.connection, capability.grant))
        else {
            return false;
        };
        grant.cancelled.cancel();
        true
    }

    pub(super) fn own_asset_access(&self, kind: AssetTypeId) -> AssetReadAccess {
        use AssetExportFormat::*;
        let format = match kind {
            ipp_core::MESH_TYPE => Some(MeshV3),
            ipp_core::TEXTURE_TYPE => Some(TextureV3),
            ipp_core::SKELETON_TYPE => Some(SkeletonV1),
            ipp_core::POSE_TYPE => Some(PoseV1),
            ipp_core::SKIN_TYPE => Some(SkinV1),
            ipp_core::services::asset_management::formats::shader::SHADER_TYPE => Some(ShaderV3),
            ipp_core::systems::animation::ANIMATION_TYPE => Some(AnimationV4),
            ipp_core::systems::geometry::GEOMETRY_TYPE => Some(GeometryV1),
            ipp_core::systems::particles::PARTICLE_CACHE_TYPE => Some(ParticleCacheV1),
            ipp_core::services::asset_management::formats::expression::EXPRESSION_TYPE => {
                Some(ExpressionV1)
            }
            _ => None,
        };
        AssetReadAccess {
            original: true,
            cpu: format.into_iter().collect(),
            gpu: self.services.asset_gpu_formats(kind),
        }
    }

    pub(super) fn begin_asset_export(
        &mut self,
        connection: &mut HostConnectionState,
        request: u64,
        operation: AssetExportRequest,
    ) -> Result<Option<AssetExportResponse>, String> {
        match operation {
            AssetExportRequest::Find(source) => {
                let existing = self
                    .connections
                    .exports
                    .grants
                    .iter()
                    .find(|((owner, _), grant)| {
                        *owner == connection.id
                            && grant.authorization.source == source
                            && grant.check().is_ok()
                    })
                    .map(|((_, id), _)| AssetReadCapability {
                        connection: connection.id,
                        grant: *id,
                    });
                let capability = if let Some(capability) = existing {
                    capability
                } else {
                    let (policy, authorization) = self
                        .connections
                        .exports
                        .public
                        .iter()
                        .find(|(_, authorization)| {
                            authorization.source == source
                                && !authorization.source_available.is_cancelled()
                        })
                        .map(|(id, authorization)| {
                            (PublicAssetSourceId(*id), authorization.clone())
                        })
                        .ok_or("Asset source read access denied")?;
                    // The connection is temporarily removed while its control queue is applied.
                    let id = self
                        .connections
                        .exports
                        .next_grant
                        .checked_add(1)
                        .ok_or("Asset grant identity exhausted")?;
                    let metadata = connection
                        .reply_budget
                        .0
                        .reserve(ipp_core::services::reliable_output::OutputCharge {
                            entries: 0,
                            bytes: authorization.identifier.len()
                                + authorization.source.uri.len()
                                + 512,
                        })
                        .map_err(|error| format!("Asset grant metadata unavailable: {error:?}"))?;
                    self.connections.exports.next_grant = id;
                    self.connections.exports.grants.insert(
                        (connection.id, id),
                        Rc::new(Grant {
                            authorization,
                            policy: Some(policy),
                            cancelled: Default::default(),
                            _metadata: metadata,
                        }),
                    );
                    AssetReadCapability {
                        connection: connection.id,
                        grant: id,
                    }
                };
                let grant = &self.connections.exports.grants[&(connection.id, capability.grant)];
                let mut access = grant.authorization.access.clone();
                let supported_cpu = self
                    .runtime
                    .asset_resources()
                    .find(&grant.authorization.source)
                    .and_then(|key| self.runtime.asset_resources().get(key))
                    .and_then(|provider| provider.cpu_export_snapshot())
                    .map(|snapshot| cpu_formats(&*snapshot.data))
                    .unwrap_or_default();
                access.cpu.retain(|format| supported_cpu.contains(format));
                let supported_gpu = self
                    .services
                    .asset_gpu_formats(grant.authorization.source.kind);
                access.gpu.retain(|format| supported_gpu.contains(format));
                Ok(Some(AssetExportResponse::Capability {
                    capability,
                    source: grant.authorization.source.clone(),
                    access,
                }))
            }
            AssetExportRequest::Revoke(capability) => {
                if capability.connection != connection.id {
                    return Err("Asset read access denied".into());
                }
                if !self.revoke_asset_grant(capability) {
                    return Err("Asset read capability is unavailable".into());
                }
                Ok(Some(AssetExportResponse::Revoked))
            }
            AssetExportRequest::Read {
                capability,
                representation,
                format,
            } => {
                if capability.connection != connection.id {
                    return Err("Asset read access denied".into());
                }
                let grant = self
                    .connections
                    .exports
                    .grants
                    .get(&(connection.id, capability.grant))
                    .cloned()
                    .ok_or("Asset read capability is unavailable")?;
                grant.check()?;
                if !grant.authorization.access.allows(representation, format) {
                    return Err("Asset read representation or format not granted".into());
                }
                let mut retention = None;
                let mut allocation = None;
                let mut required_available = None;
                let future: Pin<Box<dyn Future<Output = Result<ExportOutput, String>>>>;
                let mut fences = vec![
                    grant.cancelled.clone(),
                    grant.authorization.source_available.clone(),
                ];
                if representation == AssetReadRepresentation::Original {
                    // Memory sources capture Arc backing here before asynchronous admission.
                    let open = self.runtime.io_mut().open_read_registered(
                        &grant.authorization.identifier,
                        grant.authorization.registration,
                        ipp_core::services::io::IoReadOptions {
                            max_bytes: None,
                            recovery: false,
                        },
                        grant.cancelled.clone(),
                    );
                    future = Box::pin(async move { Ok(ExportOutput::Reader(open.await?)) });
                } else {
                    let key = self
                        .runtime
                        .asset_resources()
                        .find(&grant.authorization.source)
                        .ok_or("Asset working representation unavailable")?;
                    let provider = self
                        .runtime
                        .asset_resources()
                        .get(key)
                        .ok_or("Asset working representation unavailable")?;
                    let format = format.expect("granted typed format");
                    let cpu = if representation == AssetReadRepresentation::Cpu {
                        Some(
                            provider
                                .cpu_export_snapshot()
                                .ok_or("Complete CPU asset representation unavailable")?,
                        )
                    } else {
                        None
                    };
                    if let Some(cpu) = &cpu
                        && !cpu_formats(&*cpu.data).contains(&format)
                    {
                        return Err("Unsupported CPU asset export format".into());
                    }
                    let available = cpu
                        .as_ref()
                        .map(|cpu| cpu.available.clone())
                        .unwrap_or_else(|| provider.gpu_export_availability());
                    required_available = Some(available.clone());
                    let charge = Rc::new(self.reserve_bulk_output(512)?);
                    fences.push(charge.cancellation());
                    fences.push(available.clone());
                    let observer = Rc::new(OperationOutput {
                        grant: grant.clone(),
                        allocation: charge.clone(),
                        available,
                    });
                    let encode: AssetExportFuture = if let Some(cpu) = cpu {
                        Box::pin(encode_cpu_snapshot(cpu, format, observer))
                    } else {
                        let provider = self
                            .runtime
                            .asset_resources()
                            .get(key)
                            .ok_or("Asset GPU representation unavailable")?;
                        self.services.asset_gpu_export(provider, format, observer)?
                    };
                    let publication = self
                        .runtime
                        .asset_resources_mut()
                        .retain_publication([key])?;
                    retention = Some((publication, key));
                    allocation = Some(charge);
                    future = Box::pin(async move { Ok(ExportOutput::Encoded(encode.await?)) });
                }
                let completed = self.connections.exports.completed.clone();
                let id = (connection.id, request);
                let task = self.task_schedulers().host().spawn(async move {
                    let result = revocable(future, fences).await;
                    completed.borrow_mut().push_back((id, result));
                });
                let reservation = connection
                    .reply_reservations
                    .remove(&request)
                    .expect("admitted asset request reply");
                self.connections.exports.pending.insert(
                    (connection.id, request),
                    PendingExport {
                        grant,
                        reservation,
                        task,
                        retention,
                        allocation,
                        required_available,
                        representation,
                        format,
                    },
                );
                Ok(None)
            }
        }
    }

    /// Admit completed private results at the Host service boundary, never from task callbacks.
    pub(crate) fn progress_asset_exports(&mut self) {
        let completed = std::mem::take(&mut *self.connections.exports.completed.borrow_mut());
        for ((connection, request), result) in completed {
            let Some(pending) = self
                .connections
                .exports
                .pending
                .remove(&(connection, request))
            else {
                continue;
            };
            let valid = pending.grant.check().and_then(|()| {
                if pending
                    .required_available
                    .as_ref()
                    .is_some_and(IoCancellation::is_cancelled)
                {
                    return Err(
                        "Asset working representation unloaded before export publication".into(),
                    );
                }
                if pending
                    .allocation
                    .as_ref()
                    .is_some_and(|allocation| allocation.cancellation().is_cancelled())
                {
                    return Err(
                        "Asset export revoked by Host memory pressure before publication".into(),
                    );
                }
                if let Some((publication, key)) = pending.retention
                    && self
                        .runtime
                        .asset_resources()
                        .publication_resource(publication, key)
                        .is_none()
                {
                    return Err("Asset representation unavailable before export publication".into());
                }
                Ok(())
            });
            let result = valid.and(result).and_then(|output| match output {
                ExportOutput::Reader(reader) => {
                    self.publish_connection_reader(connection, reader, None)
                }
                ExportOutput::Encoded(bytes) => {
                    let charge =
                        Rc::try_unwrap(pending.allocation.expect("typed output allocation"))
                            .map_err(|_| "Asset output still borrowed at publication")?;
                    self.publish_connection_bytes_from_allocation(
                        connection,
                        Arc::new(bytes),
                        charge,
                    )
                }
            });
            if let Some((publication, _)) = pending.retention {
                self.runtime
                    .asset_resources_mut()
                    .release_publication(publication);
            }
            drop(pending.task);
            if let Some(state) = self.connections.states.get_mut(&connection) {
                state
                    .reply_reservations
                    .insert(request, pending.reservation);
                let body = match result {
                    Ok(read) => HostResponseBody::AssetExport(AssetExportResponse::Read {
                        read,
                        representation: pending.representation,
                        format: pending.format,
                    }),
                    Err(error) => HostResponseBody::Error(error),
                };
                if let Err(error) = state.reply(request, body) {
                    state.failure = Some(error);
                }
            }
        }
    }

    pub(super) fn close_asset_exports(&mut self, connection: u64) {
        let _ = self.services.prepare_task_poll(&mut self.runtime);
        self.connections
            .exports
            .completed
            .borrow_mut()
            .retain(|((owner, _), _)| *owner != connection);
        self.connections.exports.grants.retain(|(owner, _), grant| {
            if *owner == connection {
                grant.cancelled.cancel();
                false
            } else {
                true
            }
        });
        let pending: Vec<_> = self
            .connections
            .exports
            .pending
            .keys()
            .copied()
            .filter(|(owner, _)| *owner == connection)
            .collect();
        for id in pending {
            let operation = self.connections.exports.pending.remove(&id).unwrap();
            drop(operation.task);
            if let Some((publication, _)) = operation.retention {
                self.runtime
                    .asset_resources_mut()
                    .release_publication(publication);
            }
        }
    }
}

#[cfg(test)]
#[path = "asset_exports_tests.rs"]
mod tests;
