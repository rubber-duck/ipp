use ipp_protocol::MAX_MESSAGE_BYTES;

#[test]
fn provider_reservations_do_not_inherit_the_command_frame_limit() {
    let mut boundary = SynchronousHostDriver::new();
    assert!(boundary.open(1));
    let rgba_payload = 16 + 3072 * 2048 * 4;
    assert!(!boundary.reserve_resource(rgba_payload).is_null());
    assert_eq!(boundary.input_mut().len(), rgba_payload);
    assert!(boundary.input_mut().iter().all(|&byte| byte == 0));

    // A new reservation invalidates the old one, including across ingress types.
    assert!(boundary.reserve(MAX_MESSAGE_BYTES + 1).is_null());
    assert!(boundary.input_mut().is_empty());
    assert!(boundary.open(2));
    assert!(boundary.reserve_resource(usize::MAX).is_null());
    assert!(boundary.input_mut().is_empty());
}

type WasmHostBoundary = SynchronousHostDriver;

struct SynchronousHostDriver {
    boundary: crate::boundary::WasmHostBoundary,
    connection: u64,
    delivery: Option<u64>,
}

impl SynchronousHostDriver {
    fn new() -> Self {
        Self {
            boundary: crate::boundary::WasmHostBoundary::new(),
            connection: 0,
            delivery: None,
        }
    }

    fn complete(&mut self) {
        if let Some(delivery) = self.delivery.take() {
            if self.boundary.output_delivery_id() == delivery {
                assert!(self.boundary.output_copied(self.connection, delivery));
            }
            assert!(self.boundary.delivery_complete(self.connection, delivery));
        }
    }

    fn open(&mut self, id: u64) -> bool {
        self.close();
        self.connection = id;
        self.boundary.open(id) && self.boundary.connection_open(id)
    }

    fn close(&mut self) {
        self.delivery = None;
        self.boundary.close();
    }

    fn reserve(&mut self, len: usize) -> *mut u8 {
        self.complete();
        self.boundary.reserve(len)
    }

    fn receive(&mut self, len: usize) -> bool {
        self.complete();
        self.boundary.receive(self.connection, len)
    }

    fn poll(&mut self) -> bool {
        self.complete();
        let result = self.boundary.poll(self.connection);
        if result == 1 {
            self.delivery = Some(self.boundary.output_delivery_id());
        }
        result == 1
    }

    fn tick(&mut self, dt: f64) -> bool {
        self.complete();
        self.boundary.tick(dt)
    }

    fn accepts_input(&mut self) -> bool {
        self.boundary.accepts_input(self.connection)
    }
}

impl std::ops::Deref for SynchronousHostDriver {
    type Target = crate::boundary::WasmHostBoundary;

    fn deref(&self) -> &Self::Target {
        &self.boundary
    }
}

impl std::ops::DerefMut for SynchronousHostDriver {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.boundary
    }
}

#[test]
fn browser_host_inherits_the_default_idle_asset_cache_and_accepts_an_override() {
    let mut boundary = WasmHostBoundary::new();
    assert!(!boundary.set_asset_cache_bytes(64));

    assert!(boundary.open(1));
    assert_eq!(
        boundary.asset_cache_bytes(),
        Some(ipp_core::services::asset_management::AssetManagementService::DEFAULT_IDLE_RESIDENT_BYTES_TARGET)
    );
    assert_ne!(boundary.asset_cache_bytes(), Some(0));
    assert!(boundary.set_asset_cache_bytes(0));
    assert_eq!(boundary.asset_cache_bytes(), Some(0));
    assert!(boundary.set_asset_cache_bytes(u32::MAX));
    assert_eq!(boundary.asset_cache_bytes(), Some(u32::MAX as usize));
}

/// Submit one final-page batch of encoded commands and drain its replies.
fn submit(boundary: &mut WasmHostBoundary, batch_id: u32, commands: &[Vec<u8>]) {
    let mut bytes = request(1, 1);
    bytes.extend_from_slice(&batch_id.to_le_bytes());
    bytes.push(1); // Final page.
    bytes.extend_from_slice(&(commands.len() as u32).to_le_bytes());
    for command in commands {
        bytes.extend_from_slice(command);
    }
    assert!(send(boundary, &bytes));
    assert!(boundary.tick(0.0));
    assert!(boundary.poll());
    assert_eq!(boundary.output()[24], 1);
    assert_eq!(boundary.output()[41], 0); // Batch committed.
    while boundary.poll() {}
}

/// Encode a reference to the entity with this symbolic ID.
fn symbol_ref(symbol: &str) -> Vec<u8> {
    let mut bytes = vec![3]; // Symbolic reference.
    bytes.extend_from_slice(&(symbol.len() as u32).to_le_bytes());
    bytes.extend_from_slice(symbol.as_bytes());
    bytes
}

/// Insert an UnlitTexture selecting `source` on the entity with this symbolic ID.
fn insert_texture(symbol: &str, source: &str) -> Vec<u8> {
    let mut bytes = vec![4]; // Insert component.
    bytes.extend_from_slice(&symbol_ref(symbol));
    bytes.extend_from_slice(&ipp_core::ComponentValue::UNLIT_TEXTURE.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(
        &(std::mem::offset_of!(ipp_core::components::UnlitTexture, source) as u32).to_le_bytes(),
    );
    bytes.push(ipp_core::components::schema::FieldKind::String as u8);
    bytes.extend_from_slice(&(source.len() as u32).to_le_bytes());
    bytes.extend_from_slice(source.as_bytes());
    bytes.push(0); // Not adopting.
    bytes
}

/// Run Host frames until the browser provider is asked for `source`, or none is asked
/// within a few frames. Returns the initial-acquisition tickets.
fn acquisitions(boundary: &mut WasmHostBoundary, source: &str) -> Vec<u64> {
    let mut tickets = Vec::new();
    for _ in 0..4 {
        while boundary.resource_poll() {
            let bytes = boundary.output();
            if bytes[0] == 1 && &bytes[13..] == source.as_bytes() {
                tickets.push(u64::from_le_bytes(bytes[1..9].try_into().unwrap()));
            }
        }
        assert!(boundary.tick(0.0));
        while boundary.poll() {}
    }
    tickets
}

/// Use, release and reuse one HTTP texture through the browser Host boundary.
/// Returns how many acquisitions the provider saw for it.
fn texture_acquisitions_across_release(cache_bytes: Option<u32>) -> usize {
    let source = "https://example.test/reused.ippt";
    let mut boundary = WasmHostBoundary::new();
    connect(&mut boundary, 1);
    if let Some(bytes) = cache_bytes {
        assert!(boundary.set_asset_cache_bytes(bytes));
    }

    let mut create = vec![1]; // Create.
    create.extend_from_slice(&0u32.to_le_bytes());
    create.push(1); // Symbolic ID.
    create.extend_from_slice(&7u32.to_le_bytes());
    create.extend_from_slice(b"surface");
    create.extend_from_slice(&0u32.to_le_bytes()); // No classes.
    create.push(0); // Not adopting.
    submit(
        &mut boundary,
        1,
        &[create, insert_texture("surface", source)],
    );

    let tickets = acquisitions(&mut boundary, source);
    assert_eq!(tickets.len(), 1);
    let length = 16 + 4 * 4 * 4;
    assert!(!boundary.reserve_resource(length).is_null());
    let input = boundary.input_mut();
    input[..4].copy_from_slice(b"IPPT");
    input[4..8].copy_from_slice(&3u32.to_le_bytes());
    input[8..12].copy_from_slice(&4u32.to_le_bytes());
    input[12..16].copy_from_slice(&4u32.to_le_bytes());
    assert!(boundary.resource_complete(1, tickets[0], 1, length));
    let mut loaded = false;
    for _ in 0..64 {
        assert!(boundary.tick(0.0));
        while boundary.poll() {
            loaded |= contains_status(
                boundary.output(),
                source,
                ipp_core::AssetResourceStatus::Loaded,
            );
        }
        if loaded {
            break;
        }
    }
    assert!(loaded);

    // Removing the only consumer leaves the completed texture unused.
    let mut remove = vec![6]; // Remove component.
    remove.extend_from_slice(&symbol_ref("surface"));
    remove.extend_from_slice(&ipp_core::ComponentValue::UNLIT_TEXTURE.to_le_bytes());
    submit(&mut boundary, 2, &[remove]);
    assert!(acquisitions(&mut boundary, source).is_empty());

    submit(&mut boundary, 3, &[insert_texture("surface", source)]);
    tickets.len() + acquisitions(&mut boundary, source).len()
}

#[test]
fn released_texture_is_reused_without_another_acquisition_under_the_default_cache() {
    assert_eq!(texture_acquisitions_across_release(None), 1);

    // An explicit zero target evicts on release, so returning demand acquires again.
    assert_eq!(texture_acquisitions_across_release(Some(0)), 2);
}

fn send(boundary: &mut WasmHostBoundary, bytes: &[u8]) -> bool {
    assert!(!boundary.reserve(bytes.len()).is_null());
    boundary.input_mut().copy_from_slice(bytes);
    boundary.receive(bytes.len())
}

thread_local! {
    static WORLD_SESSIONS: std::cell::RefCell<std::collections::BTreeMap<u64, u64>> = const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
}

fn world_session(connection: u64) -> u64 {
    WORLD_SESSIONS.with_borrow(|sessions| sessions.get(&connection).copied().unwrap_or(connection))
}

fn attach(boundary: &mut WasmHostBoundary, connection: u64) -> u64 {
    let request = ipp_protocol::host::encode_host_request(&ipp_protocol::host::HostRequest {
        connection,
        request_id: 1,
        body: ipp_protocol::host::HostRequestBody::CreateWorld {
            // Sessions author textured meshes and nothing else.
            options: ipp_protocol::host::WorldCreateOptions::new(
                [
                    "ipp.animation",
                    "ipp.asset-dependencies",
                    "ipp.hierarchy",
                    "ipp.look-at",
                    "ipp.final-propagation",
                    "ipp.geometry",
                    "ipp.render",
                ]
                .map(String::from)
                .to_vec(),
            ),
            temporary: true,
        },
    })
    .unwrap();
    assert!(send(boundary, &request));
    assert!(boundary.tick(0.0));
    assert!(boundary.poll());
    let response = ipp_protocol::host::decode_host_response(boundary.output(), connection).unwrap();
    let ipp_protocol::host::HostResponseBody::Created {
        reference,
        ..
    } = response.body
    else {
        panic!("World not created")
    };
    let request = ipp_protocol::host::encode_host_request(&ipp_protocol::host::HostRequest {
        connection,
        request_id: 2,
        body: ipp_protocol::host::HostRequestBody::OpenWorld(reference),
    })
    .unwrap();
    assert!(send(boundary, &request));
    assert!(boundary.tick(0.0));
    assert!(boundary.poll());
    let response = ipp_protocol::host::decode_host_response(boundary.output(), connection).unwrap();
    let ipp_protocol::host::HostResponseBody::Attached {
        session,
        ..
    } = response.body
    else {
        panic!("World not attached")
    };
    WORLD_SESSIONS.with_borrow_mut(|sessions| {
        sessions.insert(connection, session);
    });
    let mut tick = None;
    while boundary.poll() {
        if boundary.output()[24] == 4 {
            tick = Some(u64::from_le_bytes(
                boundary.output()[16..24].try_into().unwrap(),
            ));
        }
    }
    tick.expect("attached World completed frame")
}

fn connect(boundary: &mut WasmHostBoundary, id: u64) -> u64 {
    assert!(boundary.open(id));
    assert!(send(boundary, &ipp_protocol::HELLO));
    assert_eq!(boundary.output_len(), 0);
    assert!(boundary.poll());
    assert_eq!(
        boundary.output(),
        ipp_protocol::accept_hello(&ipp_protocol::HELLO, id).unwrap()
    );
    assert!(!boundary.poll());
    assert!(boundary.output_ptr().is_null());
    attach(boundary, id)
}

fn request(session: u64, tag: u8) -> Vec<u8> {
    let mut bytes = world_session(session).to_le_bytes().to_vec();
    bytes.extend_from_slice(&9u64.to_le_bytes());
    bytes.push(tag);
    if tag == 3 {
        bytes.push(1);
        bytes.extend_from_slice(&[0; 16]);
        bytes.extend_from_slice(&256u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
    }
    bytes
}

fn batch(session: u64, alias: u32, class: &[u8]) -> Vec<u8> {
    let mut bytes = request(session, 1);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    bytes.push(1); // Final page.
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1); // Create.
    bytes.extend_from_slice(&alias.to_le_bytes());
    bytes.push(0); // No symbolic ID.
    bytes.extend_from_slice(&u32::from(!class.is_empty()).to_le_bytes());
    if !class.is_empty() {
        bytes.extend_from_slice(&(class.len() as u32).to_le_bytes());
        bytes.extend_from_slice(class);
    }
    bytes.push(0); // Not adopting.
    bytes
}

fn transact(boundary: &mut WasmHostBoundary, bytes: &[u8]) -> Vec<u8> {
    assert!(send(boundary, bytes));
    assert!(!boundary.poll());
    assert!(boundary.tick(0.0));
    assert!(boundary.poll());
    let response = boundary.output().to_vec();
    assert!(boundary.poll());
    assert_eq!(&boundary.output()[8..16], &0u64.to_le_bytes());
    assert!(!boundary.poll());
    response
}

#[test]
fn reservation_bounds_exact_lengths_and_single_consumption() {
    let mut boundary = WasmHostBoundary::new();
    assert!(boundary.reserve(16).is_null());

    for (index, len) in [0, MAX_MESSAGE_BYTES + 1, usize::MAX]
        .into_iter()
        .enumerate()
    {
        assert!(boundary.open(index as u64 + 1));
        assert!(boundary.reserve(len).is_null());
        assert!(boundary.reserve(16).is_null());
        assert!(std::str::from_utf8(boundary.output()).is_ok());
    }

    let mut boundary = WasmHostBoundary::new();
    connect(&mut boundary, 1);
    assert!(!boundary.reserve(MAX_MESSAGE_BYTES).is_null());
    assert!(boundary.input_mut().iter().all(|byte| *byte == 0));
    boundary.input_mut().fill(0xff);
    assert!(!boundary.reserve(16).is_null());
    assert_eq!(boundary.input_mut(), [0; 16]);
    assert_eq!(boundary.output_len(), 0);
    assert!(!boundary.receive(15));
    assert!(!boundary.accepts_input());
    assert!(boundary.tick(0.0));

    connect(&mut boundary, 2);
    assert!(!boundary.receive(16)); // The hello's reservation was already consumed.
    assert!(!boundary.accepts_input());
    assert!(boundary.tick(0.0));
}

#[test]
fn missing_hello_and_invalid_wire_fail_closed() {
    let mut boundary = WasmHostBoundary::new();
    assert!(boundary.open(1));
    assert!(boundary.tick(0.1)); // No frame publication before the hello.
    assert!(!boundary.poll());
    assert!(!send(&mut boundary, &request(1, 3)));
    assert!(!boundary.accepts_input());
    assert!(boundary.tick(0.0));

    assert!(boundary.open(2));
    // A schema claim is not this protocol's hello, whatever hash it names.
    assert!(!send(&mut boundary, &ipp_protocol::announcement()));
    assert!(
        std::str::from_utf8(boundary.output())
            .unwrap()
            .contains("hello length")
    );

    connect(&mut boundary, 3);
    let mut trailing = request(3, 3);
    trailing.push(0);
    assert!(!send(&mut boundary, &trailing));
    assert!(!boundary.accepts_input());
    assert!(boundary.tick(0.0));

    connect(&mut boundary, 4);
    let mut old_step = request(4, 2);
    old_step.extend_from_slice(&0.25f64.to_le_bytes());
    assert!(!send(&mut boundary, &old_step));
    assert!(!boundary.accepts_input());
    assert!(boundary.tick(0.0));

    connect(&mut boundary, 5);
    let mut reserved_id = request(5, 3);
    reserved_id[8..16].fill(0);
    assert!(!send(&mut boundary, &reserved_id));
    assert!(!boundary.accepts_input());
    assert!(boundary.tick(0.0));
}

#[test]
fn any_connection_pulls_the_contract_after_its_hello() {
    let mut boundary = WasmHostBoundary::new();
    assert!(boundary.open(1));
    assert!(!send(&mut boundary, &ipp_protocol::CONTRACT_REQUEST));
    assert!(
        std::str::from_utf8(boundary.output())
            .unwrap()
            .contains("hello magic")
    );

    assert!(boundary.open(2));
    assert!(send(&mut boundary, &ipp_protocol::HELLO));
    assert!(boundary.poll());
    assert_eq!(&boundary.output()[..16], &ipp_protocol::announcement());
    for _ in 0..2 {
        assert!(send(&mut boundary, &ipp_protocol::CONTRACT_REQUEST));
        assert!(boundary.poll());
        assert_eq!(boundary.output(), ipp_protocol::contract_reply());
    }
    assert!(!boundary.poll());

    // The connection stays usable for ordinary Host and World requests.
    attach(&mut boundary, 2);
}

#[test]
fn replacement_clears_world_pending_output_and_buffers() {
    let mut boundary = WasmHostBoundary::new();
    connect(&mut boundary, 1);
    assert_eq!(transact(&mut boundary, &batch(1, 1, b"retained"))[41], 0);
    assert!(send(&mut boundary, &batch(1, 2, b"pending")));
    assert!(boundary.tick(0.0)); // Leave old replies queued across replacement.
    assert!(send(&mut boundary, &batch(1, 3, b"never applied")));
    assert!(!boundary.reserve(16).is_null());
    boundary.input_mut().fill(0xff);
    assert!(boundary.open(2));
    assert_eq!(boundary.output_len(), 0);
    assert!(boundary.input_mut().is_empty());
    assert!(!boundary.poll());
    assert!(send(&mut boundary, &ipp_protocol::HELLO));
    assert!(boundary.poll());
    assert!(!boundary.poll());
    let baseline = attach(&mut boundary, 2);
    let inspection = transact(&mut boundary, &request(2, 3));
    assert_eq!(&inspection[16..24], &(baseline + 1).to_le_bytes());
    assert_eq!(&inspection[41..45], &0u32.to_le_bytes());

    assert!(!send(&mut boundary, &request(1, 3)));
    assert!(
        std::str::from_utf8(boundary.output())
            .unwrap()
            .contains("SessionMismatch")
    );
    assert!(!boundary.open(2));
    connect(&mut boundary, 3);
    boundary.close();
    assert!(boundary.output_ptr().is_null());
    assert_eq!(boundary.output_len(), 0);
    assert!(!boundary.open(1));
    assert!(!boundary.open(0));
    connect(&mut boundary, u64::MAX);
    boundary.close();
    assert!(!boundary.open(u64::MAX));
}

#[test]
fn queued_requests_publish_after_tick_and_idle_ticks_publish_frames() {
    let mut boundary = WasmHostBoundary::new();
    let baseline = connect(&mut boundary, 1);
    assert!(send(&mut boundary, &batch(1, 1, b"retained")));
    assert!(send(&mut boundary, &request(1, 3)));
    assert!(!boundary.poll());
    assert!(boundary.tick(0.25));
    assert!(boundary.poll());
    assert_eq!(boundary.output()[24], 1);
    assert_eq!(boundary.output()[41], 0);
    assert_eq!(&boundary.output()[16..24], &(baseline + 1).to_le_bytes());

    assert!(boundary.poll());
    assert_eq!(&boundary.output()[25..33], &0.25f64.to_le_bytes());
    assert_eq!(&boundary.output()[41..45], &1u32.to_le_bytes());
    assert!(
        boundary
            .output()
            .windows(8)
            .any(|bytes| bytes == b"retained")
    );
    assert!(boundary.poll());
    let frame_tag = boundary.output()[24];
    assert_eq!(&boundary.output()[8..16], &0u64.to_le_bytes());
    assert_eq!(&boundary.output()[16..24], &(baseline + 1).to_le_bytes());
    assert_eq!(&boundary.output()[25..33], &0.25f64.to_le_bytes());
    assert!(!boundary.poll());
    assert!(boundary.output_ptr().is_null());

    assert!(boundary.tick(0.5));
    assert!(boundary.poll());
    assert_eq!(boundary.output()[24], frame_tag);
    assert_eq!(&boundary.output()[8..16], &0u64.to_le_bytes());
    assert_eq!(&boundary.output()[16..24], &(baseline + 2).to_le_bytes());
    assert_eq!(&boundary.output()[25..33], &0.75f64.to_le_bytes());
    assert!(!boundary.poll());
}

#[test]
fn oversized_observation_pages_records_and_keeps_session() {
    let mut boundary = WasmHostBoundary::new();
    connect(&mut boundary, 1);

    // Fit retained metadata, but exceed 1MiB with serialized entity headers.
    let class = vec![b'x'; 65_000];
    for alias in 0..16 {
        assert_eq!(transact(&mut boundary, &batch(1, alias, &class))[41], 0);
    }
    assert_eq!(transact(&mut boundary, &batch(1, 16, &[b'y'; 5000]))[41], 0);
    for alias in 17..217 {
        assert_eq!(transact(&mut boundary, &batch(1, alias, b""))[41], 0);
    }

    let response = transact(&mut boundary, &request(1, 3));
    assert!(response.len() < MAX_MESSAGE_BYTES);
    assert_eq!(response[24], 3);
    let next = u64::from_le_bytes(response[33..41].try_into().unwrap());
    let count = u32::from_le_bytes(response[41..45].try_into().unwrap());
    assert!(next != 0 && count > 0 && count < 217);
    let mut continuation = request(1, 3);
    continuation[18..26].copy_from_slice(&next.to_le_bytes());
    let second = transact(&mut boundary, &continuation);
    assert_eq!(second[24], 3);
    assert!(second.len() < MAX_MESSAGE_BYTES);
    assert_eq!(&second[33..41], &0u64.to_le_bytes());
    assert_eq!(
        count + u32::from_le_bytes(second[41..45].try_into().unwrap()),
        217
    );
    assert!(boundary.tick(0.0));
    assert!(boundary.poll());
    assert!(!boundary.poll());
}

#[test]
fn semantic_batch_failure_keeps_partial_creation_and_the_session() {
    let mut boundary = WasmHostBoundary::new();
    connect(&mut boundary, 1);
    let mut duplicate = batch(1, 1, b"duplicate");
    duplicate[22..26].copy_from_slice(&2u32.to_le_bytes());
    let second_create = duplicate[26..].to_vec();
    duplicate.extend_from_slice(&second_create);
    let response = transact(&mut boundary, &duplicate);
    assert_eq!(response[24], 1);
    assert_eq!(response[41], 1);

    let inspection = transact(&mut boundary, &request(1, 3));
    assert_eq!(&inspection[41..45], &1u32.to_le_bytes());
    assert_eq!(transact(&mut boundary, &batch(1, 1, b"valid"))[41], 0);
}

#[test]
fn invalid_host_time_and_output_backpressure_fail_closed() {
    let mut boundary = WasmHostBoundary::new();
    for (index, dt) in [f64::NAN, f64::INFINITY, -0.1].into_iter().enumerate() {
        connect(&mut boundary, index as u64 + 1);
        assert!(!boundary.tick(dt));
        assert!(std::str::from_utf8(boundary.output()).is_ok());
        assert!(!boundary.poll());
        assert!(boundary.reserve(16).is_null());
    }

    connect(&mut boundary, 4);
    for _ in 0..200 {
        assert!(boundary.tick(0.0));
        assert!(!boundary.connection_failure(4));
    }
    assert!(boundary.poll());
    assert_eq!(boundary.output()[24], 4);
    assert!(!boundary.poll());
    for request_id in 1u64..=85 {
        let mut query = request(4, 3);
        query[8..16].copy_from_slice(&request_id.to_le_bytes());
        if !send(&mut boundary, &query) {
            break;
        }
        assert!(boundary.tick(0.0));
    }
    assert!(boundary.connection_failure(4));
    assert!(
        std::str::from_utf8(boundary.output())
            .unwrap()
            .contains("congestion")
    );
    assert!(!boundary.poll());
    assert!(!boundary.accepts_input());
    assert!(boundary.tick(0.0));

    // Exactly the admitted-request count is accepted; the next request is refused.
    connect(&mut boundary, 5);
    for _ in 0..64 {
        assert!(send(&mut boundary, &request(5, 3)));
    }
    assert!(!send(&mut boundary, &request(5, 3)));
    assert!(
        std::str::from_utf8(boundary.output())
            .unwrap()
            .contains("congestion")
    );
    assert!(!boundary.poll());
    assert!(!boundary.accepts_input());
    assert!(boundary.tick(0.0));
}

#[test]
fn deferred_progress_survives_wasm_copy_until_exact_physical_acknowledgement() {
    let mut driver = WasmHostBoundary::new();
    connect(&mut driver, 1);
    while driver.poll() {}
    assert!(driver.tick(0.1));
    assert!(driver.poll());
    let first = driver.output().to_vec();
    let delivery = driver.output_delivery_id();
    assert!(driver.output_copied(1, delivery));
    for _ in 0..200 {
        assert!(driver.boundary.tick(0.1));
        assert_eq!(driver.boundary.poll(1), 0);
        assert!(!driver.connection_failure(1));
        assert_eq!(driver.connection_pending(1), 1);
    }
    assert!(driver.delivery_complete(1, delivery));
    driver.delivery = None;
    assert_eq!(driver.boundary.poll(1), 1);
    let next = driver.output().to_vec();
    assert_eq!(next[24], 4);
    assert!(
        u64::from_le_bytes(next[16..24].try_into().unwrap())
            > u64::from_le_bytes(first[16..24].try_into().unwrap())
    );
    let delivery = driver.output_delivery_id();
    assert!(driver.output_copied(1, delivery));
    assert!(driver.delivery_complete(1, delivery));
    assert_eq!(driver.boundary.poll(1), 0);
}

#[test]
fn provider_completion_consumes_owned_bytes_and_fences_previous_session() {
    let mut boundary = WasmHostBoundary::new();
    connect(&mut boundary, 1);
    assert!(!boundary.resource_poll());
    assert!(!boundary.reserve(5).is_null());
    boundary.input_mut().copy_from_slice(b"error");
    assert!(boundary.resource_complete(1, 123, 0, 5)); // Obsolete ticket is harmless.
    assert!(boundary.tick(0.0));
    while boundary.poll() {}

    connect(&mut boundary, 2);
    assert!(!boundary.reserve(5).is_null());
    boundary.input_mut().copy_from_slice(b"error");
    assert!(boundary.resource_complete(1, 123, 0, 5));
    assert!(boundary.tick(0.0)); // Stale producer cannot stop the replacement.
    while boundary.poll() {}
    assert!(!boundary.resource_complete(2, 123, 0, 5)); // RenderMemoryReservation is single use.
    assert!(boundary.reserve(5).is_null());
}

/// Whether one Host output page reports `source` with `status` and usable decoded data.
fn contains_status(bytes: &[u8], source: &str, status: ipp_core::AssetResourceStatus) -> bool {
    use ipp_core::{AssetResourceKind, AssetResourceSnapshot};
    use ipp_protocol::{Response, ResponseBody};

    if bytes[24] != 9 {
        return false;
    }

    // Match a complete typed source/status record without assuming the
    // manager-assigned resource identity or its position among events.
    let expected = ipp_protocol::encode_response(&Response {
        session: 1,
        request_id: 0,
        tick: 1,
        body: ResponseBody::Resources {
            resources: vec![AssetResourceSnapshot {
                representation: Default::default(),
                id: 1,
                kind: AssetResourceKind::Texture,
                source: source.into(),
                variant: 0,
                status,
            }],
        },
    })
    .unwrap();
    // Exclude identity and the variable representation observation. Loaded
    // events must additionally advertise usable decoded data.
    let record = &expected[37..expected.len() - 19];
    bytes[29..]
        .windows(record.len() + 1)
        .any(|part| &part[..record.len()] == record && part[record.len()] == 1)
}

#[test]
fn provider_completions_above_former_aggregate_budget_both_load() {
    use ipp_core::{AssetResourceStatus, components::UnlitTexture};

    let mut boundary = WasmHostBoundary::new();
    connect(&mut boundary, 1);
    let sources = [
        "https://example.test/first.ippt",
        "https://example.test/second.ippt",
    ];
    let mut commands = request(1, 1);
    commands.extend_from_slice(&7u32.to_le_bytes());
    commands.push(1); // Final page.
    commands.extend_from_slice(&4u32.to_le_bytes());
    for (alias, source) in sources.iter().enumerate() {
        commands.extend_from_slice(&batch(1, alias as u32, b"")[26..]);
        commands.extend_from_slice(&[4, 1]); // Insert component on an alias.
        commands.extend_from_slice(&(alias as u32).to_le_bytes());
        commands.extend_from_slice(&ipp_core::ComponentValue::UNLIT_TEXTURE.to_le_bytes());
        commands.extend_from_slice(&1u32.to_le_bytes());
        commands
            .extend_from_slice(&(std::mem::offset_of!(UnlitTexture, source) as u32).to_le_bytes());
        commands.push(ipp_core::components::schema::FieldKind::String as u8);
        commands.extend_from_slice(&(source.len() as u32).to_le_bytes());
        commands.extend_from_slice(source.as_bytes());
        commands.push(0); // Not adopting.
    }
    assert!(send(&mut boundary, &commands));
    assert!(boundary.tick(0.0));
    assert!(boundary.poll());
    assert_eq!(boundary.output()[24], 1);
    assert_eq!(boundary.output()[41], 0); // Declaration batch committed.
    while boundary.poll() {}

    let mut tickets = Vec::new();
    // Committed demand starts at the next shared service phase; the provider
    // observes those newly issued requests at the following Host boundary.
    for _ in 0..3 {
        while boundary.resource_poll() {
            let bytes = boundary.output();
            assert_eq!(bytes[0], 1); // Initial acquisition, not recovery/cancellation.
            tickets.push((
                u64::from_le_bytes(bytes[1..9].try_into().unwrap()),
                std::str::from_utf8(&bytes[13..]).unwrap().to_owned(),
            ));
        }
        if tickets.len() == 2 {
            break;
        }
        assert!(boundary.tick(0.0));
        while boundary.poll() {}
    }
    assert_eq!(tickets.len(), 2);
    tickets.sort_by(|a, b| a.1.cmp(&b.1));
    assert_eq!([tickets[0].1.as_str(), tickets[1].1.as_str()], sources);

    // Complete both owned buffers before the next Host frame. Their aggregate
    // staging and residency exceed the former 16 MiB quota.
    let length = 16 + 2048 * 2048 * 4;
    assert!(length * 2 > 16 << 20);
    for (id, _) in &tickets {
        assert!(!boundary.reserve_resource(length).is_null());
        let input = boundary.input_mut();
        input[..4].copy_from_slice(b"IPPT");
        input[4..8].copy_from_slice(&3u32.to_le_bytes());
        input[8..12].copy_from_slice(&2048u32.to_le_bytes());
        input[12..16].copy_from_slice(&2048u32.to_le_bytes());
        assert!(boundary.resource_complete(1, *id, 1, length));
        assert!(boundary.input_mut().is_empty());
        assert!(!boundary.poll());
    }

    let mut loaded = [false; 2];
    for _ in 0..512 {
        assert!(boundary.tick(0.0));
        while boundary.poll() {
            for (index, source) in sources.iter().enumerate() {
                loaded[index] |=
                    contains_status(boundary.output(), source, AssetResourceStatus::Loaded);
            }
        }
        assert!(
            !boundary.resource_poll(),
            "successful completions must not cancel providers"
        );
        if loaded.iter().all(|loaded| *loaded) {
            break;
        }
    }
    assert_eq!(loaded, [true, true]);
    assert_eq!(boundary.resource_buffered_bytes(), 0);

    let inspection = transact(&mut boundary, &request(1, 3));
    assert_eq!(&inspection[..8], &world_session(1).to_le_bytes());
    assert_eq!(&inspection[41..45], &2u32.to_le_bytes());
}

#[test]
fn consumed_frame_output_is_invalidated_without_relabeling_the_owned_copy() {
    let mut boundary = WasmHostBoundary::new();
    connect(&mut boundary, 1);
    for _ in 0..3 {
        assert!(boundary.tick(0.0));
        while boundary.poll() {}
    }
    assert!(boundary.tick(0.0));
    assert!(boundary.poll());
    let bytes = boundary.output().to_vec();
    assert!(!boundary.poll());
    assert!(boundary.output_ptr().is_null());
    assert!(boundary.tick(0.0));
    assert!(boundary.poll());
    assert!(!boundary.output_ptr().is_null());
    assert_eq!(&boundary.output()[..8], &world_session(1).to_le_bytes());
    assert_eq!(&boundary.output()[8..16], &0u64.to_le_bytes());
    assert_eq!(boundary.output()[24], 4);
    assert_eq!(boundary.output_len(), bytes.len());
    assert_ne!(&boundary.output()[16..24], &bytes[16..24]);
}
