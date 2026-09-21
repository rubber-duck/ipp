//! Immutable client source delivery never enters the World command queue.

use super::*;
use ipp_core::services::asset_management::AssetSource;
use ipp_protocol::asset_source::{self, SourceOperation};

pub(crate) struct ClientSourceRecord {
    pub(crate) active: bool,
}

pub(crate) struct SourceTransfer {
    source: AssetSource,
    length: usize,
    bytes: Vec<u8>,
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
        let session = state
            .session
            .ok_or("Connection is not attached to a World")?;
        if !state.can_reserve_reply(&self.sessions) {
            return Err("connection congestion: asset source reply capacity exhausted".into());
        }
        let request = asset_source::decode(bytes, session).map_err(|error| error.to_string())?;
        let result = self.apply_asset_source(session, request.id, request.operation);
        let response = asset_source::response(session, request.id, result)
            .map_err(|error| error.to_string())?;
        self.connections
            .states
            .get_mut(&connection)
            .expect("live connection")
            .outbox
            .push_back(response);
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
                        length,
                        bytes: Vec::new(),
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
                let end = input
                    .bytes
                    .len()
                    .checked_add(bytes.len())
                    .ok_or("Asset source length overflow")?;
                if offset != input.bytes.len() as u64 || end > input.length || bytes.is_empty() {
                    session.source_transfers.remove(&transfer);
                    return Err("Invalid asset source chunk bounds".into());
                }
                if input.bytes.try_reserve(bytes.len()).is_err() {
                    session.source_transfers.remove(&transfer);
                    return Err("Asset source allocation failed".into());
                }
                input.bytes.extend_from_slice(bytes);
            }
            SourceOperation::Finish {
                transfer,
            } => {
                let input = session
                    .source_transfers
                    .remove(&transfer)
                    .ok_or("Unknown asset source transfer")?;
                if input.bytes.len() != input.length {
                    return Err("Incomplete asset source transfer".into());
                }
                self.runtime
                    .world_mut(session.world)
                    .ok_or("World is closed")?
                    .asset_resources_mut()
                    .register_client_source(session.world, input.source.clone(), input.bytes)?;
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
