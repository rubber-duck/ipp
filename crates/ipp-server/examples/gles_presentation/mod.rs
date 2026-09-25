//! GLES presentation of the testing host: platform services, the loopback
//! presentation channel and the diagnostics statistics a capture reports.
//!
//! Like the browser worker, the host presents the single World most recently
//! attached by a connection and forgets the previous World's renderer caches.
//! Ordinary frames render without readback; a frame request is answered after
//! the presented World's first frame at or after its tick, and only a capture
//! reads pixels back and collects statistics.

mod channel;
mod services;
mod statistics;

pub(crate) use services::GlesHostServices;

use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender};

use channel::{PresentationControl, PresentationOutput};

/// Pbuffer size, which bounds the viewport limits the host reports.
const PBUFFER_SIZE: u32 = 2048;

/// Inputs [`GlesHostServices`] takes when the Host initializes it.
struct PresentationSetup {
    egl_directory: PathBuf,
    control: Receiver<PresentationControl>,
    output: Sender<PresentationOutput>,
}

/// Handoff from `main` to Host initialization, which runs inside the server loop.
static SETUP: Mutex<Option<PresentationSetup>> = Mutex::new(None);

/// Start the presentation channel on `listener` and select the EGL libraries
/// the Host services load when the server initializes them.
pub(crate) fn prepare(
    egl_directory: PathBuf,
    listener: TcpListener,
) -> Result<(), Box<dyn std::error::Error>> {
    let (control, control_receiver) = mpsc::channel();
    let (output, output_receiver) = mpsc::channel();
    channel::spawn(listener, control, output_receiver)?;
    *SETUP
        .lock()
        .map_err(|_| "presentation setup lock poisoned")? = Some(PresentationSetup {
        egl_directory,
        control: control_receiver,
        output,
    });
    Ok(())
}

fn take_setup() -> Result<PresentationSetup, String> {
    SETUP
        .lock()
        .map_err(|_| "presentation setup lock poisoned".to_owned())?
        .take()
        .ok_or_else(|| "the GLES presentation was not prepared".to_owned())
}
