use super::*;
use crate::host::{HostResponse, HostResponseBody, decode_host_response, encode_host_response};
use crate::references::WorldReference;
use crate::world::gui::GuiTarget;
use crate::world::view_queries::{CameraAttachmentReference, MAX_CAMERA_PATH_DEPTH};

fn response(depth: usize) -> HostResponse {
    let world = WorldReference {
        id: 7,
        incarnation: 8,
    };
    HostResponse {
        connection: 1,
        request_id: 2,
        body: HostResponseBody::GuiInput(GuiPhysicalResponse::Routed {
            disposition: GUI_PHYSICAL_DISPOSITION_UNHANDLED,
            applied: 0,
            rejected: 0,
            cancelled: 0,
            error: None,
            remaining: Some([0.0, 1.0]),
            native: None,
            hit: Some(GuiInputHit {
                target: GuiTarget {
                    world,
                    entity: ipp_core::EntityId::from_bits(9),
                    component: 36,
                    incarnation: 10,
                },
                path: (0..depth)
                    .map(|index| CameraAttachmentReference {
                        parent: world,
                        anchor: index as u64 + 11,
                        incarnation: 12,
                        revision: 13,
                    })
                    .collect(),
            }),
        }),
    }
}

#[test]
fn wheel_hit_round_trips_exact_lifetimes_with_or_without_consumption() {
    for depth in [0, 1, MAX_CAMERA_PATH_DEPTH] {
        for disposition in [
            GUI_PHYSICAL_DISPOSITION_ROUTED,
            GUI_PHYSICAL_DISPOSITION_UNHANDLED,
        ] {
            let mut response = response(depth);
            let HostResponseBody::GuiInput(GuiPhysicalResponse::Routed {
                disposition: value,
                remaining,
                ..
            }) = &mut response.body
            else {
                unreachable!()
            };
            *value = disposition;
            if disposition == GUI_PHYSICAL_DISPOSITION_ROUTED {
                *remaining = None;
            }
            let bytes = encode_host_response(&response).unwrap();
            assert_eq!(decode_host_response(&bytes, 1).unwrap(), response);
        }
    }
}

#[test]
fn wheel_hit_path_is_bounded_before_encoding_or_decoding_entries() {
    assert!(input_hit_size(MAX_CAMERA_PATH_DEPTH + 1).is_err());
    assert!(encode_host_response(&response(MAX_CAMERA_PATH_DEPTH + 1)).is_err());
    let bytes = encode_host_response(&response(0)).unwrap();
    for count in [1u32, MAX_CAMERA_PATH_DEPTH as u32 + 1] {
        let mut malformed = bytes.clone();
        let offset = malformed.len() - 4;
        malformed[offset..].copy_from_slice(&count.to_le_bytes());
        assert!(decode_host_response(&malformed, 1).is_err());
    }
    let mut truncated = bytes;
    truncated.pop();
    assert!(decode_host_response(&truncated, 1).is_err());
}
