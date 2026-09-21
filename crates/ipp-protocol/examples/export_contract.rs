//! Execute the native target's compiled contract and write its binary export.

#[cfg(feature = "schema-export")]
fn main() -> std::io::Result<()> {
    use std::io::Write;
    std::io::stdout()
        .lock()
        .write_all(&ipp_protocol::export_contract())
}

#[cfg(not(feature = "schema-export"))]
fn main() {
    eprintln!("enable schema-export for target descriptors");
    std::process::exit(2);
}
