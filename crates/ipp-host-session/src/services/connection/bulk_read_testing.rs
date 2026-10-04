//! Instrumentation-only real-transport probes; production Hosts do not route these frames.

use super::*;
use ipp_core::services::io::{IoReadOptions, StreamIoReader};

impl<P: HostServices> Host<P> {
    pub(super) fn receive_bulk_test(
        &mut self,
        connection: u64,
        bytes: &[u8],
    ) -> Result<(), String> {
        if bytes.len() != 21 || bytes.get(4..12) != Some(&connection.to_le_bytes()) {
            return Err("Malformed bulk test control".into());
        }
        let request = u64::from_le_bytes(bytes[12..20].try_into().unwrap());
        let state = self
            .connections
            .states
            .get(&connection)
            .ok_or("Connection is closed")?;
        let reservation = state.reserve_reply(256)?;
        let mut response = b"IPDU".to_vec();
        response.extend(connection.to_le_bytes());
        response.extend(request.to_le_bytes());
        match bytes[20] {
            0 => {
                let (reader, input) = StreamIoReader::new(IoReadOptions {
                    max_bytes: None,
                    recovery: false,
                });
                let descriptor =
                    self.publish_connection_reader(connection, Box::new(reader), None)?;
                let state = self
                    .connections
                    .states
                    .get_mut(&connection)
                    .expect("live connection");
                state.bulk_test_inputs.retain(|_, input| input.is_open());
                state
                    .bulk_test_inputs
                    .insert(descriptor.reference.read, input);
                response.push(0);
                response.extend(descriptor.reference.read.to_le_bytes());
            }
            1 => {
                self.signal_severe_memory_pressure();
                response.push(1);
            }
            operation @ 3..=6 => {
                self.asset_export_test_fixture(operation)?;
                response.push(operation);
            }
            _ => return Err("Unknown bulk test control".into()),
        }
        reservation.borrow_mut().encoded(response.capacity());
        self.connections.states[&connection]
            .outbox
            .push_back(ReliableResponse {
                bytes: response,
                reservation,
            });
        Ok(())
    }
}
