use super::*;

#[test]
fn source_chunks_are_borrowed_bounded_and_separate_from_commands() {
    let mut bytes = REQUEST_MAGIC.to_vec();
    bytes.extend(7u64.to_le_bytes());
    bytes.extend(9u64.to_le_bytes());
    bytes.push(1);
    bytes.extend(2u64.to_le_bytes());
    bytes.extend(0u64.to_le_bytes());
    bytes.extend((CHUNK_BYTES as u32).to_le_bytes());
    bytes.extend(vec![42; CHUNK_BYTES]);
    let request = decode(&bytes, 7).unwrap();
    let SourceOperation::Chunk {
        bytes: payload,
        ..
    } = request.operation
    else {
        panic!("chunk expected")
    };
    assert_eq!(payload.as_ptr(), bytes[41..].as_ptr());
    assert_eq!(payload, vec![42; CHUNK_BYTES]);
    assert!(matches!(
        decode(&bytes, 8),
        Err(ProtocolError::SessionMismatch)
    ));
    assert!(crate::decode_request(&bytes, 7).is_err());
    for length in [0, 4, 12, 20, 21, 29, 37, 40, bytes.len() - 1] {
        assert!(decode(&bytes[..length], 7).is_err());
    }
    bytes.push(0);
    assert!(decode(&bytes, 7).is_err());
    bytes[37..41].copy_from_slice(&((CHUNK_BYTES + 1) as u32).to_le_bytes());
    assert!(matches!(decode(&bytes, 7), Err(ProtocolError::Limit(_))));
}

#[test]
fn asset_bytes_have_no_world_command_tag() {
    for tag in [4, 21] {
        let mut bytes = 7u64.to_le_bytes().to_vec();
        bytes.extend(1u64.to_le_bytes());
        bytes.push(tag);
        bytes.extend([42; 64]);
        assert!(
            matches!(crate::decode_request(&bytes, 7), Err(ProtocolError::Unsupported(actual)) if actual == tag)
        );
    }
}
