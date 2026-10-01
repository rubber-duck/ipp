//! Batch pages decoded before they reach the Host.
//!
//! Decoding a batch page needs no Host or World state, so a transport may run it
//! where the page arrives: a native socket thread decodes while the Host runs
//! frames, and single-threaded hosts decode when they hand the message over.
//! Every other message stays encoded until the Host receives it, because it is
//! small or needs Host state to interpret.

use crate::command_batches::BatchPageArrival;
use ipp_core::{Command, FieldValue};
use ipp_protocol::{Request, RequestBody, RequestDecodeError};

/// One complete connection message, with a batch page already decoded.
pub struct HostConnectionMessage(pub(crate) HostConnectionMessageBody);

pub(crate) enum HostConnectionMessageBody {
    /// Interpreted by the Host with its connection state.
    Encoded(Vec<u8>),
    /// A batch page decoded without Host state; its encoding is released.
    BatchPage(TransportBatchPage),
}

/// A decoded page whose commands may move to the Host thread.
pub(crate) struct TransportBatchPage(BatchPageArrival);

// SAFETY: a `TransportBatchPage` is only built from commands that
// `owns_plain_data` accepted, so it holds integers, strings, vectors and
// unresolved reference tokens and no Host runtime state. The native-only
// command variants whose payloads may hold thread-affine runtime state are
// rejected before construction, so moving the page to the Host thread moves
// plain owned data only; nothing in it is shared or aliased with the sender.
unsafe impl Send for TransportBatchPage {}

impl TransportBatchPage {
    pub(crate) fn into_arrival(self) -> BatchPageArrival {
        self.0
    }
}

impl HostConnectionMessage {
    /// Prepare a received message without Host state.
    ///
    /// A batch page is decoded here, with every check that needs no World state,
    /// and its encoding released. Other messages stay encoded, so the Host
    /// interprets them exactly as [`crate::Host::receive_connection`] would; so
    /// does a page that cannot name its batch, whose error fails the connection.
    pub fn decode(bytes: Vec<u8>) -> Self {
        let page = bytes.len() >= 8
            && bytes.len() <= ipp_protocol::MAX_MESSAGE_BYTES
            && !bytes.starts_with(&ipp_protocol::MAGIC)
            && !bytes.starts_with(ipp_protocol::host::HOST_REQUEST_MAGIC)
            && !bytes.starts_with(ipp_protocol::asset_source::REQUEST_MAGIC)
            && ipp_protocol::is_batch_page(&bytes);
        let arrival = page
            .then(|| match decode_world(&bytes, None, &mut Vec::new()) {
                DecodedWorldRequest::BatchPage(arrival) => Some(arrival),
                DecodedWorldRequest::Request(_) | DecodedWorldRequest::Failed(_) => None,
            })
            .flatten()
            .filter(|arrival| match &arrival.operations {
                Ok(operations) => operations.iter().all(owns_plain_data),
                Err(_) => true,
            });
        match arrival {
            Some(arrival) => Self(HostConnectionMessageBody::BatchPage(TransportBatchPage(
                arrival,
            ))),
            None => Self(HostConnectionMessageBody::Encoded(bytes)),
        }
    }
}

/// A decoded World request.
pub(crate) enum DecodedWorldRequest {
    /// A batch page, including one whose content failed to decode.
    BatchPage(BatchPageArrival),
    /// Any other request.
    Request(Box<Request>),
    /// An error that fails the connection.
    Failed(String),
}

/// Decode a World request, taking `operations` for a decoded batch page.
///
/// Returns the page as it arrived, including a page whose content failed to
/// decode, or else the other request or the error that fails the connection.
pub(crate) fn decode_world(
    bytes: &[u8],
    expected_session: Option<u64>,
    operations: &mut Vec<Command>,
) -> DecodedWorldRequest {
    match ipp_protocol::decode_world_request(bytes, expected_session, operations) {
        Ok(Request {
            session,
            request_id,
            body: RequestBody::SubmitBatch(page),
        }) => DecodedWorldRequest::BatchPage(BatchPageArrival {
            session,
            request_id,
            batch_id: page.batch_id,
            last: page.last,
            heap_bytes: page_heap_bytes(&page.operations),
            aliases: page
                .operations
                .iter()
                .filter(|command| crate::attachment_receipts::defines_alias(command))
                .count(),
            operations: Ok(page.operations),
        }),
        Ok(request) => DecodedWorldRequest::Request(Box::new(request)),
        Err(RequestDecodeError::BatchPage(page)) => {
            DecodedWorldRequest::BatchPage(BatchPageArrival {
                session: page.session,
                request_id: page.request_id,
                batch_id: page.batch_id,
                last: page.last,
                heap_bytes: 0,
                aliases: 0,
                operations: Err(page.error),
            })
        }
        Err(RequestDecodeError::Request(error)) => DecodedWorldRequest::Failed(error.to_string()),
    }
}

/// Every owned payload of the page's commands, or `usize::MAX` for a payload
/// without a stated bound, which then exceeds any budget.
fn page_heap_bytes(operations: &[Command]) -> usize {
    operations
        .iter()
        .try_fold(0usize, |bytes, operation| {
            bytes.checked_add(operation.retained_heap_bytes()?)
        })
        .unwrap_or(usize::MAX)
}

/// Whether a command owns only plain data that may move between threads.
///
/// The decoder produces only wire commands; the native-only variants and
/// resolved World or output handles never come from the wire. The match is
/// exhaustive so a new variant must be classified here.
fn owns_plain_data(command: &Command) -> bool {
    let plain_field = |value: &FieldValue| match value {
        FieldValue::World(value) => value.is_none(),
        FieldValue::Output(value) => value.is_none(),
        FieldValue::UnresolvedWorld(_)
        | FieldValue::UnresolvedOutput(_)
        | FieldValue::Dynamic(_)
        | FieldValue::F32(_)
        | FieldValue::U32(_)
        | FieldValue::U64(_)
        | FieldValue::String(_)
        | FieldValue::Bytes(_)
        | FieldValue::Bool(_)
        | FieldValue::Entity(_)
        | FieldValue::Rows(_)
        | FieldValue::Unset => true,
    };
    match command {
        Command::InsertComponentValue {
            ..
        }
        | Command::DetachWorldAttachmentIf {
            ..
        } => false,
        Command::InsertComponent {
            fields,
            ..
        } => fields.iter().all(|field| plain_field(&field.value)),
        Command::SetField {
            field,
            ..
        } => plain_field(&field.value),
        Command::SetFieldIf {
            field,
            expected,
            ..
        } => plain_field(&field.value) && plain_field(expected),
        Command::DetachWorldAttachmentReceipt {
            ..
        }
        | Command::Create {
            ..
        }
        | Command::Delete {
            ..
        }
        | Command::PlaceEntity {
            ..
        }
        | Command::DeleteSubtree {
            ..
        }
        | Command::SetMetadata {
            ..
        }
        | Command::SetDynamicProperty {
            ..
        }
        | Command::RemoveDynamicProperty {
            ..
        }
        | Command::RemoveComponent {
            ..
        } => true,
        #[cfg(feature = "gui")]
        Command::GuiAction {
            ..
        } => true,
    }
}
