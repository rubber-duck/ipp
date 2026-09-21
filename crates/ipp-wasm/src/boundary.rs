use ipp_protocol::MAX_MESSAGE_BYTES;

use crate::host::WasmHost;

pub(crate) struct WasmHostBoundary {
    last_session: u64,
    host: Option<WasmHost>,
    input: Vec<u8>,
    output: Vec<u8>,
}

impl WasmHostBoundary {
    pub(crate) const fn new() -> Self {
        Self {
            last_session: 0,
            host: None,
            input: Vec::new(),
            output: Vec::new(),
        }
    }

    pub(crate) fn open(&mut self, id: u64) -> bool {
        self.close();
        if id <= self.last_session {
            return self.fail("session ID must be nonzero and strictly increasing");
        }

        self.last_session = id;
        #[cfg(feature = "diagnostics")]
        crate::diagnostics::set_session(id);
        match WasmHost::new().and_then(|mut host| {
            host.open_connection(id)?;
            Ok(host)
        }) {
            Ok(session) => {
                self.host = Some(session);
                true
            }
            Err(error) => self.fail(&error),
        }
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
        self.host = None;
        self.input = Vec::new();
        self.release_output();
    }

    pub(crate) fn fail(&mut self, error: &str) -> bool {
        diagnostic!(
            Error,
            "[IPP wasm] session.failed session={} reason={}",
            self.last_session,
            error
        );
        self.close();
        let end = error.floor_char_boundary(MAX_MESSAGE_BYTES.min(error.len()));
        self.output = error.as_bytes()[..end].to_vec();
        false
    }

    pub(crate) fn reserve(&mut self, len: usize) -> *mut u8 {
        self.reserve_input(len, Some(MAX_MESSAGE_BYTES))
    }

    pub(crate) fn reserve_resource(&mut self, len: usize) -> *mut u8 {
        self.reserve_input(len, None)
    }

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

    pub(crate) fn receive(&mut self, len: usize) -> bool {
        self.release_output();
        if len == 0 || len != self.input.len() {
            return self.fail("receive length must equal a live reservation");
        }
        let Some(session) = self.host.as_mut() else {
            return self.fail("session is closed");
        };

        // Consume the reservation even on failure; decoded values own their data.
        let input = std::mem::take(&mut self.input);
        let result = session.receive_connection(self.last_session, &input);
        drop(input);

        match result {
            Ok(()) => true,
            Err(error) => self.fail(&error),
        }
    }

    pub(crate) fn accepts_input(&mut self) -> bool {
        self.host
            .as_mut()
            .is_some_and(|host| host.connection_accepts_input(self.last_session))
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

        match session.tick(dt) {
            Ok(()) => true,
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

    pub(crate) fn poll(&mut self) -> bool {
        self.release_output();
        let Some(output) = self
            .host
            .as_mut()
            .and_then(|host| host.take_connection_response(self.last_session))
        else {
            return false;
        };

        self.output = output;
        true
    }

    // ABI output is borrowed only until the next mutating call. The worker has
    // copied it before that call, so exclusive storage can return to the Host.
    fn release_output(&mut self) {
        let bytes = std::mem::take(&mut self.output);
        if let Some(host) = &mut self.host {
            host.recycle_response_buffer(bytes);
        }
    }

    pub(crate) fn output_ptr(&self) -> *const u8 {
        if self.output.is_empty() {
            std::ptr::null()
        } else {
            self.output.as_ptr()
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
        self.output.len()
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
        &self.output
    }
}
