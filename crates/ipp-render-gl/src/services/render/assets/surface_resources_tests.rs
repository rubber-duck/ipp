use super::*;
use ipp_core::services::io::BufferIoReader;
use std::{rc::Rc, task::Poll};

#[derive(Default)]
struct WakeCount(std::sync::atomic::AtomicUsize);

impl std::task::Wake for WakeCount {
    fn wake(self: std::sync::Arc<Self>) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

#[derive(Default)]
struct Device {
    fail_path: bool,
    deleted_paths: usize,
    textures: usize,
    uploaded_rows: usize,
    deleted_textures: usize,
}

impl RenderDevice for Device {
    fn viewport_limits(&self) -> Option<crate::ViewportLimits> {
        None
    }

    type Program = ();
    type Mesh = ();
    type Texture = ();
    type TextureReadback = ();
    type SurfacePath = ();
    type SurfaceCacheTarget = ();
    type SurfaceInstances = ();
    type ShadowMap = ();
    type GuiBatch = ();
    type GlyphAtlasPage = ();

    fn glyph_atlas_texture(page: &Self::GlyphAtlasPage) -> &Self::Texture {
        page
    }

    fn set_lighting(
        &mut self,
        _: &(),
        _: &[f32; 16],
        _: &[f32; 16],
        _: &[f32; 3],
        _: &crate::RenderLightingFrame,
    ) -> Result<(), crate::RenderError> {
        Ok(())
    }

    fn create_shadow_map(&mut self, _: u32) -> Result<(), crate::RenderError> {
        Ok(())
    }

    fn begin_shadow(&mut self, _: &(), _: u32, _: u32) -> Result<(), crate::RenderError> {
        Ok(())
    }

    fn end_shadow(&mut self) -> Result<(), crate::RenderError> {
        Ok(())
    }

    fn bind_shadow(
        &mut self,
        _: &(),
        _: &(),
        _: &crate::RenderLightingFrame,
    ) -> Result<(), crate::RenderError> {
        Ok(())
    }

    fn delete_shadow_map(&mut self, _: ()) {}

    fn create_program(&mut self, _: &str, _: &str) -> Result<(), crate::RenderError> {
        Ok(())
    }

    fn create_mesh(&mut self, _: &ipp_core::MeshAsset) -> Result<(), crate::RenderError> {
        Ok(())
    }

    fn create_texture(&mut self, _: u32, _: u32, _: &[u8]) -> Result<(), crate::RenderError> {
        Ok(())
    }

    fn allocate_texture(&mut self, _: u32, _: u32) -> Result<(), crate::RenderError> {
        self.textures += 1;
        Ok(())
    }

    fn upload_texture_rows(
        &mut self,
        _: &(),
        _: u32,
        _: u32,
        _: u32,
        _: &[u8],
    ) -> Result<(), crate::RenderError> {
        self.uploaded_rows += 1;
        Ok(())
    }

    fn create_surface_path(
        &mut self,
        _: &crate::SurfacePathTexels,
    ) -> Result<(), crate::RenderError> {
        if self.fail_path {
            Err(crate::RenderError::RenderDevice("injected failure".into()))
        } else {
            Ok(())
        }
    }

    fn delete_surface_path(&mut self, _: ()) {
        self.deleted_paths += 1;
    }

    fn begin_frame(&mut self, _: u32, _: u32, _: &[f32; 4]) -> Result<(), crate::RenderError> {
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn draw(
        &mut self,
        _: &(),
        _: &(),
        _: &[f32; 16],
        _: &[f32; 3],
        _: Option<(&(), f32)>,
        _: Option<&()>,
    ) -> Result<(), crate::RenderError> {
        Ok(())
    }

    fn end_frame(&mut self) -> Result<(), crate::RenderError> {
        Ok(())
    }

    fn delete_mesh(&mut self, _: ()) {}
    fn delete_texture(&mut self, _: ()) {
        self.deleted_textures += 1;
    }
    fn delete_program(&mut self, _: ()) {}
}

fn drawing(contours: bool) -> Vec<u8> {
    let mut bytes = b"IPPD".to_vec();
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    for value in [0.0_f32, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.01] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&(u32::from(contours)).to_le_bytes());
    if contours {
        bytes.extend_from_slice(&[255, 0, 0, 255, 0, 0, 0, 0]);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&0.0_f32.to_le_bytes());
        bytes.extend_from_slice(&0.0_f32.to_le_bytes());
        bytes.extend_from_slice(&2_u32.to_le_bytes());
        for point in [[1.0_f32, 0.0], [0.0, 1.0]] {
            bytes.extend_from_slice(&[0, 0, 0, 0]);
            for value in point {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
    bytes
}

fn load(
    fail_path: bool,
    bytes: Vec<u8>,
) -> (
    Result<GlDrawingData<Device>, String>,
    Option<GlDrawingData<Device>>,
) {
    let device = Rc::new(std::cell::RefCell::new(Device {
        fail_path,
        ..Default::default()
    }));
    let mut loader = drawing_loader(device, Default::default(), Default::default());
    loader
        .start_load(Box::new(BufferIoReader::new(bytes)))
        .unwrap();
    let waker = std::task::Waker::from(std::sync::Arc::new(WakeCount::default()));
    let mut context = std::task::Context::from_waker(&waker);
    let result = match loader.poll_load(&mut context) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("memory drawing load unexpectedly pending"),
    };
    let failed = loader.take_failed_data();
    (result, failed)
}

#[test]
fn empty_drawing_is_ready_without_a_gpu_allocation() {
    let (result, failed) = load(false, drawing(false));
    let asset = result.unwrap();
    assert!(failed.is_none());
    assert!(asset.path.is_none());
    assert_eq!(asset.graphics_ready(), Some(true));
    assert_eq!(asset.graphics_bytes(), Some(0));
}

#[test]
fn gpu_failure_retains_decoded_drawing_as_failed_data() {
    let (result, failed) = load(true, drawing(true));
    assert!(result.is_err());
    let asset = failed.expect("decoded asset retained after GPU failure");
    assert_eq!(asset.drawing.layers().len(), 1);
    assert_eq!(asset.drawing.layers()[0].contours.len(), 1);
    assert_eq!(asset.graphics_ready(), Some(false));
    assert!(asset.path.is_none());
}

#[test]
fn lost_generation_drop_never_deletes_handles_in_a_replacement_context() {
    let device = Rc::new(std::cell::RefCell::new(Device::default()));
    let context = super::super::context::RenderAssetContext::default();
    let mut loader = drawing_loader(device.clone(), Default::default(), context.clone());
    loader
        .start_load(Box::new(BufferIoReader::new(drawing(true))))
        .unwrap();
    let count = std::sync::Arc::new(WakeCount::default());
    let waker = std::task::Waker::from(count);
    let mut cx = std::task::Context::from_waker(&waker);
    let Poll::Ready(Ok(asset)) = loader.poll_load(&mut cx) else {
        panic!("small fixture did not complete")
    };
    assert!(asset.path.is_some());
    context.set_active(false);
    context.set_active(true);
    drop(asset);
    assert_eq!(device.borrow().deleted_paths, 0);

    let mut loader = drawing_loader(device.clone(), Default::default(), context);
    loader
        .start_load(Box::new(BufferIoReader::new(drawing(true))))
        .unwrap();
    let Poll::Ready(Ok(asset)) = loader.poll_load(&mut cx) else {
        panic!("small fixture did not complete")
    };
    drop(asset);
    assert_eq!(device.borrow().deleted_paths, 1);
}

fn texture() -> Vec<u8> {
    let mut bytes = b"IPPT".to_vec();
    for word in [3u32, 2, 2] {
        bytes.extend(word.to_le_bytes());
    }
    bytes.extend([255u8; 16]);
    bytes
}

#[test]
fn cancelling_partial_texture_in_live_context_deletes_private_gpu_storage() {
    use ipp_core::services::io::{IoReadOptions, StreamIoReader};
    let device = Rc::new(std::cell::RefCell::new(Device::default()));
    let context = super::super::context::RenderAssetContext::default();
    let mut loader =
        super::super::loaders::texture_asset_loader(device.clone(), Default::default(), context);
    let (reader, input) = StreamIoReader::new(IoReadOptions {
        max_bytes: None,
        recovery: false,
    });
    loader.start_load(Box::new(reader)).unwrap();
    let waker = std::task::Waker::from(std::sync::Arc::new(WakeCount::default()));
    let mut cx = std::task::Context::from_waker(&waker);
    assert!(loader.poll_load(&mut cx).is_pending());
    assert!(input.push(&texture()[..24]).unwrap());
    assert!(loader.poll_load(&mut cx).is_pending());
    assert_eq!(device.borrow().uploaded_rows, 1);
    drop(loader);
    assert_eq!(device.borrow().deleted_textures, 1);
    assert!(!input.is_open());
}

#[test]
fn context_loss_during_texture_input_never_resumes_old_gpu_names() {
    use ipp_core::services::io::{IoReadOptions, StreamIoReader};
    let device = Rc::new(std::cell::RefCell::new(Device::default()));
    let context = super::super::context::RenderAssetContext::default();
    let mut loader = super::super::loaders::texture_asset_loader(
        device.clone(),
        Default::default(),
        context.clone(),
    );
    let (reader, input) = StreamIoReader::new(IoReadOptions {
        max_bytes: None,
        recovery: false,
    });
    loader.start_load(Box::new(reader)).unwrap();
    let waker = std::task::Waker::from(std::sync::Arc::new(WakeCount::default()));
    let mut cx = std::task::Context::from_waker(&waker);
    assert!(loader.poll_load(&mut cx).is_pending());
    assert!(input.push(&texture()[..24]).unwrap());
    assert!(loader.poll_load(&mut cx).is_pending());
    assert_eq!(device.borrow().uploaded_rows, 1);
    context.set_active(false);
    context.set_active(true);
    assert!(input.push(&texture()[24..]).unwrap());
    input.finish(Ok(()));
    assert!(matches!(loader.poll_load(&mut cx), Poll::Ready(Err(_))));
    assert_eq!(device.borrow().uploaded_rows, 1);
    assert_eq!(device.borrow().deleted_textures, 0);

    // Recovery starts a new acquisition/decoder rather than resuming old names.
    let mut reopened =
        super::super::loaders::texture_asset_loader(device.clone(), Default::default(), context);
    reopened
        .start_load(Box::new(BufferIoReader::new(texture())))
        .unwrap();
    let Poll::Ready(Ok(asset)) = reopened.poll_load(&mut cx) else {
        panic!("reopened texture unavailable")
    };
    assert_eq!(device.borrow().textures, 2);
    assert_eq!(device.borrow().uploaded_rows, 3);
    drop(asset);
    assert_eq!(device.borrow().deleted_textures, 1);
}
