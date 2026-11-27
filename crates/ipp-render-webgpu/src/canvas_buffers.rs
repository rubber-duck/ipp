//! Reusable vertex and parameter storage and render-pass encoding for a Canvas batch.
use crate::{
    canvas::Draws,
    canvas_device::{CanvasPipeline, RecordBuffer},
    device::{Graphics, Target},
    service::{FrameSummary, RenderError},
};

#[derive(Clone, Copy, PartialEq)]
pub(super) enum CanvasLoad {
    Load,
    Root,
    Image,
    Owner,
}

#[derive(Default)]
pub(super) struct CanvasBuffers {
    vertices: Option<wgpu::Buffer>,
    records: Option<RecordBuffer>,
    previous_records: Vec<[f32; 4]>,
    previous_vertices: Vec<u8>,
    masks: crate::path_masks::PathMasks,
}

impl CanvasBuffers {
    #[allow(clippy::too_many_arguments)]
    pub fn encode(
        &mut self,
        gpu: &Graphics,
        encoder: &mut wgpu::CommandEncoder,
        target: &Target,
        draws: &Draws,
        load: CanvasLoad,
        scissor: Option<[u32; 4]>,
        white: &wgpu::BindGroup,
        empty_path: &RecordBuffer,
        summary: &mut FrameSummary,
        upload: bool,
        forward: Option<wgpu::RenderBundle>,
    ) -> Result<(), RenderError> {
        let load = if forward.is_some() {
            CanvasLoad::Root
        } else {
            load
        };
        let clear = load != CanvasLoad::Load;
        summary.failed += draws.failed;
        if draws.runs.is_empty() && !clear && scissor.is_none() {
            return Ok(());
        }
        let bytes = bytemuck::cast_slice(&draws.vertices);
        if bytes.len() as u64 > gpu.device.limits().max_buffer_size {
            return Err(RenderError::Draw(
                "Canvas vertex buffer exceeds device limit",
            ));
        }
        let mut changed = false;
        if upload && !bytes.is_empty() {
            if self
                .vertices
                .as_ref()
                .is_none_or(|buffer| buffer.size() < bytes.len() as u64)
            {
                self.previous_vertices.clear();
                self.vertices = Some(
                    gpu.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("Canvas triangles"),
                        size: (bytes.len() as u64)
                            .next_power_of_two()
                            .min(gpu.device.limits().max_buffer_size),
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    }),
                );
            }
            if bytes != self.previous_vertices {
                changed = true;
                gpu.queue
                    .write_buffer(self.vertices.as_ref().unwrap(), 0, bytes);
                self.previous_vertices.clear();
                self.previous_vertices.extend_from_slice(bytes);
            }
            let count = draws.records.len();
            let limit = RecordBuffer::limit(&gpu.device);
            if count > limit {
                return Err(RenderError::Draw(
                    "Canvas records exceed the storage buffer limit",
                ));
            }
            if self
                .records
                .as_ref()
                .is_none_or(|buffer| buffer.capacity() < count)
            {
                self.previous_records.clear();
                self.records = Some(RecordBuffer::new(
                    gpu,
                    0,
                    count.next_power_of_two().min(limit),
                ));
            }
            let table = self.records.as_ref().unwrap();
            let unchanged = |i: usize, value: &[[f32; 4]]| {
                self.previous_records.get(i..i + value.len()) == Some(value)
            };
            // Combine adjacent changed blocks into one queue write.
            const BLOCK: usize = 1024;
            let mut first = None;
            for (i, row) in draws.records.chunks(BLOCK).enumerate() {
                let start = i * BLOCK;
                if unchanged(start, row) {
                    if let Some(begin) = first.take() {
                        table.write(&gpu.queue, begin, &draws.records[begin..start]);
                    }
                } else {
                    changed = true;
                    first.get_or_insert(start);
                }
            }
            if let Some(begin) = first {
                table.write(&gpu.queue, begin, &draws.records[begin..]);
            }
            changed |= self.previous_records.len() != count;
            self.previous_records.clone_from(&draws.records);
        }

        if draws.mask_pixels > 0 && upload && forward.is_some() && gpu.canvas.masks.is_some() {
            self.masks.prepare(
                gpu,
                encoder,
                target,
                self.vertices.as_ref().unwrap(),
                &self.records.as_ref().unwrap().bind,
                draws,
                changed,
                summary,
            );
        } else {
            self.masks = Default::default();
        }

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Canvas composition"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: if load == CanvasLoad::Owner {
                    target.owner.as_ref().unwrap()
                } else {
                    &target.view
                },
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: match load {
                        // Layer IDs start at zero. Reserve all ones for unpainted pixels.
                        CanvasLoad::Owner => wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        CanvasLoad::Image => wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        CanvasLoad::Root => wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.003095975,
                            g: 0.004400849,
                            b: 0.007194409,
                            a: 1.0,
                        }),
                        CanvasLoad::Load => wgpu::LoadOp::Load,
                    },
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &target.depth,
                depth_ops: Some(wgpu::Operations {
                    // Opaque Camera draws and depth-tested Surfaces share this pass.
                    // A subsequent viewport overlay only needs the accumulated color.
                    load: wgpu::LoadOp::Clear(1.),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        if let Some(bundle) = &forward {
            pass.execute_bundles(std::iter::once(bundle));
        }
        if let Some([x, y, w, h]) = scissor
            && w > 0
            && h > 0
        {
            pass.set_scissor_rect(x, y, w, h);
            pass.set_pipeline(&gpu.canvas.clear);
            pass.draw(0..3, 0..1);
            summary.draws += 1;
            summary.triangles += 1;
        }
        if !draws.runs.is_empty() {
            pass.set_vertex_buffer(0, self.vertices.as_ref().unwrap().slice(..));
            pass.set_bind_group(0, &self.records.as_ref().unwrap().bind, &[]);
            for run in &draws.runs {
                let program = run.program.as_ref().unwrap_or(&gpu.canvas.ordinary);
                let kind = if load == CanvasLoad::Owner {
                    CanvasPipeline::Owner
                } else {
                    run.pipeline
                };
                let mask = self
                    .masks
                    .read
                    .as_ref()
                    .filter(|_| run.masked && kind == CanvasPipeline::DepthTested);
                pass.set_pipeline(if mask.is_some() {
                    &gpu.canvas.masks.as_ref().unwrap().read
                } else {
                    &program[kind]
                });
                pass.set_bind_group(1, run.image.as_ref().unwrap_or(white), &[]);
                pass.set_bind_group(
                    2,
                    mask.or(run.path.as_ref()).unwrap_or(&empty_path.bind),
                    &[],
                );
                pass.draw(run.start..run.end, 0..1);
                summary.draws += 1;
                summary.triangles += (run.end - run.start) / 3;
            }
        }
        Ok(())
    }
}
