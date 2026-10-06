//! Execute the maintained layout/owned-dispatch fixture on the native target.

fn main() -> std::io::Result<()> {
    use std::io::Write;
    std::io::stdout()
        .lock()
        .write_all(&ipp_protocol::contract::export_layout_fixture())
}
