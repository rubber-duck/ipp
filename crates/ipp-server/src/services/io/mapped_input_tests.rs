use super::*;
use ipp_core::services::io::IoService;
use ipp_host_session::services::task_scheduler::TaskSchedulerService;
use std::{
    io::{BufRead, BufReader, Write},
    num::NonZeroUsize,
    process::{Command, Stdio},
};

#[test]
fn sealed_process_publication_lends_mapping_after_producer_exit_and_revocation() {
    let tasks = TaskSchedulerService::new();
    let mut producer = Command::new("python3")
        .args([
            "-u",
            "-c",
            r#"
import os,fcntl,sys
fd=os.memfd_create('ipp-mapped-fixture',os.MFD_ALLOW_SEALING)
os.write(fd,bytes(range(256))*4096)
fcntl.fcntl(fd,fcntl.F_ADD_SEALS,fcntl.F_SEAL_WRITE|fcntl.F_SEAL_GROW|fcntl.F_SEAL_SHRINK)
try:
 os.write(fd,b'forbidden')
 raise RuntimeError('sealed producer write succeeded')
except PermissionError:
 pass
print(f'/proc/{os.getpid()}/fd/{fd}',flush=True)
sys.stdin.readline()
"#,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut path = String::new();
    BufReader::new(producer.stdout.take().unwrap())
        .read_line(&mut path)
        .unwrap();
    let file = File::open(path.trim()).unwrap();
    futures_lite::future::block_on(async {
        let source =
            SealedMappedIoSource::new("mapped:fixture".into(), file, tasks.schedulers().io())
                .await
                .unwrap();
        let mut io = IoService::new();
        io.register("mapped:fixture", source).unwrap();
        let options = IoReadOptions {
            max_bytes: None,
            recovery: false,
        };
        let mut first = io.open_read("mapped:fixture", options).await.unwrap();
        let mut second = io.open_read("mapped:fixture", options).await.unwrap();
        let footprint = first.retained_storage().unwrap();
        assert_eq!(footprint.bytes, 0);
        assert_eq!(footprint.mapped_bytes, 1 << 20);
        assert!(footprint.identity == second.retained_storage().unwrap().identity);
        let first_window = first.read(NonZeroUsize::new(512).unwrap()).await.unwrap();
        let second_window = second.read(NonZeroUsize::new(512).unwrap()).await.unwrap();
        assert_eq!(
            first_window.bytes().as_ptr(),
            second_window.bytes().as_ptr()
        );
        producer
            .stdin
            .take()
            .unwrap()
            .write_all(b"release\n")
            .unwrap();
        assert!(producer.wait().unwrap().success());
        assert!(io.unregister("mapped:fixture"));
        assert!(
            first_window
                .bytes()
                .iter()
                .enumerate()
                .all(|(index, byte)| *byte == (index & 255) as u8)
        );
        first_window.consume(17).unwrap();
        drop(second_window);
        assert!(first.read(NonZeroUsize::new(1).unwrap()).await.is_err());
    });
}

#[test]
fn ordinary_readonly_file_is_not_immutable_mapped_publication() {
    let tasks = TaskSchedulerService::new();
    let path = std::env::temp_dir().join(format!("ipp-unsealed-mapping-{}", std::process::id()));
    std::fs::write(&path, b"ordinary mutable backing").unwrap();
    let source = futures_lite::future::block_on(SealedMappedIoSource::new(
        "mapped:unsafe".into(),
        File::open(&path).unwrap(),
        tasks.schedulers().io(),
    ));
    assert!(source.is_err());
    std::fs::remove_file(path).unwrap();
}
