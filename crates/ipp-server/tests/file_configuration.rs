//! The production executable validates explicit filesystem configuration before startup.
#![cfg(feature = "websocket")]
use std::process::Command;

#[test]
fn filesystem_access_requires_a_pair_and_a_real_directory() {
    for arguments in [
        vec!["--file-root", "."],
        vec!["--file-prefix", "files://"],
        vec![
            "--file-root",
            "missing-ipp-fixture-directory",
            "--file-prefix",
            "files://",
        ],
        vec!["--file-root", ".", "--file-prefix", ""],
        vec!["--file-root", ".", "--file-prefix", "asset-memory:"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_ipp-server"))
            .args(arguments)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(
            output.stdout.is_empty(),
            "invalid configuration must not announce readiness"
        );
    }
}
