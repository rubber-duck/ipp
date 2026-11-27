//! Retain exact four-sample coverage for repeated paths projected into the root Camera.
use super::{resources::Backend, uploads, *};
use ipp_render_scene::path_masks::reserve;

struct ProjectedPath {
    vertices: Box<[CanvasVertex]>,
    page: Buffer,
    start: u32,
    mode: f32,
    radius: f32,
    rect: [f32; 4],
    end: u32,
    height: u32,
    vertex_bytes_end: usize,
}

#[derive(Default)]
pub(super) struct PathMasks {
    paths: Vec<ProjectedPath>,
    samples: Option<Buffer>,
    producer: Option<Gpu<dyn MTLCommandBuffer>>,
    size: [u32; 2],
    cursor: usize,
    changed: bool,
}

impl PathMasks {
    pub fn begin(&mut self, size: [u32; 2]) {
        if size != self.size {
            self.paths.clear();
            self.samples = None;
            self.producer = None;
            self.size = size;
        }
        if let Some(producer) = &self.producer {
            match producer.status() {
                MTLCommandBufferStatus::Completed => self.producer = None,
                MTLCommandBufferStatus::Error => {
                    self.producer = None;
                    self.samples = None;
                }
                _ => {}
            }
        }

        self.cursor = 0;
        self.changed = false;
    }

    pub fn visit(
        &mut self,
        vertices: &[CanvasVertex],
        page: &Buffer,
        start: u32,
        options: [f32; 4],
    ) -> Option<(Buffer, [f32; 4], u32)> {
        if vertices.is_empty() || size_of_val(vertices) > uploads::INLINE_LIMIT {
            return None;
        }

        if let Some(path) = self.paths.get(self.cursor)
            && std::ptr::eq(&*path.page, &**page)
            && path.start == start
            && path.mode == options[1]
            && path.radius == options[3]
            && bytemuck::cast_slice::<_, u8>(&path.vertices) == bytemuck::cast_slice(vertices)
        {
            self.cursor += 1;
            return self
                .samples
                .as_ref()
                .filter(|_| self.producer.is_none())
                .map(|b| (b.clone(), path.rect, path.height));
        }

        // Tiny overlapping paths can occupy little screen space. Bound their CPU
        // vertex copies independently of the GPU sample budget.
        let previous = self.paths[..self.cursor].last();
        let vertex_bytes_end = previous.map_or(0, |p| p.vertex_bytes_end) + size_of_val(vertices);
        if vertex_bytes_end > 4 << 20 {
            return None;
        }

        let mut end = previous.map_or(0, |p| p.end);
        let rect = reserve(vertices, self.size, &mut end)?;
        self.paths.truncate(self.cursor);
        self.paths.push(ProjectedPath {
            vertices: vertices.into(),
            page: page.clone(),
            start,
            mode: options[1],
            radius: options[3],
            rect,
            height: (end - rect[3] as u32) / rect[2] as u32,
            end,
            vertex_bytes_end,
        });
        self.cursor += 1;
        self.changed = true;
        self.producer = None;
        self.samples = None;
        None
    }

    pub fn finish(
        &mut self,
        dev: &Backend,
        cmd: &Gpu<dyn MTLCommandBuffer>,
        target: &Target,
    ) -> (u32, u32) {
        self.paths.truncate(self.cursor);
        let Some(last) = self.paths.last() else {
            self.samples = None;
            self.producer = None;
            return (0, 0);
        };
        if self.changed || self.samples.is_some() {
            return (0, 0);
        }

        // This frame already drew analytically. Populate an immutable buffer for
        // later submissions on the same queue, independent of current occluders.
        let Some(buffer) = dev.gpu.newBufferWithLength_options(
            last.end as usize * size_of::<f32>(),
            MTLResourceOptions::StorageModePrivate,
        ) else {
            return (0, 0);
        };
        let pass = MTLRenderPassDescriptor::new();
        let depth = pass.depthAttachment();
        depth.setTexture(Some(&target.depth));
        depth.setLoadAction(MTLLoadAction::DontCare);
        depth.setStoreAction(MTLStoreAction::DontCare);
        let Some(encoder) = cmd.renderCommandEncoderWithDescriptor(&pass) else {
            return (0, 0);
        };
        encoder.setRenderPipelineState(&dev.write_path_mask);
        uploads::fragment(&encoder, 3, &buffer, 0);

        let mut triangles = 0;
        for path in &self.paths {
            uploads::inline(&encoder, true, 0, bytemuck::cast_slice(&path.vertices));
            uploads::inline(&encoder, false, 1, bytemuck::bytes_of(&path.rect));
            uploads::inline(
                &encoder,
                false,
                4,
                bytemuck::bytes_of(&[path.mode, path.radius, path.height as f32, 0.0]),
            );
            uploads::fragment(&encoder, 2, &path.page, path.start as usize * 16);

            let count = path.vertices.len();
            let topology = if count == 4 {
                triangles += 2;
                MTLPrimitiveType::TriangleStrip
            } else {
                triangles += count as u32 / 3;
                MTLPrimitiveType::Triangle
            };
            // SAFETY: The encoder copied this primitive's vertices, and the immutable
            // page owns its path records. Each primitive writes its own reserved pixel range.
            unsafe { encoder.drawPrimitives_vertexStart_vertexCount(topology, 0, count) };
        }
        encoder.endEncoding();
        self.samples = Some(buffer);
        self.producer = Some(cmd.clone());
        (self.paths.len() as u32, triangles)
    }
}

#[cfg(test)]
#[path = "path_masks_tests.rs"]
mod tests;
