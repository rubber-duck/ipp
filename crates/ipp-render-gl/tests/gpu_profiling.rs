//! Public capture metadata and bounded unavailable records; real GL scenarios own device evidence.
mod support;

use ipp_render_gl::{
    RenderGpuAvailability, RenderGpuCapability, RenderGpuSampling, RenderGpuScope, RenderService,
};
use std::rc::Rc;
use support::{DeviceState, TestDevice};

#[test]
fn unsupported_queries_preserve_rendering_and_bound_capture_records() {
    let mut renderer = RenderService::new(TestDevice(Rc::new(DeviceState::default()))).unwrap();
    assert_eq!(
        renderer.start_gpu_capture(17, 29, RenderGpuSampling::Frame),
        Ok(RenderGpuCapability::Unsupported)
    );
    let viewport = ipp_core::WorldViewport {
        width: 64,
        height: 64,
        device_pixel_ratio: 1.0,
    };
    for _ in 0..300 {
        renderer.clear(viewport).unwrap();
    }
    let samples = renderer.gpu_samples();
    assert_eq!(samples.len(), 256);
    assert_eq!(renderer.gpu_dropped_records(), 44);
    for (index, sample) in samples.iter().enumerate() {
        assert_eq!(sample.scope, RenderGpuScope::Frame);
        assert_eq!(sample.availability, RenderGpuAvailability::Unsupported);
        assert_eq!((sample.identity.capture, sample.identity.host), (17, 29));
        assert_eq!(sample.identity.frame, index as u64 + 1);
        assert_eq!(
            (sample.identity.world, sample.identity.surface),
            (None, None)
        );
    }
    assert_eq!(renderer.drain_gpu_samples(), samples);
    assert!(renderer.gpu_samples().is_empty());
    renderer.clear(viewport).unwrap();
    assert_eq!(renderer.drain_gpu_samples()[0].identity.frame, 301);
    renderer.stop_gpu_capture();
    renderer.clear(viewport).unwrap();
    assert!(renderer.gpu_samples().is_empty());
}

#[test]
fn replacement_context_uses_new_generation_and_prior_samples_keep_origin() {
    let mut host = support::task_scheduler::host();
    let mut renderer = RenderService::new(TestDevice(Rc::new(DeviceState::default()))).unwrap();
    let viewport = ipp_core::WorldViewport {
        width: 32,
        height: 32,
        device_pixel_ratio: 1.0,
    };
    renderer
        .start_gpu_capture(1, host.identity(), RenderGpuSampling::Frame)
        .unwrap();
    renderer.clear(viewport).unwrap();
    let first = renderer.drain_gpu_samples()[0];
    renderer
        .replace_device(&mut host, TestDevice(Rc::new(DeviceState::default())))
        .unwrap();
    renderer
        .start_gpu_capture(2, host.identity(), RenderGpuSampling::Frame)
        .unwrap();
    renderer.clear(viewport).unwrap();
    let second = renderer.drain_gpu_samples()[0];
    assert_eq!(first.identity.context, 1);
    assert_eq!(second.identity.context, 2);
    assert_eq!(first.identity.capture, 1);
    assert_eq!(second.identity.capture, 2);
    assert!(second.identity.frame > first.identity.frame);
}

#[test]
fn simulated_loss_preserves_terminal_records_and_requires_new_capture() {
    let mut renderer = RenderService::new(TestDevice(Rc::new(DeviceState::default()))).unwrap();
    let viewport = ipp_core::WorldViewport {
        width: 32,
        height: 32,
        device_pixel_ratio: 1.0,
    };
    renderer
        .start_gpu_capture(1, 2, RenderGpuSampling::Frame)
        .unwrap();
    renderer.clear(viewport).unwrap();
    let original = renderer.gpu_samples();
    renderer.invalidate_gpu_context();
    renderer.clear(viewport).unwrap();
    assert_eq!(renderer.gpu_samples(), original);
    assert_eq!(renderer.drain_gpu_samples()[0].identity.context, 1);
    renderer
        .start_gpu_capture(3, 2, RenderGpuSampling::Frame)
        .unwrap();
    renderer.clear(viewport).unwrap();
    assert_eq!(renderer.drain_gpu_samples()[0].identity.context, 2);
}
