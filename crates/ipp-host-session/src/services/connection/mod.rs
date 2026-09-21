//! Host-first connections, discovery and attachment lifetime.

use crate::{Host, HostServices, WorldSession};
use ipp_core::{WorldDescriptor, WorldId};
use ipp_protocol::host::{self, HostRequest, HostRequestBody, HostResponse, HostResponseBody};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub(crate) mod scope;

pub(crate) mod asset_sources;
mod persistence;

/// Process-owned connection and attachment routing, independent of World instances.
#[derive(Default)]
pub(crate) struct HostConnectionService {
    states: BTreeMap<u64, HostConnectionState>,
    pub(crate) now: std::time::Duration,
    last_persistence_connection: u64,
    persistence: persistence::HostPersistenceService,
}

struct HostConnectionState {
    id: u64,
    ready: bool,
    throttled: bool,
    session: Option<u64>,
    temporary_worlds: BTreeSet<WorldId>,
    pending: VecDeque<HostConnectionIngress>,
    outbox: VecDeque<Vec<u8>>,
    failure: Option<String>,
    transfer: Option<persistence::HostWorldTransfer>,
}

enum HostConnectionIngress {
    Control(HostRequest),
    World(Vec<u8>),
    DecodedWorld(ipp_protocol::Request),
}

impl HostConnectionState {
    fn reserved_replies(&self, sessions: &BTreeMap<u64, WorldSession>) -> usize {
        self.pending.len()
            + self.outbox.len()
            + self
                .session
                .and_then(|id| sessions.get(&id))
                .map_or(0, |session| {
                    session.outbox.len() + session.request_origins.len()
                })
    }

    fn can_reserve_reply(&self, sessions: &BTreeMap<u64, WorldSession>) -> bool {
        self.reserved_replies(sessions) + crate::EVENT_RESERVE < crate::MAX_OUTBOX
    }

    fn reply(&mut self, request_id: u64, body: HostResponseBody) -> Result<(), String> {
        if self.outbox.len() >= crate::MAX_OUTBOX {
            return Err("connection congestion: Host reliable output capacity exhausted".into());
        }
        let response = HostResponse {
            connection: self.id,
            request_id,
            body,
        };
        let bytes = host::encode_host_response(&response)
            .or_else(|error| {
                host::encode_host_response(&HostResponse {
                    connection: self.id,
                    request_id,
                    body: HostResponseBody::Error(format!("Host response unavailable: {error}")),
                })
            })
            .map_err(|error| error.to_string())?;
        self.outbox.push_back(bytes);
        Ok(())
    }
}

#[cfg(test)]
mod connection_tests;

mod service;
