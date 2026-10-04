//! Real GLES staging/fence/copy evidence independent of World/frame submission.

#[cfg(target_os = "linux")]
#[allow(dead_code)]
#[path = "smoke/egl.rs"]
mod egl;

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use ipp_render_gl::RenderDevice;
    use std::{
        path::Path,
        time::{Duration, Instant},
    };

    let directory = std::env::args()
        .nth(1)
        .ok_or("usage: egl_texture_readback <EGL/GLES directory>")?;
    let context = egl::Context::new(Path::new(&directory), 8, 8)?;
    let mut device = context.device()?;
    // Deliberately asymmetric rows, channels and straight alpha. Pixel storage
    // readback must preserve authored top-row-first sRGB bytes without a flip.
    let pixels = [
        17, 89, 231, 41, 199, 51, 103, 255, 11, 208, 75, 0, 93, 14, 181, 117, 240, 123, 7, 201, 62,
        154, 219, 63,
    ];
    let texture = device.create_texture(3, 2, &pixels)?;
    let staging = device.begin_texture_readback(&texture, 3, 2)?;
    assert!(
        device
            .copy_texture_readback(&staging, 0, &mut [0; 4])
            .is_err()
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while !device.poll_texture_readback(&staging)? {
        assert!(
            Instant::now() < deadline,
            "fence must progress without a draw or swap"
        );
        std::thread::park_timeout(Duration::from_millis(2));
    }
    let mut output = [0; 24];
    for (index, destination) in output.chunks_mut(7).enumerate() {
        device.copy_texture_readback(&staging, index * 7, destination)?;
    }
    assert_eq!(
        output, pixels,
        "row orientation, encoded color and straight alpha"
    );
    assert!(
        device
            .copy_texture_readback(&staging, 23, &mut [0; 2])
            .is_err()
    );
    device.delete_texture_readback(staging);
    // Cancellation before a fence poll must release private staging as well.
    let cancelled = device.begin_texture_readback(&texture, 3, 2)?;
    device.delete_texture_readback(cancelled);
    device.delete_texture(texture);
    println!(
        "GLES pixel-pack/fence readback passed: asymmetric3x2, no frame, bounded copies, cancellation"
    );
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    panic!("GLES readback evidence requires the maintained Linux EGL environment");
}
