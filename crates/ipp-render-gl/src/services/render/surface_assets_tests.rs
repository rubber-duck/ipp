use super::*;
use ipp_core::services::data_source::MemoryDataReader;

#[derive(Default)]
struct Device {
    fail_path: bool,
}

impl RenderDevice for Device {
    type Program = ();
    type Mesh = ();
    type Texture = ();
    type SurfacePath = ();
    type SurfaceCacheTarget = ();
    type SurfaceInstances = ();
    #[cfg(feature = "shadows")]
    type ShadowMap = ();
    #[cfg(feature = "gui")]
    type GuiBatch = ();
    #[cfg(feature = "gui")]
    type GlyphAtlasPage = ();

    #[cfg(feature = "gui")]
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

    #[cfg(feature = "shadows")]
    fn create_shadow_map(&mut self, _: u32) -> Result<(), crate::RenderError> {
        Ok(())
    }

    #[cfg(feature = "shadows")]
    fn begin_shadow(&mut self, _: &(), _: u32, _: u32) -> Result<(), crate::RenderError> {
        Ok(())
    }

    #[cfg(feature = "shadows")]
    fn end_shadow(&mut self) -> Result<(), crate::RenderError> {
        Ok(())
    }

    #[cfg(feature = "shadows")]
    fn bind_shadow(
        &mut self,
        _: &(),
        _: &(),
        _: &crate::RenderLightingFrame,
    ) -> Result<(), crate::RenderError> {
        Ok(())
    }

    #[cfg(feature = "shadows")]
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
        Ok(())
    }

    fn create_surface_path(
        &mut self,
        _: &[f32; 4],
        _: &[[f32; 8]],
        _: &[[u32; 2]],
    ) -> Result<(), crate::RenderError> {
        if self.fail_path {
            Err(crate::RenderError::RenderDevice("injected failure".into()))
        } else {
            Ok(())
        }
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
        #[cfg(feature = "mesh-poses")] _: Option<(&(), f32)>,
        _: Option<&()>,
    ) -> Result<(), crate::RenderError> {
        Ok(())
    }

    fn end_frame(&mut self) -> Result<(), crate::RenderError> {
        Ok(())
    }

    fn delete_mesh(&mut self, _: ()) {}
    fn delete_texture(&mut self, _: ()) {}
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
    }));
    let mut loader = drawing_loader(device, Rc::new(Cell::new(0)));
    let mut reader = MemoryDataReader::new(bytes);
    let waker = std::task::Waker::noop();
    let mut context = std::task::Context::from_waker(waker);
    let result = match loader.poll_load(&mut reader, &mut context) {
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
