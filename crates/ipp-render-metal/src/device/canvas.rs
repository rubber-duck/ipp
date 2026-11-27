use super::{frame::Frame, pipelines::*, uploads, *};
use crate::analytic_surface::{AnalyticDraw, AnalyticMapping};
use std::{mem, ops::Range};

#[derive(Default)]
struct Staging {
    vertices: Vec<CanvasVertex>,
    paints: Vec<CanvasPaint>,
    bindings: Vec<[u32; 4]>,
    mapping: Option<AnalyticMapping>,
    intersections: Vec<AnalyticDraw>,
    edges: Vec<[f32; 4]>,
    parameters: Vec<[f32; 4]>,
}

impl Staging {
    fn slices(&self) -> [&[u8]; 7] {
        [
            bytemuck::cast_slice(&self.vertices),
            bytemuck::cast_slice(&self.paints),
            bytemuck::cast_slice(&self.bindings),
            self.mapping.as_ref().map_or(&[], |m| bytemuck::bytes_of(m)),
            bytemuck::cast_slice(&self.intersections),
            bytemuck::cast_slice(&self.edges),
            bytemuck::cast_slice(&self.parameters),
        ]
    }

    fn clear(&mut self) {
        self.vertices.clear();
        self.paints.clear();
        self.bindings.clear();
        self.mapping = None;
        self.intersections.clear();
        self.edges.clear();
        self.parameters.clear();
    }
}

struct Packed {
    buffer: Buffer,
    ranges: [Range<usize>; 7],
}

#[derive(Default)]
pub(crate) struct Batch {
    stage: Staging,
    packed: Option<Packed>,
    color: Option<Texture>,
    mask: Option<Texture>,
    path: Option<Buffer>,
    program: Option<Rc<Programs>>,
    mode: usize,
    depth: bool,
    analytic: bool,
    strip: bool,
    count: u32,
    vertex_count: usize,
}

fn same<T>(a: &Retained<T>, b: &Retained<T>) -> bool
where
    T: objc2::Message,
{
    std::ptr::eq(&**a, &**b)
}

impl Batch {
    fn reset(&mut self) {
        self.stage.clear();
        self.count = 0;
        self.vertex_count = 0;
        self.path = None;
        self.packed = None;
    }

    fn bytes(&self) -> usize {
        self.packed.as_ref().map_or_else(
            || self.stage.slices().iter().map(|s| s.len()).sum(),
            |p| p.buffer.length(),
        ) + 1024
    }

    fn encode(&mut self, frame: &mut Frame, record: bool) -> bool {
        if self.count == 0 {
            return true;
        }
        let mut transient = None;
        if self.packed.is_none() {
            let streams = self.stage.slices();
            if streams.iter().any(|s| s.len() > uploads::INLINE_LIMIT) {
                let mut end = 0;
                let ranges = std::array::from_fn(|i| {
                    let start = uploads::aligned(end);
                    end = start + streams[i].len();
                    start..end
                });
                let allocation = if record {
                    frame
                        .dev
                        .gpu
                        .newBufferWithLength_options(end, MTLResourceOptions::StorageModeShared)
                        .map(|b| (b, 0))
                } else {
                    frame.uploads.reserve(&frame.dev, end)
                };
                let Some((buffer, base)) = allocation else {
                    return false;
                };
                let packed = Packed {
                    buffer,
                    ranges: ranges.map(|r| r.start + base..r.end + base),
                };
                for (bytes, r) in streams.iter().zip(&packed.ranges) {
                    if !bytes.is_empty() {
                        uploads::write(&packed.buffer, r.start, bytes);
                    }
                }
                if record {
                    self.packed = Some(packed);
                    self.stage = Staging::default();
                } else {
                    transient = Some(packed);
                }
            }
        }
        self.draw_batch(frame, self.packed.as_ref().or(transient.as_ref()))
    }

    fn draw_batch(&self, frame: &mut Frame, packed: Option<&Packed>) -> bool {
        let encoder = frame
            .encoder
            .as_ref()
            .expect("Canvas requires a render pass");
        let pipeline = self
            .program
            .as_deref()
            .unwrap_or(&frame.dev.programs)
            .get(self.mode, frame.is_root);
        let state = (self.mode, Retained::as_ptr(pipeline) as usize);
        if frame.state != Some(state) {
            encoder.setRenderPipelineState(pipeline);
            encoder.setCullMode(MTLCullMode::None);
            frame.state = Some(state);
        }
        encoder.setDepthStencilState(Some(
            &frame.dev.depth[if self.depth {
                1
            } else {
                2
            }],
        ));

        let streams = self.stage.slices();
        let bind = |i: usize, is_vertex: bool, slot: usize, default: bool| {
            if let Some(p) = packed
                && !p.ranges[i].is_empty()
            {
                let offset = p.ranges[i].start;
                if is_vertex {
                    uploads::vertex(encoder, slot, &p.buffer, offset)
                } else {
                    uploads::fragment(encoder, slot, &p.buffer, offset)
                };
            } else if !streams[i].is_empty() {
                uploads::inline(encoder, is_vertex, slot, streams[i])
            } else if default {
                uploads::inline(encoder, is_vertex, slot, &[0; 16])
            }
        };
        bind(0, true, 0, false);
        bind(1, false, 1, false);
        bind(2, false, 6, false);
        if self.analytic {
            bind(3, false, 8, false);
            bind(3, true, 2, false);
            bind(4, false, 5, false);
            bind(4, true, 1, false);
            bind(5, false, 7, true);
            uploads::fragment(
                encoder,
                9,
                frame.columns.as_ref().unwrap_or(&frame.dev.empty_columns),
                0,
            );
        }
        bind(6, false, 3, true);
        uploads::texture(encoder, 0, self.color.as_ref().unwrap());
        uploads::texture(encoder, 2, self.mask.as_ref().unwrap());
        if let Some(path) = &self.path {
            uploads::fragment(encoder, 2, path, 0)
        } else {
            uploads::inline(encoder, false, 2, &[0; 16])
        }
        // SAFETY: Each strip instance addresses four prepared corners or one analytic cover.
        // A clipped triangle list uses a single instance and its own vertex count.
        unsafe {
            if self.strip {
                encoder.drawPrimitives_vertexStart_vertexCount_instanceCount(
                    MTLPrimitiveType::TriangleStrip,
                    0,
                    4,
                    self.count as usize,
                );
            } else {
                encoder.drawPrimitives_vertexStart_vertexCount(
                    MTLPrimitiveType::Triangle,
                    0,
                    self.vertex_count,
                );
            }
        }
        frame.merged += self.count - 1;
        true
    }
}

impl Frame {
    pub(super) fn flush(&mut self) -> bool {
        if self.failed || self.batch.count == 0 {
            return !self.failed;
        }
        let mut batch = mem::take(&mut self.batch);
        let record = self.recording.is_some();
        if !batch.encode(self, record) {
            self.failed = true;
            return false;
        }
        if let Some(r) = &mut self.recording {
            r.push(batch)
        } else {
            batch.reset();
            self.batch = batch;
        }
        true
    }

    pub(crate) fn begin_canvas_record(&mut self) -> bool {
        if self.recording.is_some() || !self.flush() {
            return false;
        }
        self.recording = Some(Vec::new());
        true
    }

    pub(crate) fn end_canvas_record(&mut self) -> Option<(Object, usize)> {
        let ok = self.flush();
        let batches = self.recording.take()?;
        let bytes = batches.iter().map(Batch::bytes).sum();
        ok.then_some((Object::Recording(batches), bytes))
    }

    pub(crate) fn replay_canvas(&mut self, obj: &Object) -> bool {
        if !self.flush() {
            return false;
        }
        let Object::Recording(batches) = obj else {
            return false;
        };
        for b in batches {
            if !b.draw_batch(self, b.packed.as_ref()) {
                self.failed = true;
                return false;
            }
        }
        true
    }

    pub(crate) fn canvas(
        &mut self,
        vertices: &[CanvasVertex],
        paint: &CanvasPaint,
        binding: Option<CanvasBinding<'_>>,
        custom: Option<PaintParameters<'_>>,
        cover: CanvasCoverage<'_>,
    ) -> bool {
        let analytic = cover.analytic;
        if self.failed
            || analytic.is_some() == !vertices.is_empty()
            || (analytic.is_some() && self.layered)
        {
            return false;
        }
        let screen = matches!(binding, Some(CanvasBinding::Screen(_)));
        let (image, owner, path, start, layer) = match binding {
            Some(CanvasBinding::Bitmap(o)) => (Some(o.texture()), None, None, 0, 0),
            Some(CanvasBinding::Screen(t) | CanvasBinding::Target(t)) => {
                (Some(&t.color), t.owner.as_ref(), None, 0, 0)
            }
            Some(CanvasBinding::Layer {
                target: t,
                id,
            }) => (
                Some(&t.color),
                t.owner.as_ref(),
                None,
                0,
                id.saturating_add(1),
            ),
            Some(CanvasBinding::Path {
                buffer,
                start,
            }) => (None, None, Some(buffer.buffer()), start, 0),
            None => (None, None, None, 0, 0),
        };
        let dev = self.dev.clone();
        let color = image.unwrap_or(&dev.white);
        let mask = owner.unwrap_or(&dev.empty_owner);
        let prog = custom.as_ref().map(|v| v.program.program());
        let params = custom.as_ref().map_or(&[][..], |v| v.values);
        if size_of_val(params) > uploads::INLINE_LIMIT {
            return false;
        }
        let cached = if cover.plot
            && cover.depth_test
            && self.is_root
            && prog.is_none()
            && self.recording.is_none()
        {
            path.and_then(|page| self.path_masks.visit(vertices, page, start, paint.options))
        } else {
            None
        };
        let path = cached.as_ref().map(|(buffer, _, _)| buffer).or(path);
        let mode = if screen && analytic.is_some_and(|a| a.mapping.uses_columns()) {
            COLUMN_SCREEN_PAINT
        } else if screen {
            SCREEN_PAINT
        } else if cached.is_some() {
            MASKED_PATH
        } else if self.layered {
            LAYERS
        } else if let Some(a) = analytic {
            if a.mapping.is_cylinder() && image.is_none() && (path.is_none() || prog.is_none()) {
                match (a.mapping.uses_columns(), path.is_some()) {
                    (true, true) => COLUMN_PATH,
                    (true, false) => COLUMN_SHAPE,
                    (false, true) => CYLINDER_PATH,
                    (false, false) => CYLINDER_SHAPE,
                }
            } else if path.is_some() && prog.is_none() {
                PATH
            } else {
                ANALYTIC
            }
        } else if image.is_some() && prog.is_none() {
            IMAGE
        } else {
            AFFINE
        };
        let strip = analytic.is_some() || vertices.len() == 4;
        let b = &self.batch;
        if b.count > 0
            && (b.count >= 64
                || (b.vertex_count + vertices.len()) * size_of::<CanvasVertex>() > 128 * 1024
                || b.strip != strip
                || b.mode != mode
                || b.depth != cover.depth_test
                || !same(b.color.as_ref().unwrap(), color)
                || !same(b.mask.as_ref().unwrap(), mask)
                || b.path.as_ref().zip(path).is_some_and(|(a, b)| !same(a, b))
                || b.program.as_ref().map(Rc::as_ptr) != prog.map(Rc::as_ptr)
                || b.stage.mapping.as_ref() != analytic.map(|a| a.mapping)
                || (analytic.is_none() && b.stage.parameters != params))
            && !self.flush()
        {
            return false;
        }
        let b = &mut self.batch;
        if b.count == 0 {
            b.color = Some(color.clone());
            b.mask = Some(mask.clone());
            b.program = prog.cloned();
            b.mode = mode;
            b.strip = strip;
            b.depth = cover.depth_test;
            b.analytic = analytic.is_some();
            b.stage.mapping = analytic.map(|a| *a.mapping);
            if analytic.is_none() {
                b.stage.parameters.extend_from_slice(params)
            }
        }
        if b.path.is_none() {
            b.path = path.cloned()
        }
        let slot = b.count;
        b.stage
            .vertices
            .extend(vertices.iter().map(|v| CanvasVertex {
                slot,
                ..*v
            }));
        b.vertex_count += vertices.len();
        let mut paint = *paint;
        if let Some((_, rect, height)) = &cached {
            paint.fill[0] = *rect;
            paint.fill[1][0] = *height as f32;
        }
        b.stage.paints.push(paint);
        let mut first_edge = 0;
        let mut offset = 0;
        if let Some(a) = analytic {
            first_edge = b.stage.edges.len() as u32 + 1;
            b.stage.edges.extend_from_slice(&a.edges);
            b.stage.intersections.push(a.draw);
            if !params.is_empty() {
                if !b.stage.parameters.ends_with(params) {
                    b.stage.parameters.extend_from_slice(params)
                }
                offset = (b.stage.parameters.len() - params.len()) as u32;
            }
        }
        b.stage
            .bindings
            .push([cover.layer_owner, layer, start, first_edge | (offset << 16)]);
        b.count += 1;
        true
    }
}
