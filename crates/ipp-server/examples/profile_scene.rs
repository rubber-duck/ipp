//! Native release benchmark using a target-correct saved Blender World and real GLES.

#[cfg(feature = "profiling")]
#[global_allocator]
static ALLOCATOR: ipp_core::profiling::CountingAllocator = ipp_core::profiling::CountingAllocator;

// Share the maintained test-host context loader, never a production device shim.
#[cfg(target_os = "linux")]
#[path = "../../ipp-render-gl/examples/smoke/egl.rs"]
mod egl;

#[cfg(target_os = "linux")]
mod scene_profile;

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    scene_profile::run()
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("Native scene profiling currently requires the Linux EGL test host");
    std::process::exit(1);
}
