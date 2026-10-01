use super::*;
use crate::host::*;
use crate::references::WorldReference;

#[test]
fn presentation_contract_roundtrips_exact_identities_and_rejects_truncation() {
    let identity = PresentationIdentity {
        host: u64::MAX - 2,
        serial: u64::MAX - 1,
    };
    let root = RootBinding {
        output: OutputReference {
            world: WorldReference {
                id: 9,
                incarnation: 17,
            },
            target: crate::references::OutputTarget::Camera {
                entity: u64::MAX,
                incarnation: 18,
            },
        },
        viewport: WorldViewport {
            width: 640,
            height: 480,
            device_pixel_ratio: 2.0,
        },
        generation: identity,
    };
    let surface = PresentationSurface {
        id: 1,
        context: u64::MAX,
        max_width: 8192,
        max_height: 4096,
    };
    let view = PresentationView {
        surface,
        selection: u64::MAX - 3,
        binding: root,
    };
    let frame = PresentedFrame {
        view,
        sequence: 31,
        publication: identity,
        draw_calls: 8,
        triangles: 97,
        failed_draw_calls: 2,
        sources: vec![PresentedSource {
            output: root.output,
            minimum_tick: 12,
            publication: identity,
            tick: 13,
        }],
    };
    for body in [
        PresentationRequest::Surface,
        PresentationRequest::Select {
            surface,
            binding: root,
        },
        PresentationRequest::Clear(view),
        PresentationRequest::Frame {
            view,
            after_sequence: None,
            publication: None,
            capture: false,
            after_outputs: Vec::new(),
        },
        PresentationRequest::Frame {
            view,
            after_sequence: Some(30),
            publication: Some(identity),
            capture: true,
            after_outputs: vec![root.output; 65],
        },
        PresentationRequest::ReadCapture {
            capture: 41,
            offset: 65536,
        },
        PresentationRequest::ReleaseCapture(41),
        PresentationRequest::CancelFrame {
            request: 19,
        },
    ] {
        let request = HostRequest {
            connection: 7,
            request_id: 2,
            body: HostRequestBody::Presentation(body),
        };
        let bytes = encode_host_request(&request).unwrap();
        assert_eq!(decode_host_request(&bytes, 7).unwrap(), request);
        for length in 0..bytes.len() {
            assert!(decode_host_request(&bytes[..length], 7).is_err());
        }
        let mut trailing = bytes;
        trailing.push(0);
        assert!(decode_host_request(&trailing, 7).is_err());
    }
    let bounded = HostRequest {
        connection: 7,
        request_id: 9,
        body: HostRequestBody::Presentation(PresentationRequest::Frame {
            view,
            after_sequence: None,
            publication: None,
            capture: false,
            after_outputs: vec![root.output; MAX_PRESENTATION_SOURCES],
        }),
    };
    let bounded_bytes = encode_host_request(&bounded).unwrap();
    assert!(bounded_bytes.len() <= crate::MAX_MESSAGE_BYTES);
    assert_eq!(decode_host_request(&bounded_bytes, 7).unwrap(), bounded);
    let mut largest_frame = frame.clone();
    largest_frame.sources = vec![frame.sources[0].clone(); MAX_PRESENTATION_SOURCES];
    let bounded_reply = HostResponse {
        connection: 7,
        request_id: 9,
        body: HostResponseBody::Presentation(PresentationResponse::Frame(largest_frame)),
    };
    let reply_bytes = encode_host_response(&bounded_reply).unwrap();
    assert!(reply_bytes.len() <= crate::MAX_MESSAGE_BYTES);
    assert_eq!(
        decode_host_response(&reply_bytes, 7).unwrap(),
        bounded_reply
    );
    let mut oversized = bounded;
    if let HostRequestBody::Presentation(PresentationRequest::Frame {
        after_outputs,
        ..
    }) = &mut oversized.body
    {
        after_outputs.push(root.output);
    }
    assert!(encode_host_request(&oversized).is_err());
    let mut replies = vec![
        HostResponseBody::RootBinding(None),
        HostResponseBody::RootBinding(Some(root)),
        HostResponseBody::Presentation(PresentationResponse::Surface(surface)),
        HostResponseBody::Presentation(PresentationResponse::View(view)),
        HostResponseBody::Presentation(PresentationResponse::Frame(frame.clone())),
        HostResponseBody::Presentation(PresentationResponse::Capture {
            frame,
            capture: 41,
            bytes: 640 * 480 * 4,
        }),
        HostResponseBody::Presentation(PresentationResponse::Chunk {
            capture: 41,
            offset: 65536,
            bytes: vec![42; 65536],
        }),
        HostResponseBody::Presentation(PresentationResponse::Complete),
    ];
    for error in [
        PresentationError::Unsupported,
        PresentationError::Unavailable,
        PresentationError::StaleView,
        PresentationError::InvalidViewport,
        PresentationError::ObsoletePublication,
        PresentationError::Capacity,
        PresentationError::Timeout,
        PresentationError::DrawFailed,
    ] {
        replies.push(HostResponseBody::Presentation(PresentationResponse::Error(
            error,
        )));
    }
    for body in replies {
        let response = HostResponse {
            connection: 7,
            request_id: 2,
            body,
        };
        let bytes = encode_host_response(&response).unwrap();
        assert_eq!(decode_host_response(&bytes, 7).unwrap(), response);
        for length in [0, 24, bytes.len() - 1] {
            assert!(decode_host_response(&bytes[..length], 7).is_err());
        }
        let mut trailing = bytes;
        trailing.push(0);
        assert!(decode_host_response(&trailing, 7).is_err());
    }
}
