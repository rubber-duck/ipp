use ipp_protocol::MAX_MESSAGE_BYTES;

use crate::connection_output::{
    CONNECTION_METADATA_BYTES, ConnectionOutput, Delivery, MAX_CONNECTIONS, MAX_DELIVERIES,
    MAX_FAILURE_BYTES,
};
use crate::host::WasmHost;
use std::collections::BTreeMap;

#[cfg(test)]
#[path = "boundary_output_tests.rs"]
mod output_tests;

pub(crate) struct WasmHostBoundary {
    last_session: u64,
    last_connection: u64,
    last_delivery: u64,
    connections: BTreeMap<u64, ConnectionOutput>,
    host: Option<WasmHost>,
    input: Vec<u8>,
    output: Vec<u8>,
    borrowed_delivery: Option<(u64, u64)>,
}

impl WasmHostBoundary {
    pub(crate) const fn new() -> Self {
        Self {
            last_session: 0,
            last_connection: 0,
            last_delivery: 0,
            connections: BTreeMap::new(),
            host: None,
            input: Vec::new(),
            output: Vec::new(),
            borrowed_delivery: None,
        }
    }

    pub(crate) fn open(&mut self, id: u64) -> bool {
        if self.host.is_some() || !self.connections.is_empty() {
            return self.diagnostic("Dispose the previous Host endpoints before reopening");
        }
        if id <= self.last_session {
            return self.fail("session ID must be nonzero and strictly increasing");
        }

        self.last_session = id;
        #[cfg(feature = "diagnostics")]
        crate::diagnostics::set_session(id);
        match WasmHost::new() {
            Ok(session) => {
                self.host = Some(session);
                true
            }
            Err(error) => self.fail(&error),
        }
    }

    pub(crate) fn connection_open(&mut self, id: u64) -> bool {
        self.release_output();
        if id == 0 || id <= self.last_connection || self.connections.len() >= MAX_CONNECTIONS {
            return self
                .diagnostic("connection identity is stale or connection capacity exhausted");
        }
        let Some(host) = self.host.as_mut() else {
            return self.diagnostic("Host is closed");
        };
        if let Err(error) = host.open_connection(id) {
            return self.diagnostic(&error);
        }
        let connection = host
            .reserve_connection_output_bytes(id, CONNECTION_METADATA_BYTES)
            .and_then(ConnectionOutput::new);
        match connection {
            Ok(connection) => {
                self.last_connection = id;
                self.connections.insert(id, connection);
                true
            }
            Err(error) => {
                host.close_connection(id);
                self.diagnostic(&error)
            }
        }
    }

    pub(crate) fn connection_close(&mut self, id: u64) -> bool {
        self.release_output();
        let Some(connection) = self.connections.get_mut(&id) else {
            return false;
        };
        connection.live = false;
        if let Some(host) = self.host.as_mut() {
            host.close_connection(id);
        }
        true
    }

    pub(crate) fn connection_dispose(&mut self, id: u64) -> bool {
        if !self.connection_close(id) {
            return false;
        }
        self.connections.remove(&id);
        true
    }

    pub(crate) fn connection_pending(&self, id: u64) -> usize {
        self.connections
            .get(&id)
            .map_or(0, |connection| connection.deliveries.len())
    }

    pub(crate) fn connection_failure(&mut self, id: u64) -> bool {
        self.release_output();
        let Some(error) = self
            .connections
            .get(&id)
            .and_then(|connection| connection.failure.as_deref())
        else {
            return false;
        };
        self.output.extend_from_slice(error.as_bytes());
        true
    }

    fn fail_connection(&mut self, id: u64, error: &str) -> bool {
        self.connection_close(id);
        if let Some(connection) = self.connections.get_mut(&id) {
            connection.fail(error);
        }
        self.diagnostic(error)
    }

    fn diagnostic(&mut self, error: &str) -> bool {
        self.release_output();
        let end = error.floor_char_boundary(MAX_FAILURE_BYTES.min(error.len()));
        self.output.extend_from_slice(&error.as_bytes()[..end]);
        false
    }

    pub(crate) fn set_identity_namespace(&mut self, namespace: u64) -> bool {
        match self
            .host
            .as_mut()
            .ok_or("Host is closed".to_owned())
            .and_then(|host| host.runtime_mut().set_identity_namespace(namespace))
        {
            Ok(()) => true,
            Err(error) => self.fail(&error),
        }
    }

    pub(crate) fn set_asset_cache_bytes(&mut self, bytes: u32) -> bool {
        let Some(host) = self.host.as_mut() else {
            return self.fail("Host is closed");
        };
        host.runtime_mut()
            .asset_resources_mut()
            .set_idle_resident_bytes_target(bytes as usize);
        true
    }

    #[cfg(test)]
    pub(crate) fn asset_cache_bytes(&self) -> Option<usize> {
        self.host.as_ref().map(|host| {
            host.runtime()
                .asset_resources()
                .idle_resident_bytes_target()
        })
    }

    pub(crate) fn close(&mut self) {
        if let Some(mut host) = self.host.take() {
            for &id in self.connections.keys() {
                host.close_connection(id);
            }
        }
        self.input = Vec::new();
        self.release_output();
        self.connections.clear();
    }

    pub(crate) fn fail(&mut self, error: &str) -> bool {
        diagnostic!(
            Error,
            "[IPP wasm] session.failed session={} reason={}",
            self.last_session,
            error
        );
        self.input = Vec::new();
        self.release_output();
        if let Some(mut host) = self.host.take() {
            for (&id, connection) in &mut self.connections {
                host.close_connection(id);
                connection.fail(error);
            }
        }
        self.diagnostic(error)
    }

    pub(crate) fn reserve(&mut self, len: usize) -> *mut u8 {
        self.reserve_input(len, Some(MAX_MESSAGE_BYTES))
    }

    pub(crate) fn reserve_resource(&mut self, len: usize) -> *mut u8 {
        self.reserve_input(len, None)
    }

    #[cfg(any(test, feature = "diagnostics"))]
    pub(crate) fn resource_buffered_bytes(&self) -> usize {
        self.host
            .as_ref()
            .map_or(0, |session| session.runtime().asset_input_bytes())
    }

    fn reserve_input(&mut self, len: usize, limit: Option<usize>) -> *mut u8 {
        // Invalidate old input/output before allocating. A reservation is replaced,
        // never extended, and cannot expose bytes left over from another call.
        self.input = Vec::new();
        self.release_output();
        if self.host.is_none() {
            self.fail("session is closed");
            return std::ptr::null_mut();
        }
        if len == 0 || limit.is_some_and(|limit| len > limit) {
            self.fail("input length is zero or exceeds the command frame limit");
            return std::ptr::null_mut();
        }
        if self.input.try_reserve_exact(len).is_err() {
            self.fail("input allocation failed");
            return std::ptr::null_mut();
        }

        self.input.resize(len, 0);
        self.input.as_mut_ptr()
    }

    pub(crate) fn receive(&mut self, connection: u64, len: usize) -> bool {
        self.release_output();
        if len == 0 || len != self.input.len() {
            return self
                .fail_connection(connection, "receive length must equal a live reservation");
        }
        let Some(session) = self.host.as_mut() else {
            return self.fail("session is closed");
        };

        // Consume the reservation even on failure; decoded values own their data.
        let input = std::mem::take(&mut self.input);
        let result = session.receive_connection(connection, &input);
        drop(input);

        match result {
            Ok(()) => true,
            Err(error) => self.fail_connection(connection, &error),
        }
    }

    pub(crate) fn accepts_input(&mut self, id: u64) -> bool {
        self.connections
            .get(&id)
            .is_some_and(|connection| connection.live)
            && self
                .host
                .as_mut()
                .is_some_and(|host| host.connection_accepts_input(id))
    }

    pub(crate) fn maintain_connections(&mut self, now: std::time::Duration) -> bool {
        let Some(host) = self.host.as_mut() else {
            return false;
        };
        host.maintain_connections(now);
        true
    }

    pub(crate) fn tick(&mut self, dt: f64) -> bool {
        self.release_output();
        let Some(session) = self.host.as_mut() else {
            return self.fail("session is closed");
        };

        match session.tick_worlds(dt) {
            Ok(failures) => {
                for (connection, error) in failures {
                    self.fail_connection(connection, &error);
                }
                true
            }
            Err(error) => self.fail(&error),
        }
    }

    pub(crate) fn progress_resources(&mut self) -> bool {
        self.release_output();
        let Some(session) = self.host.as_mut() else {
            return self.fail("session is closed");
        };

        match session.progress_resources() {
            Ok(()) => true,
            Err(error) => self.fail(&error),
        }
    }

    pub(crate) fn service_resources(&mut self) -> bool {
        self.release_output();
        let Some(session) = self.host.as_mut() else {
            return self.fail("session is closed");
        };

        match session.service_resources() {
            Ok(()) => true,
            Err(error) => self.fail(&error),
        }
    }

    pub(crate) fn poll(&mut self, id: u64) -> i32 {
        self.release_output();
        let Some(connection) = self.connections.get(&id) else {
            self.diagnostic("Connection is closed");
            return -1;
        };
        if !connection.live {
            if !self.connection_failure(id) {
                self.diagnostic("Connection is closing");
            }
            return -1;
        }
        if connection.deliveries.len() >= MAX_DELIVERIES {
            return 0;
        }
        let Some(output) = self
            .host
            .as_mut()
            .and_then(|host| host.take_connection_response(id))
        else {
            return 0;
        };
        let Some(delivery) = self.last_delivery.checked_add(1) else {
            self.fail_connection(id, "Output delivery identity exhausted");
            return -1;
        };
        let output = match output.prepare_copy() {
            Ok(output) => output,
            Err((output, _)) => {
                drop(output);
                self.fail_connection(
                    id,
                    "connection congestion: reliable output copy capacity exhausted",
                );
                return -1;
            }
        };
        self.last_delivery = delivery;
        self.connections
            .get_mut(&id)
            .unwrap()
            .deliveries
            .push_back(Delivery {
                id: delivery,
                output,
                copied: false,
            });
        self.borrowed_delivery = Some((id, delivery));
        1
    }

    fn release_output(&mut self) {
        self.output = Vec::new();
        if let Some((connection, delivery)) = self.borrowed_delivery.take()
            && let Some(record) = self
                .connections
                .get_mut(&connection)
                .and_then(|connection| connection.deliveries.back_mut())
        {
            assert_eq!(record.id, delivery);
            record.output.release_source();
            record.copied = true;
        }
    }

    pub(crate) fn output_delivery_id(&self) -> u64 {
        self.borrowed_delivery.map_or(0, |(_, delivery)| delivery)
    }

    pub(crate) fn output_copied(&mut self, connection: u64, delivery: u64) -> bool {
        if self.borrowed_delivery != Some((connection, delivery)) {
            return self.fail_connection(connection, "Copied output identity is stale");
        }
        self.release_output();
        true
    }

    pub(crate) fn delivery_complete(&mut self, connection: u64, delivery: u64) -> bool {
        let Some(front) = self
            .connections
            .get(&connection)
            .and_then(|connection| connection.deliveries.front())
        else {
            return self.fail_connection(connection, "Unexpected output acknowledgement");
        };
        if front.id != delivery || !front.copied {
            return self.fail_connection(
                connection,
                "Output acknowledgement is stale or out of order",
            );
        }
        self.connections
            .get_mut(&connection)
            .unwrap()
            .deliveries
            .pop_front();
        true
    }

    fn output_bytes(&self) -> &[u8] {
        if let Some((connection, delivery)) = self.borrowed_delivery {
            let record = self.connections[&connection].deliveries.back().unwrap();
            assert_eq!(record.id, delivery);
            record.output.bytes()
        } else {
            &self.output
        }
    }

    pub(crate) fn output_ptr(&self) -> *const u8 {
        if self.output_bytes().is_empty() {
            std::ptr::null()
        } else {
            self.output_bytes().as_ptr()
        }
    }

    pub(crate) fn resource_poll(&mut self) -> bool {
        self.release_output();
        let Some(bytes) = self.host.as_mut().and_then(WasmHost::take_resource_request) else {
            return false;
        };
        self.output = bytes;
        true
    }

    pub(crate) fn resource_complete(
        &mut self,
        expected_session: u64,
        id: u64,
        success: u32,
        len: usize,
    ) -> bool {
        self.release_output();
        if id == 0 || success > 1 || len == 0 || len != self.input.len() {
            return self.fail("invalid resource completion or reservation");
        }
        let input = std::mem::take(&mut self.input);
        if expected_session != self.last_session {
            // A previous producer's ticket may be reused by a fresh world.
            // Discard its owned bytes without touching the replacement session.
            return true;
        }
        let result = if success == 1 {
            Ok(input)
        } else {
            Err(String::from_utf8_lossy(&input[..input.len().min(1024)]).into_owned())
        };
        let Some(session) = self.host.as_mut() else {
            return self.fail("session is closed");
        };
        ipp_host_session::deliver_resource(session.runtime_mut(), id, result);
        true
    }

    pub(crate) fn asset_chunk(&mut self, expected_session: u64, id: u64, len: usize) -> u32 {
        self.release_output();
        if id == 0
            || len == 0
            || len > ipp_core::services::asset_management::STREAM_CAPACITY
            || len != self.input.len()
        {
            self.fail("invalid asset chunk or reservation");
            return 0;
        }
        if expected_session != self.last_session {
            self.input.clear();
            return 1;
        }
        let Some(session) = &self.host else {
            self.fail("session is closed");
            return 0;
        };
        match session.runtime().asset_input_chunk(id, &self.input) {
            Ok(true) => {
                self.input.clear();
                1
            }
            Ok(false) => 2,
            Err(error) => {
                self.fail(&error);
                0
            }
        }
    }

    pub(crate) fn asset_end(
        &mut self,
        expected_session: u64,
        id: u64,
        success: u32,
        len: usize,
    ) -> u32 {
        self.release_output();
        if id == 0
            || success > 1
            || (success == 0 && (len == 0 || len > 1024 || len != self.input.len()))
            || (success == 1 && len != 0)
        {
            self.fail("invalid asset stream end");
            return 0;
        }
        let input = std::mem::take(&mut self.input);
        if expected_session != self.last_session {
            return 1;
        }
        let Some(session) = &self.host else {
            self.fail("session is closed");
            return 0;
        };
        session.runtime().asset_input_end(
            id,
            if success == 1 {
                Ok(())
            } else {
                Err(String::from_utf8_lossy(&input).into_owned())
            },
        );
        1
    }

    pub(crate) fn output_len(&self) -> usize {
        self.output_bytes().len()
    }

    #[cfg(all(feature = "render", target_arch = "wasm32"))]
    pub(crate) fn presentation(
        &mut self,
    ) -> Option<&mut crate::services::render::RenderSurfaceService> {
        self.host
            .as_mut()
            .map(|session| &mut session.services_mut().presentation)
    }

    #[cfg(all(feature = "render", target_arch = "wasm32"))]
    pub(crate) fn presentation_host(
        &mut self,
    ) -> Option<(
        &mut crate::services::render::RenderSurfaceService,
        &mut ipp_core::HostRuntime,
    )> {
        self.host.as_mut().map(|session| {
            let (runtime, platform) = session.parts_mut();
            (&mut platform.presentation, runtime)
        })
    }

    #[cfg(test)]
    pub(crate) fn input_mut(&mut self) -> &mut [u8] {
        &mut self.input
    }

    #[cfg(test)]
    pub(crate) fn output(&self) -> &[u8] {
        self.output_bytes()
    }
}
