use super::{
    canvas::Batch,
    pipelines::{PLOT_ALPHA, PLOT_OPAQUE},
    resources::{Submission, command_error, region},
    uploads::{self, Uploads},
    *,
};
use crate::{analytic_surface::AnalyticMapping, lighting::Lighting};
use objc2_quartz_core::CAMetalDrawable;
use std::ptr::NonNull;

pub(crate) struct Frame {
    pub(super) dev: Rc<Backend>,
    pub(super) cmd: Gpu<dyn MTLCommandBuffer>,
    pub(super) encoder: Option<Gpu<dyn MTLRenderCommandEncoder>>,
    root: Target,
    pub(crate) target_size: [u32; 2],
    pub(super) is_root: bool,
    pub(super) layered: bool,
    shadow_pass: bool,
    pub(super) state: Option<(usize, usize)>,
    pub(super) batch: Batch,
    pub(crate) canvas_vertices: Vec<CanvasVertex>,
    pub(super) recording: Option<Vec<Batch>>,
    pub(super) columns: Option<Buffer>,
    pub(super) uploads: Uploads,
    pub(super) merged: u32,
    pub(super) failed: bool,
}

impl Frame {
    pub(super) fn new(dev: Rc<Backend>) -> Result<Self, String> {
        dev.retire(false)?;
        let root = dev
            .root
            .borrow()
            .clone()
            .ok_or("Metal viewport not configured")?;
        let cmd = dev.queue.commandBuffer().ok_or("No Metal command buffer")?;
        #[cfg(feature = "instrumentation")]
        cmd.setLabel(Some(objc2_foundation::ns_string!("IPP root output")));
        Ok(Self {
            dev,
            target_size: root.size,
            root,
            cmd,
            encoder: None,
            is_root: false,
            layered: false,
            shadow_pass: false,
            state: None,
            batch: Default::default(),
            canvas_vertices: Vec::new(),
            recording: None,
            columns: None,
            uploads: Default::default(),
            merged: 0,
            failed: false,
        })
    }

    fn close_pass(&mut self) -> bool {
        let ok = self.flush();
        if let Some(e) = self.encoder.take() {
            e.endEncoding();
        }
        self.state = None;
        ok
    }

    pub(crate) fn presentation_extent(&self) -> Option<[u32; 2]> {
        self.is_root.then_some(self.root.size)
    }

    pub(crate) fn pass(
        &mut self,
        target: Option<&Target>,
        background: [f32; 4],
        lighting: Option<&Lighting>,
        shadow: Option<&Object>,
    ) -> bool {
        self.render_pass(target, background, lighting, shadow, None)
    }

    pub(crate) fn repaint_canvas(
        &mut self,
        target: &Target,
        lighting: &Lighting,
        rect: [u32; 4],
    ) -> bool {
        self.render_pass(Some(target), [0.; 4], Some(lighting), None, Some(rect))
    }

    fn render_pass(
        &mut self,
        target: Option<&Target>,
        bg: [f32; 4],
        lights: Option<&Lighting>,
        shadow: Option<&Object>,
        damage: Option<[u32; 4]>,
    ) -> bool {
        if !self.close_pass() {
            return false;
        }
        self.is_root = target.is_none();
        let t = target.unwrap_or(&self.root);
        self.target_size = t.size;
        self.layered = t.layered();
        self.shadow_pass = false;
        let pass = MTLRenderPassDescriptor::new();
        // SAFETY: The attachments match the dimensions and formats used to create the pipelines.
        unsafe {
            let c = pass.colorAttachments().objectAtIndexedSubscript(0);
            c.setTexture(Some(&t.color));
            c.setLoadAction(if damage.is_some() {
                MTLLoadAction::Load
            } else {
                MTLLoadAction::Clear
            });
            c.setStoreAction(MTLStoreAction::Store);
            c.setClearColor(MTLClearColor {
                red: bg[0] as f64,
                green: bg[1] as f64,
                blue: bg[2] as f64,
                alpha: bg[3] as f64,
            });
            if let Some(owner) = &t.owner {
                let c = pass.colorAttachments().objectAtIndexedSubscript(1);
                c.setTexture(Some(owner));
                c.setLoadAction(MTLLoadAction::Clear);
                c.setStoreAction(MTLStoreAction::Store);
                c.setClearColor(MTLClearColor {
                    red: 0.,
                    green: 0.,
                    blue: 0.,
                    alpha: 0.,
                });
            }
        }
        let depth = pass.depthAttachment();
        depth.setTexture(Some(&t.depth));
        depth.setLoadAction(MTLLoadAction::Clear);
        depth.setStoreAction(MTLStoreAction::DontCare);
        depth.setClearDepth(1.);
        let Some(e) = self.cmd.renderCommandEncoderWithDescriptor(&pass) else {
            return false;
        };
        e.setDepthStencilState(Some(&self.dev.depth[0]));
        e.setFrontFacingWinding(MTLWinding::CounterClockwise);
        e.setCullMode(MTLCullMode::Back);
        uploads::inline(
            &e,
            false,
            0,
            bytemuck::bytes_of(lights.unwrap_or(&Lighting::default())),
        );
        uploads::texture(&e, 1, shadow.map_or(&self.dev.empty_depth, Object::texture));
        if let Some([x, y, w, h]) = damage {
            e.setScissorRect(MTLScissorRect {
                x: x as usize,
                y: y as usize,
                width: w as usize,
                height: h as usize,
            });
            e.setRenderPipelineState(&self.dev.clear);
            e.setDepthStencilState(Some(&self.dev.depth[2]));
            e.setCullMode(MTLCullMode::None);
            // SAFETY: The clear shader synthesizes a full-screen triangle from three vertex IDs.
            unsafe { e.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 3) };
        }
        self.encoder = Some(e);
        true
    }

    pub(crate) fn shadow(&mut self, texture: &Object) -> bool {
        if !self.close_pass() {
            return false;
        }
        self.is_root = false;
        self.layered = false;
        self.shadow_pass = true;
        let pass = MTLRenderPassDescriptor::new();
        let depth = pass.depthAttachment();
        depth.setTexture(Some(texture.texture()));
        depth.setLoadAction(MTLLoadAction::Clear);
        depth.setStoreAction(MTLStoreAction::Store);
        depth.setClearDepth(1.);
        let Some(e) = self.cmd.renderCommandEncoderWithDescriptor(&pass) else {
            return false;
        };
        e.setDepthStencilState(Some(&self.dev.depth[0]));
        e.setFrontFacingWinding(MTLWinding::CounterClockwise);
        e.setCullMode(MTLCullMode::Back);
        self.encoder = Some(e);
        true
    }

    pub(crate) fn shadow_tile(&mut self, x: u32, y: u32, size: u32) {
        let e = self.encoder.as_ref().unwrap();
        e.setViewport(MTLViewport {
            originX: x as f64,
            originY: y as f64,
            width: size as f64,
            height: size as f64,
            znear: 0.,
            zfar: 1.,
        });
        e.setScissorRect(MTLScissorRect {
            x: x as usize,
            y: y as usize,
            width: size as usize,
            height: size as usize,
        });
    }

    pub(crate) fn lighting(&mut self, lights: &Lighting) {
        if self.flush() {
            uploads::inline(
                self.encoder.as_ref().unwrap(),
                false,
                0,
                bytemuck::bytes_of(lights),
            )
        }
    }

    pub(crate) fn canvas_columns(&mut self, maps: &[AnalyticMapping], extent: [u32; 2]) -> bool {
        let raw = bytemuck::cast_slice::<_, u8>(maps);
        if maps.is_empty()
            || extent.contains(&0)
            || raw.len() > uploads::INLINE_LIMIT
            || !self.flush()
        {
            return false;
        }
        let mut cache = self.dev.maps.borrow_mut();
        for entry in cache.iter_mut().rev() {
            if entry.extent != extent || entry.mappings != maps {
                continue;
            }
            if entry
                .producer
                .as_ref()
                .is_some_and(|cmd| cmd.status() != MTLCommandBufferStatus::Completed)
            {
                continue;
            }
            // Once ready, the cache owns the column buffer independently of its producer.
            entry.producer = None;
            self.columns = Some(entry.buffer.clone());
            return true;
        }
        let mut prev = self.dev.previous.borrow_mut();
        if prev.1 != extent || prev.0 != maps {
            prev.0.clear();
            prev.0.extend_from_slice(maps);
            prev.1 = extent;
            return false;
        }
        let stride = extent[0] as usize + 2;
        let Some(b) = self.dev.gpu.newBufferWithLength_options(
            64 * stride * maps.len(),
            MTLResourceOptions::StorageModePrivate,
        ) else {
            return false;
        };
        if let Some(e) = self.encoder.take() {
            e.endEncoding();
        }
        let Some(e) = self.cmd.computeCommandEncoder() else {
            return false;
        };
        #[cfg(feature = "instrumentation")]
        e.setLabel(Some(objc2_foundation::ns_string!("IPP cylinder columns")));
        e.setComputePipelineState(&self.dev.columns);
        // SAFETY: Uniforms are copied immediately. Buffer capacity covers every dispatched column and mapping.
        unsafe {
            e.setBytes_length_atIndex(NonNull::from_ref(&raw[0]).cast(), raw.len(), 0);
            e.setBuffer_offset_atIndex(Some(&b), 0, 1);
            e.setBytes_length_atIndex(NonNull::from_ref(&extent).cast(), 8, 2);
        }
        e.dispatchThreads_threadsPerThreadgroup(
            MTLSize {
                width: stride,
                height: maps.len(),
                depth: 1,
            },
            MTLSize {
                width: self.dev.columns.threadExecutionWidth(),
                height: 1,
                depth: 1,
            },
        );
        e.endEncoding();
        if cache.len() == 4 {
            cache.pop_front();
        }
        cache.push_back(super::resources::ColumnMap {
            mappings: maps.to_vec(),
            extent,
            buffer: b.clone(),
            producer: Some(self.cmd.clone()),
        });
        self.columns = Some(b);
        true
    }

    pub(crate) fn draw(
        &mut self,
        streams: MeshStreams<'_>,
        texture: Option<&Object>,
        materials: &[Material],
        blend: Blend,
        custom: Option<MeshParameters<'_>>,
    ) -> bool {
        if !self.flush() {
            return false;
        }
        let e = self.encoder.as_ref().unwrap();
        if materials.is_empty()
            || materials.len() > Material::MAX_INSTANCES
            || streams.joints.is_empty()
            || custom.as_ref().is_some_and(|draw| {
                draw.textures.len() > 8 || size_of_val(draw.values) > uploads::INLINE_LIMIT
            })
        {
            return false;
        }
        let prog = custom.as_ref().map(|p| p.program.program());
        let p = prog.map_or(&self.dev.programs, Rc::as_ref);
        let mode = blend as usize;
        // The pipeline stays alive through the command buffer even if a shader asset is released.
        let pipeline = if self.shadow_pass {
            p.shadow
                .as_ref()
                .unwrap_or_else(|| self.dev.programs.shadow.as_ref().unwrap())
        } else {
            p.get(
                if prog.is_some() {
                    mode.min(1)
                } else if materials[0].options[1] == -1.0 {
                    if blend == Blend::Opaque {
                        PLOT_OPAQUE
                    } else {
                        PLOT_ALPHA
                    }
                } else {
                    mode
                },
                self.is_root,
            )
        };
        let state = (mode, Retained::as_ptr(pipeline) as usize);
        if self.state != Some(state) {
            e.setRenderPipelineState(pipeline);
            e.setDepthStencilState(Some(
                &self.dev.depth[usize::from(!self.shadow_pass && mode != 0)],
            ));
            e.setCullMode(MTLCullMode::Back);
            self.state = Some(state);
        }
        let mesh = streams.mesh;
        uploads::vertex(e, 0, mesh.vertices.buffer(), 0);
        uploads::vertex(e, 2, streams.pose.unwrap_or(mesh).vertices.buffer(), 0);
        for (slot, bytes) in [
            (1, bytemuck::cast_slice::<_, u8>(materials)),
            (3, bytemuck::cast_slice(streams.joints)),
        ] {
            if bytes.len() > uploads::INLINE_LIMIT {
                let Some((b, offset)) = self.uploads.reserve(&self.dev, bytes.len()) else {
                    return false;
                };
                uploads::write(&b, offset, bytes);
                uploads::vertex(e, slot, &b, offset);
            } else {
                uploads::inline(e, true, slot, bytes)
            }
        }
        if !self.shadow_pass {
            uploads::texture(e, 0, texture.map_or(&self.dev.white, Object::texture));
        }
        if let Some(c) = custom {
            if self.shadow_pass {
                uploads::texture(e, 1, &self.dev.empty_depth);
            }
            let bytes = bytemuck::cast_slice(if c.values.is_empty() {
                &[[0u32; 4]]
            } else {
                c.values
            });
            uploads::inline(e, true, 4, bytes);
            uploads::inline(e, false, 4, bytes);
            for (i, t) in c.textures.iter().enumerate() {
                // SAFETY: Each declared material slot refers to a live Metal texture from this device.
                unsafe { e.setVertexTexture_atIndex(Some(t.texture()), i + 2) };
                uploads::texture(e, i + 2, t.texture());
            }
        }
        // SAFETY: Mesh upload validates indices and vertex ranges. The instance range exactly matches the material records.
        unsafe {
            if let Some(idx) = &mesh.indices {
                e.drawIndexedPrimitives_indexCount_indexType_indexBuffer_indexBufferOffset_instanceCount(
                    MTLPrimitiveType::Triangle,
                    mesh.count,
                    MTLIndexType::UInt16,
                    idx.buffer(),
                    0,
                    materials.len(),
                )
            } else {
                e.drawPrimitives_vertexStart_vertexCount_instanceCount(
                    MTLPrimitiveType::Triangle,
                    0,
                    mesh.count,
                    materials.len(),
                )
            }
        }
        true
    }

    fn present_layer(&self) -> Result<(), String> {
        let layer = self.dev.layer.borrow();
        let Some((layer, pipeline)) = layer.as_ref() else {
            return Ok(());
        };
        let Some(drawable) = layer.nextDrawable() else {
            // Drawable starvation is transient. The root frame and any readback still complete.
            return Ok(());
        };
        let pass = MTLRenderPassDescriptor::new();
        // SAFETY: This pipeline draws to one BGRA8 sRGB attachment owned by the acquired drawable.
        let c = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
        c.setTexture(Some(&drawable.texture()));
        c.setLoadAction(MTLLoadAction::DontCare);
        c.setStoreAction(MTLStoreAction::Store);
        let e = self
            .cmd
            .renderCommandEncoderWithDescriptor(&pass)
            .ok_or("No drawable encoder")?;
        e.setRenderPipelineState(pipeline);
        uploads::texture(&e, 0, &self.root.color);
        // SAFETY: Three procedural vertices cover the drawable. The root texture is shader-readable.
        unsafe { e.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 3) };
        e.endEncoding();
        self.cmd
            .presentDrawable(ProtocolObject::from_ref(&*drawable));
        Ok(())
    }

    pub(crate) fn finish(
        mut self,
        capture: Option<&mut [u8]>,
        gpu_seconds: Option<&mut f64>,
    ) -> Result<u32, String> {
        if !self.close_pass() {
            return Err("Could not encode Metal Canvas batch".into());
        }
        let [w, h] = self.root.size.map(|v| v as usize);
        let row = w * 4;
        let stride = uploads::aligned(row);
        let readback = if let Some(pixels) = capture.as_deref() {
            if pixels.len() != row * h {
                return Err("Invalid Metal readback extent".into());
            }
            let b = self
                .dev
                .gpu
                .newBufferWithLength_options(stride * h, MTLResourceOptions::StorageModeShared)
                .ok_or("No readback buffer")?;
            let e = self.cmd.blitCommandEncoder().ok_or("No readback encoder")?;
            let r = region(w, h);
            // SAFETY: Stride is Metal-aligned, the destination covers every row, and the texture extent matches.
            unsafe {
                e.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toBuffer_destinationOffset_destinationBytesPerRow_destinationBytesPerImage(
                    &self.root.color,
                    0,
                    0,
                    r.origin,
                    r.size,
                    &b,
                    0,
                    stride,
                    stride * h,
                )
            };
            e.endEncoding();
            Some(b)
        } else {
            None
        };
        self.present_layer()?;
        self.dev.retire(true)?;
        self.cmd.commit();
        if capture.is_some() || gpu_seconds.is_some() {
            self.cmd.waitUntilCompleted();
            self.dev.retire(false)?;
            if self.cmd.status() != MTLCommandBufferStatus::Completed {
                return Err(command_error(&self.cmd));
            }
            if let (Some(pixels), Some(b)) = (capture, readback) {
                for (y, out) in pixels.chunks_exact_mut(row).enumerate() {
                    // SAFETY: All GPU writes have completed. The mapped allocation covers every copied row.
                    unsafe {
                        out.as_mut_ptr().copy_from_nonoverlapping(
                            b.contents().cast::<u8>().as_ptr().add(y * stride),
                            row,
                        )
                    }
                }
            } else if let Some(seconds) = gpu_seconds {
                let start = self.cmd.GPUStartTime();
                let end = self.cmd.GPUEndTime();
                if start > 0. && end > start {
                    *seconds = end - start;
                }
            }
            self.dev.recycle(self.uploads.used);
        } else {
            self.dev.submitted.borrow_mut().push_back(Submission {
                commands: self.cmd,
                uploads: self.uploads.used,
            });
        }
        Ok(self.merged)
    }
}
