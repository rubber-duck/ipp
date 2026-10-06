//! Network inactivity timeouts that exclude consumer backpressure.
//!
//! This adapter uses ureq's explicitly unversioned transport API, guarded by the
//! workspace's exact dependency pin. A fresh timeout is applied only while the
//! transport waits for network input, not while an IO window remains borrowed.

use ureq::unversioned::transport::{
    Buffers, ConnectionDetails, Connector, NextTimeout, Transport, time::Duration,
};

#[derive(Debug, Default)]
pub(super) struct InactivityConnector;

#[derive(Debug)]
pub(super) struct InactivityTransport<T>(T);

impl<T: Transport> Connector<T> for InactivityConnector {
    type Out = InactivityTransport<T>;

    fn connect(
        &self,
        _details: &ConnectionDetails<'_>,
        chained: Option<T>,
    ) -> Result<Option<Self::Out>, ureq::Error> {
        Ok(chained.map(InactivityTransport))
    }
}

impl<T: Transport> Transport for InactivityTransport<T> {
    fn buffers(&mut self) -> &mut dyn Buffers {
        self.0.buffers()
    }

    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), ureq::Error> {
        self.0.transmit_output(amount, timeout)
    }

    fn await_input(&mut self, mut timeout: NextTimeout) -> Result<bool, ureq::Error> {
        let inactivity = Duration::from_secs(30);
        if timeout.after > inactivity {
            timeout.after = inactivity;
            timeout.reason = ureq::Timeout::RecvBody;
        }
        self.0.await_input(timeout)
    }

    fn is_open(&mut self) -> bool {
        self.0.is_open()
    }

    fn is_tls(&self) -> bool {
        self.0.is_tls()
    }
}
