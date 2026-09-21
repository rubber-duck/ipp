use super::*;

fn framed(payload: &[u8]) -> Vec<u8> {
    let mut bytes = (payload.len() as u32).to_le_bytes().to_vec();
    bytes.extend_from_slice(payload);
    bytes
}

fn prefix(version: u8, action: u8) -> Vec<u8> {
    let mut payload = vec![version, action];
    payload.extend_from_slice(&1u64.to_le_bytes());
    payload.extend_from_slice(&1u32.to_le_bytes());
    payload
}

fn decode(payload: &[u8]) -> Result<SurfaceCommand, ProtocolError> {
    let bytes = framed(payload);
    Reader {
        bytes: &bytes,
        at: 0,
    }
    .surface_command()
}

fn drawing_insert(asset_presence: u8) -> Vec<u8> {
    let mut payload = prefix(1, 1);
    payload.extend_from_slice(&0u32.to_le_bytes());
    payload.push(3);
    for value in [0.0f32, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.1] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.push(asset_presence);
    payload
}

#[test]
fn rejects_unknown_versions_actions_masks_and_trailing_bytes() {
    assert_eq!(
        decode(&prefix(2, 3)),
        Err(ProtocolError::Malformed("Surface edit version"))
    );
    assert_eq!(
        decode(&prefix(1, 0)),
        Err(ProtocolError::Malformed("Surface edit action"))
    );

    let mut update = prefix(1, 2);
    update.push(0x80);
    assert_eq!(
        decode(&update),
        Err(ProtocolError::Malformed("Surface edit mask"))
    );

    let mut remove = prefix(1, 3);
    remove.push(0);
    assert_eq!(
        decode(&remove),
        Err(ProtocolError::Malformed("trailing Surface edit bytes"))
    );
}

#[test]
fn rejects_invalid_presence_and_nonfinite_numeric_fields() {
    assert_eq!(
        decode(&drawing_insert(2)),
        Err(ProtocolError::Malformed("boolean encoding"))
    );

    let mut update = prefix(1, 2);
    update.push(16);
    update.extend_from_slice(&f32::NAN.to_le_bytes());
    assert_eq!(
        decode(&update),
        Err(ProtocolError::Malformed("nonfinite f32"))
    );
}

#[test]
fn enforces_outer_and_nested_collection_limits_before_allocation() {
    let mut oversized_frame = (65_537u32).to_le_bytes().to_vec();
    oversized_frame.extend_from_slice(&[0; 8]);
    assert_eq!(
        Reader {
            bytes: &oversized_frame,
            at: 0,
        }
        .surface_command(),
        Err(ProtocolError::Limit("count"))
    );

    let mut glyph_run = prefix(1, 1);
    glyph_run.extend_from_slice(&0u32.to_le_bytes());
    glyph_run.push(2);
    glyph_run.extend_from_slice(&65_537u32.to_le_bytes());
    assert_eq!(decode(&glyph_run), Err(ProtocolError::Limit("count")));
}

#[test]
fn accepts_the_smallest_complete_edit() {
    assert_eq!(
        decode(&prefix(1, 3)),
        Ok(SurfaceCommand::Remove {
            entity: ipp_core::EntityId::from_bits(1),
            id: SurfaceItemId(1),
        })
    );
}
