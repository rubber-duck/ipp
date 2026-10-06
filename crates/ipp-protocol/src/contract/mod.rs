//! Schema-independent connection opening and contract retrieval: the hello, the
//! Host's announcement and the exported binary contract with its wire manifest.

mod data_authoring;
mod layout_fixture;
pub(crate) mod wire_manifest;

#[cfg(test)]
mod contract_tests;

pub use layout_fixture::{check as check_layout_fixture, export as export_layout_fixture};

use crate::ProtocolError;
use ipp_core::components::schema::{ContractHash, ContractSink};

/// Fixed schema-independent marker that starts the hello, the Host's
/// announcement and every exported contract.
pub const MAGIC: [u8; 4] = *b"IPPB";

/// Wire revision, announced by the Host and recorded in its contract.
pub const VERSION: u32 = 3;

/// The first message of every connection. It carries no claim, so any client
/// can open a connection and read the Host's [`announcement`].
pub const HELLO: [u8; 4] = MAGIC;

/// Schema-independent request for the Host's full contract.
pub const CONTRACT_REQUEST: [u8; 4] = *b"IPCQ";

/// Marker of the fixed read-descriptor reply to [`CONTRACT_REQUEST`].
pub const CONTRACT_REPLY_MAGIC: [u8; 4] = *b"IPCR";

fn write_contract(sink: &mut impl ContractSink) {
    ipp_core::components::registry::write_contract(sink);
    wire_manifest::write_contract(sink);
}

/// Identity of this target's actual compiled registry, defaults and wire contract.
///
/// The contract is fixed for a compiled build, so it is hashed once per process.
/// Zero marks the unset cache; concurrent first reads compute the same value.
pub fn schema_hash() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};

    static HASH: AtomicU64 = AtomicU64::new(0);

    let cached = HASH.load(Ordering::Relaxed);
    if cached != 0 {
        return cached;
    }

    let mut hash = ContractHash::default();
    write_contract(&mut hash);
    HASH.store(hash.0, Ordering::Relaxed);
    hash.0
}

/// The Host's 16-byte announcement: [`MAGIC`], the little-endian [`VERSION`]
/// and the little-endian [`schema_hash`]. It also heads the exported contract.
///
/// The Host makes no compatibility decision: clients compare the announcement
/// with the contract they were generated from, or read the contract.
pub fn announcement() -> [u8; 16] {
    let mut bytes = [0u8; 16];
    bytes[..4].copy_from_slice(&MAGIC);
    bytes[4..8].copy_from_slice(&VERSION.to_le_bytes());
    bytes[8..].copy_from_slice(&schema_hash().to_le_bytes());
    bytes
}

/// Accept the [`HELLO`] and reply with the [`announcement`] followed by the
/// connection's fresh little-endian identity.
///
/// Only a message that is not this protocol's hello is rejected; the hello
/// carries no claim to check.
pub fn accept_hello(bytes: &[u8], connection: u64) -> Result<Vec<u8>, ProtocolError> {
    if bytes.len() != HELLO.len() {
        return Err(ProtocolError::Malformed("hello length"));
    }
    if bytes != HELLO {
        return Err(ProtocolError::Malformed("hello magic"));
    }
    if connection == 0 {
        return Err(ProtocolError::SessionMismatch);
    }
    let mut reply = Vec::with_capacity(24);
    reply.extend_from_slice(&announcement());
    reply.extend_from_slice(&connection.to_le_bytes());
    Ok(reply)
}

/// Binary contract descriptors, executed in the same target build as the
/// Host and headed by its [`announcement`]: the bytes the generator consumes.
///
/// The contract is fixed for a compiled build, so it is built once per process.
pub fn export_contract() -> &'static [u8] {
    static CONTRACT: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();

    CONTRACT.get_or_init(|| {
        let mut bytes = announcement().to_vec();
        write_contract(&mut bytes);
        bytes
    })
}
