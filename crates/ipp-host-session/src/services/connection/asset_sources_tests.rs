use super::*;

struct UploadClient {
    session: u64,
    request: u64,
}

impl UploadClient {
    fn new(host: &mut Host<TestHostServices>) -> Self {
        open(host, 1);
        let HostResponseBody::Attached {
            session,
            ..
        } = create_and_open(
            host,
            1,
            HostRequestBody::CreateWorld {
                options: host::WorldCreateOptions::new(Vec::new()),
                temporary: false,
            },
        )
        else {
            panic!("attachment expected")
        };
        while host.take_connection_response(1).is_some() {}

        Self {
            session,
            request: 0,
        }
    }

    fn send(
        &mut self,
        host: &mut Host<TestHostServices>,
        tag: u8,
        payload: &[u8],
    ) -> crate::ReliableResponse {
        self.request += 1;
        let mut frame = ipp_protocol::asset_source::REQUEST_MAGIC.to_vec();
        frame.extend(self.session.to_le_bytes());
        frame.extend(self.request.to_le_bytes());
        frame.push(tag);
        frame.extend(payload);
        host.receive_connection(1, &frame).unwrap();

        let response = host.take_connection_response(1).unwrap();
        assert_eq!(&response[..4], ipp_protocol::asset_source::RESPONSE_MAGIC);
        assert_eq!(&response[12..20], &self.request.to_le_bytes());
        response
    }

    fn begin(&mut self, host: &mut Host<TestHostServices>, length: u64) -> u64 {
        let name = format!("client://{}/fixture#upload", self.session);
        let mut payload = 2u32.to_le_bytes().to_vec();
        payload.extend((name.len() as u32).to_le_bytes());
        payload.extend(name.as_bytes());
        payload.extend(0u32.to_le_bytes());
        payload.extend(length.to_le_bytes());
        assert_eq!(self.send(host, 0, &payload)[20], 0);
        self.request
    }

    fn chunk(
        &mut self,
        host: &mut Host<TestHostServices>,
        transfer: u64,
        offset: u64,
        bytes: &[u8],
    ) -> crate::ReliableResponse {
        let mut payload = transfer.to_le_bytes().to_vec();
        payload.extend(offset.to_le_bytes());
        payload.extend((bytes.len() as u32).to_le_bytes());
        payload.extend(bytes);
        self.send(host, 1, &payload)
    }
}

fn assert_error(response: &[u8], expected: &str) {
    assert_eq!(response[20], 1);
    let length = u32::from_le_bytes(response[21..25].try_into().unwrap()) as usize;
    assert_eq!(
        std::str::from_utf8(&response[25..25 + length]).unwrap(),
        expected
    );
}

#[test]
fn invalid_asset_chunks_discard_staging_without_publication() {
    let mut host = Host::<TestHostServices>::new().unwrap();
    let mut client = UploadClient::new(&mut host);

    for (offset, bytes) in [(0, &[][..]), (0, &[1, 2, 3][..]), (1, &[1][..])] {
        let transfer = client.begin(&mut host, 2);
        assert_error(
            &client.chunk(&mut host, transfer, offset, bytes),
            "Invalid asset source chunk bounds",
        );
        assert!(host.sessions[&client.session].source_transfers.is_empty());
        assert!(host.sessions[&client.session].client_sources.is_empty());
        assert_error(
            &client.send(&mut host, 2, &transfer.to_le_bytes()),
            "Unknown asset source transfer",
        );
    }
}

#[test]
fn cancellation_discards_partial_and_complete_unpublished_asset_bytes() {
    let mut host = Host::<TestHostServices>::new().unwrap();
    let mut client = UploadClient::new(&mut host);

    for bytes in [&[1][..], &[1, 2][..]] {
        let transfer = client.begin(&mut host, 2);
        assert_eq!(client.chunk(&mut host, transfer, 0, bytes)[20], 0);
        assert_eq!(client.send(&mut host, 3, &transfer.to_le_bytes())[20], 0);
        assert_eq!(client.send(&mut host, 3, &transfer.to_le_bytes())[20], 0);
        assert!(host.sessions[&client.session].source_transfers.is_empty());
        assert_error(
            &client.chunk(&mut host, transfer, 1, &[2]),
            "Unknown asset source transfer",
        );
        assert_error(
            &client.send(&mut host, 2, &transfer.to_le_bytes()),
            "Unknown asset source transfer",
        );
        assert!(host.sessions[&client.session].client_sources.is_empty());
    }
}

#[test]
fn incomplete_finish_discards_staging_and_duplicate_finish_does_not_republish() {
    let mut host = Host::<TestHostServices>::new().unwrap();
    let mut client = UploadClient::new(&mut host);
    let transfer = client.begin(&mut host, 2);
    assert_eq!(client.chunk(&mut host, transfer, 0, &[1])[20], 0);
    assert_error(
        &client.send(&mut host, 2, &transfer.to_le_bytes()),
        "Incomplete asset source transfer",
    );
    assert!(host.sessions[&client.session].source_transfers.is_empty());
    assert!(host.sessions[&client.session].client_sources.is_empty());

    let replacement = client.begin(&mut host, 2);
    assert_error(
        &client.send(&mut host, 2, &transfer.to_le_bytes()),
        "Unknown asset source transfer",
    );
    assert_eq!(host.sessions[&client.session].source_transfers.len(), 1);
    assert_eq!(client.chunk(&mut host, replacement, 0, &[1])[20], 0);
    assert_eq!(client.chunk(&mut host, replacement, 1, &[2])[20], 0);
    assert!(host.sessions[&client.session].client_sources.is_empty());
    assert_eq!(client.send(&mut host, 2, &replacement.to_le_bytes())[20], 0);
    assert_eq!(host.sessions[&client.session].client_sources.len(), 1);
    assert_error(
        &client.send(&mut host, 2, &replacement.to_le_bytes()),
        "Unknown asset source transfer",
    );
    assert_eq!(host.sessions[&client.session].client_sources.len(), 1);
}
