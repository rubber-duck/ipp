//! Native sources use asynchronous operations and the Host's shared IO pool.
//!
//! FileSystemIoSource confines literal names to its configured root. The shipped
//! server enables HTTP only through repeatable `--http-prefix` options (none by
//! default); Host registration rejects overlapping namespaces. An enabled source
//! does not itself grant a connection asset-export authority.
//!
//! Linux SealedMappedIoSource lends immutable memfd bytes directly after kernel
//! WRITE/GROW/SHRINK seals are verified. It does not support ordinary mutable-file
//! mapping. File and HTTP streams fill one eventual lent buffer; HTTP/TLS library
//! transport buffers are separate from that reader-storage accounting. External
//! browser ArrayBuffer/SAB sources use one admitted copy into WASM storage, not
//! native mapping or directly borrowed shared WebAssembly.Memory.

mod file_output;
mod file_system;
mod http;
mod http_transport;
#[cfg(target_os = "linux")]
mod mapped_input;
mod stream_input;

pub use file_output::NativeFileIoWriter;
pub use file_system::FileSystemIoSource;
pub use http::HttpIoSource;
#[cfg(target_os = "linux")]
pub use mapped_input::SealedMappedIoSource;
