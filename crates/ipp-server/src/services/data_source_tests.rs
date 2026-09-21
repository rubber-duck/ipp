use super::*;
use ipp_core::services::data_source::{DataSourceManagementService, DataWriteJob};
use std::task::Wake;

struct ThreadWake(std::thread::Thread);

impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

fn context() -> Waker {
    Waker::from(Arc::new(ThreadWake(std::thread::current())))
}

#[test]
fn generic_filesystem_reader_writer_and_listing_use_real_files() {
    let root = std::env::temp_dir().join(format!("ipp-file-source-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let mut sources = DataSourceManagementService::new();
    sources
        .register(
            "disk:",
            FileSystemDataSource::new("disk:", &root, true).unwrap(),
        )
        .unwrap();
    let bytes = vec![42; 180_000];
    let waker = context();
    let mut cx = Context::from_waker(&waker);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut write = DataWriteJob::new(
        bytes.clone(),
        sources.open_write("disk:world.ippw", bytes.len()).unwrap(),
    );
    loop {
        match write.poll(&mut cx) {
            Poll::Ready(result) => {
                result.unwrap();
                break;
            }
            Poll::Pending => {
                assert!(std::time::Instant::now() < deadline);
                std::thread::park_timeout(std::time::Duration::from_millis(10));
            }
        }
    }
    assert_eq!(sources.list("disk:").unwrap(), ["disk:world.ippw"]);
    let mut reader = sources
        .open_read(
            "disk:world.ippw",
            DataReadOptions {
                max_bytes: Some(bytes.len()),
                recovery: false,
            },
        )
        .unwrap();
    let mut result = Vec::new();
    loop {
        let mut buffer = [0; 8192];
        match reader.poll_read(&mut cx, &mut buffer) {
            Poll::Ready(Ok(0)) => break,
            Poll::Ready(Ok(count)) => result.extend_from_slice(&buffer[..count]),
            Poll::Ready(Err(error)) => panic!("{error}"),
            Poll::Pending => {
                assert!(std::time::Instant::now() < deadline);
                std::thread::park_timeout(std::time::Duration::from_millis(10));
            }
        }
    }
    assert_eq!(result, bytes);
    assert!(
        sources
            .open_read(
                "disk:../outside",
                DataReadOptions {
                    max_bytes: Some(128),
                    recovery: false
                }
            )
            .is_err()
    );
    assert!(!sources.can_write("disk:../outside"));
    assert!(sources.open_write("disk:/absolute", 128).is_err());
    assert!(
        sources
            .open_read(
                "disk:world.ippw",
                DataReadOptions {
                    max_bytes: Some(bytes.len()),
                    recovery: true
                }
            )
            .is_err()
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn filesystem_root_prefix_preserves_path_boundaries_and_listing() {
    let root = std::env::temp_dir().join(format!("ipp-file-prefix-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("bar.mesh"), [1, 2, 3]).unwrap();
    let prefix = "file://path/foo";
    let mut source = FileSystemDataSource::new(prefix, &root, true).unwrap();
    assert_eq!(source.list(prefix).unwrap(), ["file://path/foo/bar.mesh"]);
    assert_eq!(
        source.path("file://path/foo/bar.mesh", false).unwrap(),
        root.canonicalize().unwrap().join("bar.mesh")
    );
    assert!(source.can_write("file://path/foo/new.ippw"));
    assert!(source.path("file://path/foobar.mesh", false).is_err());
    assert!(source.path("file://path/foo//absolute", true).is_err());
    assert!(source.path("file://path/foo/../outside", true).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn filesystem_symlinks_cannot_escape_the_authorized_root() {
    let root = std::env::temp_dir().join(format!("ipp-file-symlink-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(std::env::temp_dir(), root.join("escape")).unwrap();
    let mut source = FileSystemDataSource::new("disk:", &root, true).unwrap();
    assert!(!source.can_write("disk:escape/outside"));
    assert!(
        source
            .open_read(
                "disk:escape/",
                DataReadOptions {
                    max_bytes: Some(128),
                    recovery: false
                }
            )
            .is_err()
    );
    std::fs::remove_dir_all(root).unwrap();
}
