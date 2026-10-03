//! Entity-local projections of Host data; presentation observes one component-owned view.
//!
//! Author an output `foo` as an Asset(kind 19) dynamic property, with an optional
//! ordinary typed `foo_parameter` property. Definition inputs named `column:<name>`
//! read raw source columns; the input `parameter` reads that output's companion.
//! The `_parameter` suffix is reserved for companion values; other dynamic values
//! must select an output definition asset. Absent parameters supply `None` to the
//! evaluator: reached inputs are invalid unless an authored fallback handles them.
//! Raw schema mismatches and present parameters of the wrong type are unavailable.
//!
//! Systems read completed views and register/finish/release their entity-local
//! consumer through `SystemUpdateContext::world` (`SystemRuntimeAccess`), using
//! ordinary lifecycle hooks to track their component membership. Declare ordering
//! after `DataBindingSystem` and acknowledge only after successful preparation.
//! The same APIs exist on `WorldContext`; observations require no consumer.
//!
//! Outputs reuse their value buffers. Exact single-node raw-column identities
//! materialize admitted source values directly. General evaluation allocates an
//! input-reference Vec once per output per changed pass and reuses it across rows; this is not a claim
//! that the entire binding update is allocation-free.
//! No computed output can be used as another projection's input. Definitions and
//! source data are shared; input resolution, scratch and output storage are local.

mod components;
mod presentation;
mod query;
pub use presentation::DataBindingPresentationConsumer;
mod runtime;
mod system;
mod update;
mod windows;

pub use components::{BufferDataSourceBinding, StreamingDataSourceBinding};
pub use query::{
    DATA_BINDING_QUERY_MAX_BYTES, DATA_BINDING_QUERY_MAX_ROWS, DataBindingColumnView,
    DataBindingOutputColumn, DataBindingPreparedView, DataBindingView, DataBindingViewQuery,
    DataBindingViewResult,
};
pub use runtime::{DataBindingAvailability, DataBindingRuntime, DataBindingUnavailable};
pub use system::{DataBindingSystem, DataBindingSystemFactory};
pub use windows::{decode_data_windows, encode_data_windows};

#[cfg(test)]
mod parameter_tests;
