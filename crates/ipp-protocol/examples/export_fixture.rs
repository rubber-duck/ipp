//! Execute the maintained layout/owned-dispatch fixture on the native target.

#[cfg(feature = "schema-export")]
fn main() -> std::io::Result<()> {
    use std::io::Write;
    std::io::stdout()
        .lock()
        .write_all(&ipp_protocol::export_layout_fixture())
}

#[cfg(not(feature = "schema-export"))]
fn main() {
    eprintln!("enable schema-export");
    std::process::exit(2);
}
