//! Native pooled HTTP acquisition and immutable-content recovery validation.
//!
//! ureq is exactly pinned because the inactivity adapter uses its unversioned
//! transport boundary. Default features are disabled; rustls supplies verified TLS
//! without a native OpenSSL/curl distribution. No gzip, charset or cookie stack is
//! enabled. The owned body reader fills the eventual reader storage on Smol's
//! shared blocking pool and never calls a whole-body convenience reader.
//! The pinned rustls configuration adds 16 normal Linux runtime package entries
//! against the scheduler baseline; three further entries are build tools, while
//! the WASM runtime graph receives none. This sync streaming backend reuses the
//! accepted blocking pool rather than adding a second async networking stack.
//! Reader heap counters exclude intrinsic HTTP/TLS transport buffers.
//! Only HTTP200 complete representations are supported; HTTP202 pending descriptors
//! fail explicitly. Browser availability notification remains a separate adapter.

use ipp_core::services::io::{IoListFuture, IoOpenReadFuture, IoReadOptions, IoReader, IoSource};
use ipp_host_session::services::task_scheduler::IoScheduler;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use ureq::unversioned::{
    resolver::DefaultResolver,
    transport::{Connector, DefaultConnector},
};

/// Host-owned HTTP connection pool. Byte reads run on the shared blocking pool.
#[derive(Clone)]
pub struct HttpIoSource {
    agent: ureq::Agent,
    scheduler: IoScheduler,
    validators: Arc<Mutex<BTreeMap<String, Option<String>>>>,
}

impl HttpIoSource {
    /// Create a pooled TLS-capable source without cookies or whole-body buffering.
    pub fn new(scheduler: IoScheduler) -> Result<Self, String> {
        let config = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(30)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .timeout_recv_body(None)
            .build();
        let connector =
            DefaultConnector::default().chain(super::http_transport::InactivityConnector);
        let agent = ureq::Agent::with_parts(config, connector, DefaultResolver::default());
        Ok(Self {
            agent,
            scheduler,
            validators: Arc::default(),
        })
    }
}

impl IoSource for HttpIoSource {
    fn list(&mut self, _identifier: &str) -> IoListFuture {
        Box::pin(std::future::ready(Err(
            "HTTP source listing is unavailable".into(),
        )))
    }

    fn open_read(&mut self, identifier: &str, options: IoReadOptions) -> IoOpenReadFuture {
        let identifier = identifier.to_owned();
        let agent = self.agent.clone();
        let validators = self.validators.clone();
        let scheduler = self.scheduler.clone();
        Box::pin(async move {
            let reader = scheduler
                .blocking(move || {
                    if !identifier.starts_with("http://") && !identifier.starts_with("https://") {
                        return Err("HTTP source requires an HTTP or HTTPS identifier".into());
                    }
                    let pinned = validators
                        .lock()
                        .expect("HTTP validator lock")
                        .get(&identifier)
                        .cloned()
                        .flatten();
                    if options.recovery && pinned.is_none() {
                        return Err("Immutable HTTP recovery requires a strong ETag".into());
                    }
                    let mut request = agent.get(&identifier);
                    if let Some(pinned) = &pinned {
                        request = request.header("If-Match", pinned);
                    }
                    let response = request.call().map_err(|error| error.to_string())?;
                    if response.status().as_u16() != 200 {
                        return Err("HTTP source did not return a complete representation".into());
                    }
                    let validator = response
                        .headers()
                        .get("etag")
                        .and_then(|value| value.to_str().ok())
                        .filter(|value| value.starts_with('"') && value.ends_with('"'))
                        .map(str::to_owned);
                    if pinned.is_some() && validator != pinned {
                        return Err("Immutable HTTP recovery content validator changed".into());
                    }
                    // Two first opens can overlap on the blocking pool. Validate
                    // again under the publication lock so the second header cannot
                    // overwrite the immutable content selected by the first one.
                    let mut published = validators.lock().expect("HTTP validator lock");
                    if let Some(Some(original)) = published.get(&identifier) {
                        if validator.as_ref() != Some(original) {
                            return Err("Immutable HTTP content validator changed".into());
                        }
                    } else {
                        published.insert(identifier, validator);
                    }
                    drop(published);
                    // The streaming reader has no implicit convenience-method size cap.
                    Ok::<_, String>(
                        response
                            .into_body()
                            .into_with_config()
                            .limit(u64::MAX)
                            .reader(),
                    )
                })
                .await
                .map_err(|error| error.to_string())??;
            Ok(Box::new(super::stream_input::NativeStreamIoReader::new(
                reader, scheduler, options,
            )) as Box<dyn IoReader>)
        })
    }
}

#[cfg(test)]
#[path = "http_tests.rs"]
mod tests;
