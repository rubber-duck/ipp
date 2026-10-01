use ipp_core::services::reliable_output::{OutputCharge, ReliableOutputLease};
use ipp_host_session::PreparedOutputCopy;
use std::collections::LinkedList;

/// Physical connections one worker Host serves at once, exported to the worker by
/// `ipp_connection_limit`. 64 covers many tabs' or components' ports on one surface while
/// bounding per-connection metadata to 512 KiB; one more connection is refused.
pub(crate) const MAX_CONNECTIONS: usize = 64;
/// Uncompleted output deliveries per connection, exported by `ipp_delivery_limit`. Matches
/// the request window so a full window of replies can be in flight; the worker waits for
/// completions beyond it.
pub(crate) const MAX_DELIVERIES: usize = 64;
/// Reliable-output bytes charged for each open connection's bookkeeping.
pub(crate) const CONNECTION_METADATA_BYTES: usize = 8192;
/// UTF-8 bytes kept of a connection failure diagnostic; longer text is truncated.
pub(crate) const MAX_FAILURE_BYTES: usize = 1024;

pub(crate) struct Delivery {
    pub(crate) id: u64,
    pub(crate) output: PreparedOutputCopy,
    pub(crate) copied: bool,
}

pub(crate) struct ConnectionOutput {
    pub(crate) live: bool,
    pub(crate) failure: Option<String>,
    pub(crate) deliveries: LinkedList<Delivery>,
    _metadata: ReliableOutputLease,
}

impl ConnectionOutput {
    pub(crate) fn new(mut metadata: ReliableOutputLease) -> Result<Self, String> {
        metadata
            .resize(OutputCharge {
                entries: 1,
                bytes: CONNECTION_METADATA_BYTES,
            })
            .map_err(|error| format!("connection metadata capacity: {error:?}"))?;
        Ok(Self {
            live: true,
            failure: None,
            deliveries: LinkedList::new(),
            _metadata: metadata,
        })
    }

    pub(crate) fn fail(&mut self, message: &str) {
        self.live = false;
        if self.failure.is_none() {
            self.failure = Some(
                message[..message.floor_char_boundary(MAX_FAILURE_BYTES.min(message.len()))]
                    .to_owned(),
            );
        }
    }
}
