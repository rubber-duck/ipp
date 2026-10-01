//! Diagnostics-only loopback channel. Frames and pixels use the common Host protocol.

use std::io;
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::Duration;

use tungstenite::{Error, Message, WebSocket};

/// Longest diagnostic request: kind and two u32 testing parameters.
const MAX_REQUEST_BYTES: usize = 9;

/// Idle wait between socket reads, which bounds output latency.
const POLL_INTERVAL: Duration = Duration::from_millis(1);

/// A decoded client request, applied by the Host at its next frame.
#[derive(Debug, PartialEq)]
pub(super) enum PresentationControl {
    Statistics {
        id: u32,
    },
    GlyphAtlasLimits {
        max_pages: u32,
        idle_page_frames: u32,
    },
    SurfaceCacheBudget {
        bytes: u32,
    },
    ExhaustiveDrawChecks {
        enabled: bool,
    },
    ContextLoss,
    ContextRestore,
}

/// A Host message for the connected client.
pub(super) enum PresentationOutput {
    ViewportLimits {
        max_width: u32,
        max_height: u32,
    },
    Statistics {
        id: u32,
        snapshot: String,
    },
    /// A request this build cannot honour; the client fails its connection.
    Failure(String),
}

pub(super) fn decode(bytes: &[u8]) -> Result<PresentationControl, String> {
    let u32_at = |offset: usize| -> Result<u32, String> {
        bytes
            .get(offset..offset + 4)
            .map(|word| u32::from_le_bytes(word.try_into().expect("four bytes")))
            .ok_or_else(|| "truncated presentation request".to_owned())
    };
    let flag_at = |offset: usize| match bytes.get(offset) {
        Some(0) => Ok(false),
        Some(1) => Ok(true),
        _ => Err("invalid diagnostics flag".to_owned()),
    };

    let (control, length) = match bytes.first() {
        Some(9) => (
            PresentationControl::Statistics {
                id: u32_at(1)?,
            },
            5,
        ),
        Some(4) => (
            PresentationControl::GlyphAtlasLimits {
                max_pages: u32_at(1)?,
                idle_page_frames: u32_at(5)?,
            },
            9,
        ),
        Some(5) => (
            PresentationControl::SurfaceCacheBudget {
                bytes: u32_at(1)?,
            },
            5,
        ),
        Some(6) => (
            PresentationControl::ExhaustiveDrawChecks {
                enabled: flag_at(1)?,
            },
            2,
        ),
        Some(7) => (PresentationControl::ContextLoss, 1),
        Some(8) => (PresentationControl::ContextRestore, 1),
        _ => return Err("unknown presentation request".to_owned()),
    };
    if bytes.len() != length {
        return Err("presentation request has trailing bytes".to_owned());
    }

    Ok(control)
}

pub(super) fn encode(output: PresentationOutput) -> Vec<u8> {
    match output {
        PresentationOutput::ViewportLimits {
            max_width,
            max_height,
        } => [
            &[1][..],
            &max_width.to_le_bytes(),
            &max_height.to_le_bytes(),
        ]
        .concat(),
        PresentationOutput::Statistics {
            id,
            snapshot,
        } => [&[5][..], &id.to_le_bytes(), snapshot.as_bytes()].concat(),
        PresentationOutput::Failure(message) => [&[4][..], message.as_bytes()].concat(),
    }
}

/// Serve presentation clients on a dedicated thread until the Host exits.
pub(super) fn spawn(
    listener: TcpListener,
    control: Sender<PresentationControl>,
    output: Receiver<PresentationOutput>,
) -> io::Result<()> {
    listener.set_nonblocking(true)?;
    std::thread::Builder::new()
        .name("ipp-gles-presentation".into())
        .spawn(move || serve(&listener, &control, &output))?;
    Ok(())
}

fn serve(
    listener: &TcpListener,
    control: &Sender<PresentationControl>,
    output: &Receiver<PresentationOutput>,
) {
    // Limits are reported when they change; a later client receives the latest.
    let mut limits = None;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                if let Err(error) = client(stream, control, output, &mut limits) {
                    eprintln!("gles_host: presentation client failed: {error}");
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                // Without a client, keep only the latest limits.
                loop {
                    match output.try_recv() {
                        Ok(
                            message @ PresentationOutput::ViewportLimits {
                                ..
                            },
                        ) => limits = Some(encode(message)),
                        Ok(_) => {}
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => return,
                    }
                }
                std::thread::sleep(POLL_INTERVAL);
            }
            Err(error) => {
                eprintln!("gles_host: presentation listener failed: {error}");
                return;
            }
        }
    }
}

fn client(
    stream: TcpStream,
    control: &Sender<PresentationControl>,
    output: &Receiver<PresentationOutput>,
    limits: &mut Option<Vec<u8>>,
) -> Result<(), String> {
    stream
        .set_nonblocking(false)
        .and_then(|()| stream.set_write_timeout(Some(Duration::from_secs(10))))
        .and_then(|()| stream.set_read_timeout(Some(Duration::from_secs(10))))
        .map_err(|error| error.to_string())?;
    let mut socket = tungstenite::accept(stream).map_err(|error| error.to_string())?;
    // Short reads let one thread alternate between requests and output.
    socket
        .get_ref()
        .set_read_timeout(Some(POLL_INTERVAL))
        .map_err(|error| error.to_string())?;
    if let Some(bytes) = limits.clone() {
        send(&mut socket, bytes)?;
    }

    loop {
        match socket.read() {
            Ok(Message::Binary(bytes)) => {
                if bytes.len() > MAX_REQUEST_BYTES {
                    return Err("oversized presentation request".into());
                }
                let request = decode(&bytes)?;
                control
                    .send(request)
                    .map_err(|_| "the Host stopped".to_owned())?;
            }
            Ok(Message::Close(_)) | Err(Error::ConnectionClosed | Error::AlreadyClosed) => {
                return Ok(());
            }
            Ok(Message::Ping(_) | Message::Pong(_)) => {}
            Ok(Message::Text(_) | Message::Frame(_)) => {
                return Err("binary presentation requests required".into());
            }
            Err(Error::Io(error))
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(error) => return Err(error.to_string()),
        }

        loop {
            match output.try_recv() {
                Ok(message) => {
                    let limit = matches!(message, PresentationOutput::ViewportLimits { .. });
                    let bytes = encode(message);
                    if limit {
                        *limits = Some(bytes.clone());
                    }
                    send(&mut socket, bytes)?;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return Ok(()),
            }
        }
    }
}

fn send(socket: &mut WebSocket<TcpStream>, bytes: Vec<u8>) -> Result<(), String> {
    socket
        .send(Message::Binary(bytes.into()))
        .map_err(|error| error.to_string())
}
