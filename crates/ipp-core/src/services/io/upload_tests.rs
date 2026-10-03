use super::*;

#[test]
fn ordered_chunks_complete_with_owned_bytes_without_a_completion_copy() {
    let mut upload = IoUploadAssembly::new(5);
    upload.push(0, &[2, 3]).unwrap();
    upload.push(2, &[5, 7, 11]).unwrap();
    let staging = upload.bytes.as_ptr();

    let bytes = upload.finish().unwrap();
    assert_eq!(bytes, [2, 3, 5, 7, 11]);
    assert_eq!(bytes.as_ptr(), staging);
}

#[test]
fn invalid_chunks_preserve_accepted_bytes_and_do_not_allocate() {
    let mut upload = IoUploadAssembly::new(4);
    upload.push(0, &[2, 3]).unwrap();
    let capacity = upload.bytes.capacity();

    for (offset, bytes) in [
        (0, &[5][..]), // Duplicate prefix.
        (1, &[5][..]), // Overlap.
        (3, &[5][..]), // Gap.
        (u64::MAX, &[5][..]),
        (2, &[][..]),
        (2, &[5, 7, 11][..]), // Beyond the declared end.
    ] {
        assert_eq!(
            upload.push(offset, bytes),
            Err(IoUploadError::InvalidChunkBounds)
        );
        assert_eq!(upload.bytes, [2, 3]);
        assert_eq!(upload.bytes.capacity(), capacity);
    }

    upload.push(2, &[5, 7]).unwrap();
    assert_eq!(upload.finish().unwrap(), [2, 3, 5, 7]);
}

#[test]
fn complete_input_rejects_further_chunks() {
    let mut upload = IoUploadAssembly::new(1);
    upload.push(0, &[2]).unwrap();
    assert_eq!(upload.push(1, &[3]), Err(IoUploadError::InvalidChunkBounds));
    assert_eq!(upload.finish().unwrap(), [2]);
}

#[test]
fn truncated_input_cannot_be_completed() {
    let mut upload = IoUploadAssembly::new(3);
    upload.push(0, &[2, 3]).unwrap();
    assert_eq!(upload.finish(), Err(IoUploadError::Incomplete));
    assert_eq!(
        IoUploadAssembly::new(1).finish(),
        Err(IoUploadError::Incomplete)
    );
}

#[test]
fn empty_upload_finishes_without_a_chunk() {
    assert_eq!(IoUploadAssembly::new(0).finish().unwrap(), []);
}

#[test]
fn declaring_a_large_upload_does_not_allocate_the_declared_length() {
    let upload = IoUploadAssembly::new(usize::MAX);
    assert_eq!(upload.bytes.capacity(), 0);
    assert_eq!(upload.finish(), Err(IoUploadError::Incomplete));
}
