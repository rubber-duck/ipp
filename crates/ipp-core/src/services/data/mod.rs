//! Host-owned typed datasets, independent of component rows, assets and codecs.

mod consumer;
mod consumer_access;
mod read;
mod retention;
mod schema;
mod service;
mod source;
mod update;

pub use consumer::{
    DataAvailability, DataConsumerHandle, DataConsumerIdentity, DataConsumerNotification,
    DataConsumerRequest, DataConsumerState, DataWindow, DataWindowAnchor,
};
pub use consumer_access::DataConsumerAccess;
pub use read::{DataReadView, DataRowView};
pub use schema::{DataColumn, DataSchema};
pub use service::{DataService, DataServiceConfig};
pub use source::{
    DataProducerHandle, DataRowId, DataSourceHandle, DataSourceKind, DataSourceMemory,
};
pub use update::{DataBatchError, DataBatchOutcome, DataDelta, DataError};

#[cfg(test)]
mod service_tests;

#[cfg(test)]
mod clock_tests;
