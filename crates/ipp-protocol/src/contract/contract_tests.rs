use super::*;

#[test]
fn hello_announces_revision_hash_and_connection_and_rejects_only_foreign_input() {
    let reply = accept_hello(&HELLO, 7).unwrap();
    assert_eq!(reply.len(), 24);
    assert_eq!(&reply[..4], b"IPPB");
    assert_eq!(reply[4..8], VERSION.to_le_bytes());
    assert_eq!(reply[8..16], schema_hash().to_le_bytes());
    assert_eq!(reply[16..], 7u64.to_le_bytes());

    // A hello is not this protocol when its marker or length differs; a
    // former bootstrap carrying a schema claim is rejected for its length.
    let mut wrong = HELLO;
    wrong[3] ^= 1;
    for foreign in [&wrong[..], &HELLO[..3], &announcement()[..]] {
        assert!(matches!(
            accept_hello(foreign, 7),
            Err(ProtocolError::Malformed(_))
        ));
    }
    assert_eq!(accept_hello(&HELLO, 0), Err(ProtocolError::SessionMismatch));
}

#[test]
fn contract_reply_carries_a_schema_independent_bulk_descriptor() {
    let reply = crate::bulk_read::contract_descriptor(crate::bulk_read::BulkReadDescriptor {
        reference: crate::bulk_read::BulkReadReference {
            connection: 7,
            read: 1,
        },
        length: Some(export_contract().len() as u64),
    })
    .unwrap();
    assert_eq!(&reply[..4], &CONTRACT_REPLY_MAGIC);
    assert_eq!(reply.len(), 28);
    assert_eq!(
        u64::from_le_bytes(reply[20..28].try_into().unwrap()),
        export_contract().len() as u64
    );
    assert_eq!(reply.capacity(), reply.len());
    assert!(reply.len() <= crate::MAX_MESSAGE_BYTES);

    let contract = export_contract();
    assert_eq!(contract[..16], announcement());
    let mut hash = ContractHash::default();
    ContractSink::write(&mut hash, &contract[16..]);
    assert_eq!(hash.0, schema_hash());
}
