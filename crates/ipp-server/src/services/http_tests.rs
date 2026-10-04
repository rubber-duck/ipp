use super::*;
use ipp_core::services::io::IoService;
use ipp_host_session::services::task_scheduler::TaskSchedulerService;
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    num::NonZeroUsize,
};

#[test]
fn pooled_http_streams_beyond_convenience_body_limit_and_pins_recovery() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let uri = format!("http://{}/large", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        for request_index in 0..4 {
            let (mut socket, _) = listener.accept().unwrap();
            let mut input = BufReader::new(socket.try_clone().unwrap());
            let mut headers = String::new();
            loop {
                let mut line = String::new();
                input.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                headers.push_str(&line);
            }
            if request_index != 0 {
                assert!(
                    headers
                        .to_ascii_lowercase()
                        .contains("if-match: \"original\"")
                );
            }
            let etag = if request_index >= 2 {
                "changed"
            } else {
                "original"
            };
            let length = 11 * 1024 * 1024;
            write!(socket, "HTTP/1.1 200 OK\r\nContent-Length: {length}\r\nETag: \"{etag}\"\r\nConnection: close\r\n\r\n").unwrap();
            if request_index >= 2 {
                continue;
            }
            for _ in 0..176 {
                socket.write_all(&[37; 65536]).unwrap();
            }
        }
    });
    let tasks = TaskSchedulerService::new();
    let mut io = IoService::new();
    io.register(&uri, HttpIoSource::new(tasks.schedulers().io()).unwrap())
        .unwrap();
    futures_lite::future::block_on(async {
        for recovery in [false, true] {
            let mut reader = io
                .open_read(
                    &uri,
                    IoReadOptions {
                        max_bytes: None,
                        recovery,
                    },
                )
                .await
                .unwrap();
            let mut count = 0;
            loop {
                let window = reader
                    .read(NonZeroUsize::new(131072).unwrap())
                    .await
                    .unwrap();
                let bytes = window.bytes().len();
                assert!(window.bytes().iter().all(|byte| *byte == 37));
                let done = window.is_final();
                window.consume(bytes).unwrap();
                count += bytes;
                if done {
                    break;
                }
            }
            assert_eq!(count, 11 * 1024 * 1024);
        }
        for recovery in [false, true] {
            assert!(
                io.open_read(
                    &uri,
                    IoReadOptions {
                        max_bytes: None,
                        recovery
                    }
                )
                .await
                .is_err()
            );
        }
    });
    server.join().unwrap();
}
