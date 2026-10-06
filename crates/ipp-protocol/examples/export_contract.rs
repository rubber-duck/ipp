//! Execute the native target's compiled contract and write its binary export.

fn main() -> std::io::Result<()> {
    use std::io::Write;
    std::io::stdout()
        .lock()
        .write_all(ipp_protocol::contract::export_contract())
}
