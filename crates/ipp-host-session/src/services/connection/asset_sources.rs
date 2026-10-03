//! Immutable client source delivery never enters the World command queue.

use super::*;
use ipp_core::services::asset_management::AssetSource;
use ipp_core::services::io::{IoUploadAssembly, IoUploadError};
use ipp_protocol::asset_source::{self, SourceOperation};

pub(crate) struct ClientSourceRecord {
    pub(crate) active: bool,
}

pub(crate) struct SourceTransfer {
    source: AssetSource,
    upload: IoUploadAssembly,
}

impl<P: HostServices> Host<P> {
    pub(super) fn receive_asset_source(
        &mut self,
        connection: u64,
        bytes: &[u8],
    ) -> Result<(), String> {
        let state = self
            .connections
            .states
            .get(&connection)
            .ok_or("Connection is closed")?;
        let start = asset_source::REQUEST_MAGIC.len();
        let session = u64::from_le_bytes(
            bytes
                .get(start..start + 8)
                .ok_or("Truncated asset session")?
                .try_into()
                .unwrap(),
        );
        if !state.holds_session(session)? {
            ipp_core::diagnostic!(
                Debug,
                "[IPP {}] session.stale connection={} session={}",
                P::NAME,
                connection,
                session
            );
            return Ok(());
        }
        if state.admitted_requests(&self.sessions) >= crate::MAX_PENDING {
            return Err("connection congestion: asset source reply capacity exhausted".into());
        }
        let request = asset_source::decode(bytes, session).map_err(|error| error.to_string())?;
        let reservation = state.reserve_reply(8192)?;
        let result = self.apply_asset_source(session, request.id, request.operation);
        let response = asset_source::response(session, request.id, result)
            .map_err(|error| error.to_string())?;
        reservation.borrow_mut().encoded(response.capacity());
        self.connections
            .states
            .get_mut(&connection)
            .expect("live connection")
            .outbox
            .push_back(ReliableResponse {
                bytes: response,
                reservation,
            });
        Ok(())
    }

    fn apply_asset_source(
        &mut self,
        session_id: u64,
        id: u64,
        operation: SourceOperation<'_>,
    ) -> Result<(), String> {
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or("Session is closed")?;
        match operation {
            SourceOperation::Begin {
                source,
                length,
            } => {
                validate_source(session_id, &source)?;
                if session.source_transfers.len() >= 8 {
                    return Err("Asset source transfer capacity exhausted".into());
                }
                if session.source_transfers.contains_key(&id)
                    || session.client_sources.contains_key(&source)
                    || session
                        .source_transfers
                        .values()
                        .any(|transfer| transfer.source == source)
                {
                    return Err(
                        "Immutable asset source already registered or being delivered".into(),
                    );
                }
                let length = usize::try_from(length)
                    .map_err(|_| "Asset source length cannot be represented")?;
                session.source_transfers.insert(
                    id,
                    SourceTransfer {
                        source,
                        upload: IoUploadAssembly::new(length),
                    },
                );
            }
            SourceOperation::Chunk {
                transfer,
                offset,
                bytes,
            } => {
                let input = session
                    .source_transfers
                    .get_mut(&transfer)
                    .ok_or("Unknown asset source transfer")?;
                if let Err(error) = input.upload.push(offset, bytes) {
                    session.source_transfers.remove(&transfer);
                    return Err(asset_upload_error(error).into());
                }
            }
            SourceOperation::Finish {
                transfer,
            } => {
                let input = session
                    .source_transfers
                    .remove(&transfer)
                    .ok_or("Unknown asset source transfer")?;
                let bytes = input.upload.finish().map_err(asset_upload_error)?;
                self.runtime
                    .world_mut(session.world)
                    .ok_or("World is closed")?
                    .asset_resources_mut()
                    .register_client_source(session.world, input.source.clone(), bytes)?;
                session.client_sources.insert(
                    input.source,
                    ClientSourceRecord {
                        active: true,
                    },
                );
            }
            SourceOperation::Cancel {
                transfer,
            } => {
                session.source_transfers.remove(&transfer);
            }
            SourceOperation::Release {
                source,
            } => {
                validate_source(session_id, &source)?;
                if session
                    .client_sources
                    .get_mut(&source)
                    .is_some_and(|record| std::mem::replace(&mut record.active, false))
                {
                    self.runtime
                        .world_mut(session.world)
                        .ok_or("World is closed")?
                        .asset_resources_mut()
                        .release_client_source(session.world, &source);
                }
            }
        }
        Ok(())
    }
}

fn asset_upload_error(error: IoUploadError) -> &'static str {
    match error {
        IoUploadError::LengthOverflow => "Asset source length overflow",
        IoUploadError::InvalidChunkBounds => "Invalid asset source chunk bounds",
        IoUploadError::AllocationFailed => "Asset source allocation failed",
        IoUploadError::Incomplete => "Incomplete asset source transfer",
    }
}

fn validate_source(session: u64, source: &AssetSource) -> Result<(), String> {
    let prefix = format!("client://{session}/");
    if source.kind.0 == 0
        || !source.uri.starts_with(&prefix)
        || !source.uri[prefix.len()..].contains('#')
    {
        return Err("Client asset source is outside this session scope".into());
    }
    Ok(())
}
