//! GLES surface integration and diagnostics controls. Authoring sessions never select it.

mod channel;
#[cfg(feature = "instrumentation")]
mod profiling;
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
    read_sources: Vec<(String, PathBuf)>,
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
    read_sources: Vec<(String, PathBuf)>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (control, control_receiver) = mpsc::channel();
    let (output, output_receiver) = mpsc::channel();
    channel::spawn(listener, control, output_receiver)?;
    *SETUP
        .lock()
        .map_err(|_| "presentation setup lock poisoned")? = Some(PresentationSetup {
        egl_directory,
        read_sources,
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
