use super::*;
use ipp_core::services::io::IoService;
use ipp_host_session::services::task_scheduler::TaskSchedulerService;
use std::num::NonZeroUsize;

#[test]
fn generic_filesystem_reader_writer_and_listing_use_real_files() {
    let tasks = TaskSchedulerService::new();
    let root = std::env::temp_dir().join(format!("ipp-file-source-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let mut sources = IoService::new();
    sources
        .register(
            "disk:",
            FileSystemIoSource::new("disk:", &root, true, tasks.schedulers().io()).unwrap(),
        )
        .unwrap();
    let bytes = vec![42; 180_000];
    futures_lite::future::block_on(async {
        let mut writer = sources
            .open_write("disk:world.ippw", bytes.len())
            .await
            .unwrap();
        let mut offset = 0;
        while offset < bytes.len() {
            offset += writer.write(&bytes[offset..]).await.unwrap();
        }
        writer.flush().await.unwrap();
        writer.finish().await.unwrap();
        let mut listing = sources.list("disk:").await.unwrap();
        assert_eq!(
            listing.next().await.unwrap(),
            Some("disk:world.ippw".into())
        );
        assert_eq!(listing.next().await.unwrap(), None);
        let mut reader = sources
            .open_read(
                "disk:world.ippw",
                IoReadOptions {
                    max_bytes: Some(bytes.len()),
                    recovery: false,
                },
            )
            .await
            .unwrap();
        let mut result = Vec::new();
        loop {
            let window = reader
                .read(NonZeroUsize::new(100_000).unwrap())
                .await
                .unwrap();
            let count = window.bytes().len();
            let done = window.is_final();
            result.extend_from_slice(window.bytes());
            window.consume(count).unwrap();
            if done {
                break;
            }
        }
        assert_eq!(result, bytes);
        assert!(
            sources
                .open_read(
                    "disk:../outside",
                    IoReadOptions {
                        max_bytes: None,
                        recovery: false
                    }
                )
                .await
                .is_err()
        );
        assert!(!sources.can_write("disk:../outside"));
        assert!(sources.open_write("disk:/absolute", 128).await.is_err());
        assert!(
            sources
                .open_read(
                    "disk:world.ippw",
                    IoReadOptions {
                        max_bytes: Some(bytes.len()),
                        recovery: true
                    }
                )
                .await
                .is_err()
        );
    });
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn filesystem_root_prefix_preserves_path_boundaries_and_listing() {
    let tasks = TaskSchedulerService::new();
    let root = std::env::temp_dir().join(format!("ipp-file-prefix-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("bar.mesh"), [1, 2, 3]).unwrap();
    let prefix = "file://path/foo";
    let mut source = FileSystemIoSource::new(prefix, &root, true, tasks.schedulers().io()).unwrap();
    futures_lite::future::block_on(async {
        let mut listing = source.list(prefix).await.unwrap();
        assert_eq!(
            listing.next().await.unwrap(),
            Some("file://path/foo/bar.mesh".into())
        );
        assert_eq!(listing.next().await.unwrap(), None);
    });
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
    let tasks = TaskSchedulerService::new();
    let root = std::env::temp_dir().join(format!("ipp-file-symlink-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(std::env::temp_dir(), root.join("escape")).unwrap();
    let mut source =
        FileSystemIoSource::new("disk:", &root, true, tasks.schedulers().io()).unwrap();
    futures_lite::future::block_on(async {
        assert!(source.open_write("disk:escape/outside", 128).await.is_err());
        assert!(
            source
                .open_read(
                    "disk:escape/",
                    IoReadOptions {
                        max_bytes: None,
                        recovery: false
                    }
                )
                .await
                .is_err()
        );
    });
    std::fs::remove_dir_all(root).unwrap();
}
