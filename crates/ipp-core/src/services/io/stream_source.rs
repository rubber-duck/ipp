//! Host-fed stream routing and exact acquisition correlation.

use super::{
    IoListFuture, IoOpenReadFuture, IoReadOptions, IoReadRequest, IoReader, IoSource,
    IoStreamInput, StreamIoReader,
};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct IoStreamRegistry {
    next_id: u64,
    requests: VecDeque<IoReadRequest>,
    inputs: BTreeMap<u64, IoStreamInput>,
}

#[derive(Clone, Default)]
pub(super) struct IoStreamProvider {
    streams: Arc<Mutex<IoStreamRegistry>>,
}

impl IoStreamProvider {
    pub fn take_requests(&self, mut selected: impl FnMut(u64) -> bool) -> Vec<IoReadRequest> {
        let mut streams = self.streams.lock().expect("IO stream registry");
        let pending: Vec<_> = streams.requests.drain(..).collect();
        let mut requests = Vec::new();
        for request in pending {
            if !streams
                .inputs
                .get(&request.id)
                .is_some_and(IoStreamInput::is_open)
            {
                continue;
            }
            if selected(request.id) {
                requests.push(request);
            } else {
                streams.requests.push_back(request);
            }
        }
        requests
    }

    pub fn input(&self, id: u64) -> Option<IoStreamInput> {
        self.streams
            .lock()
            .expect("IO stream registry")
            .inputs
            .get(&id)
            .cloned()
    }

    pub fn take_closed(&self) -> Vec<u64> {
        let mut streams = self.streams.lock().expect("IO stream registry");
        let closed: Vec<_> = streams
            .inputs
            .iter()
            .filter(|(_, input)| !input.is_open())
            .map(|(&id, _)| id)
            .collect();
        let cancelled = closed
            .iter()
            .filter(|id| !streams.inputs[id].finished())
            .copied()
            .collect();
        for id in &closed {
            streams.inputs.remove(id);
        }
        streams
            .requests
            .retain(|request| !closed.contains(&request.id));
        cancelled
    }

    pub fn buffered_bytes(&self) -> usize {
        self.streams
            .lock()
            .expect("IO stream registry")
            .inputs
            .values()
            .map(IoStreamInput::buffered_bytes)
            .sum()
    }
}

impl IoSource for IoStreamProvider {
    fn list(&mut self, _identifier: &str) -> IoListFuture {
        Box::pin(std::future::ready(Err(
            "Source listing is unavailable".into()
        )))
    }

    fn open_read(&mut self, identifier: &str, options: IoReadOptions) -> IoOpenReadFuture {
        let mut streams = self.streams.lock().expect("IO stream registry");
        let Some(id) = streams.next_id.checked_add(1) else {
            return Box::pin(std::future::ready(Err(
                "IO stream identity exhausted".into()
            )));
        };
        streams.next_id = id;
        let (mut reader, input) = StreamIoReader::new(options);
        reader.set_request(id);
        streams.inputs.insert(id, input);
        streams.requests.push_back(IoReadRequest {
            id,
            identifier: identifier.to_owned(),
            max_bytes: options.max_bytes,
            recovery: options.recovery,
        });
        Box::pin(std::future::ready(
            Ok(Box::new(reader) as Box<dyn IoReader>),
        ))
    }
}
