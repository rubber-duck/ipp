//! Custom canvas paints: authored fill functions inside the canvas shape program.
//!
//! A paint is a [shader definition](ipp_core::services::asset_management::formats::shader)
//! with a GLSL ES 3.00 paint body: the statements of one function returning the
//! straight linear RGBA of a box's fill. The [guide](CANVAS_PAINTS.md) owns the
//! authored interface; this module owns how paints enter the canvas program.
//!
//! # Slots and the canvas program
//!
//! The canvas program draws the GUI's shape records: it is `surface_gui.frag` with
//! its `// CANVAS_PAINTS` placeholder replaced by one generated function per
//! admitted paint and a dispatch on the primitive's slot, so painted and ordinary
//! shapes keep drawing in the same batches. Glyphs draw with their own static
//! program and never see paints.
//! Without paints the program is the static source itself, whose own dispatch
//! returns the box's colour. [`CanvasPaintPrograms`] holds at most
//! [`CANVAS_PAINT_SLOTS`] distinct paint sources and rebuilds the
//! program when a paint enters a slot. A paint keeps its slot until another paint
//! needs one and it has not been drawn in the current frame, so a panel scrolling
//! out of view does not recompile the program twice; a ninth paint drawn in one
//! frame finds no slot.
//!
//! The shader provider compiles each body alone before the asset is loaded, so a
//! paint that cannot compile never reaches the canvas program. If the combined
//! program still fails to build, the paints admitted since the last successful
//! build are rejected and the program is rebuilt without them.
//!
//! # Parameter blocks
//!
//! Every paint instance of a canvas, one `CanvasPaint` component, owns a block of
//! consecutive vectors in the program's `u_paint_blocks` array of
//! [`CANVAS_PAINT_VECTORS`]: one vector per declared parameter in name order, with
//! a scalar in `x` and a vector from `x`. [`CanvasPaintBlocks`] keeps each
//! instance's block in place while its parameter count is unchanged, and the
//! primitive carries the block's offset beside its slot. Property values are packed
//! into the blocks every frame the canvas is prepared and uploaded before its GUI
//! draws when they changed, so a property write or animation never regenerates
//! geometry. The array is sized well within the 224 fragment uniform vectors that
//! WebGL 2 and GLES 3 guarantee; the static program uses none of them.
//!
//! # Failure
//!
//! An instance whose paint is unavailable, is not a paint, finds no slot, has no
//! room for its block or lacks a declared property draws its box's colour solidly,
//! and [`RenderService::canvas_paint_diagnostics`](crate::RenderService::canvas_paint_diagnostics)
//! names its entity and reason; a changed reason is logged once.

use super::super::retained::box_records::GUI_PAINT_BLOCK_STRIDE;
use crate::{RenderDevice, RenderError};
use ipp_core::services::asset_management::{AssetKey, formats::shader::ShaderParameterKind};
use ipp_core::systems::canvas::{CanvasPaintInstance, CanvasTarget};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Paints the canvas program holds at most.
pub const CANVAS_PAINT_SLOTS: usize = 8;

/// Vectors of the canvas program's paint parameter array, which the paint
/// instances of one canvas share.
pub const CANVAS_PAINT_VECTORS: usize = 128;

/// The backend whose paint bodies the GL renderer compiles.
pub(crate) const CANVAS_PAINT_BACKEND: &str = "glsl-es-300";

/// The line of `surface_gui.frag` the canvas program replaces with its paints.
const PAINT_PLACEHOLDER: &str = "// CANVAS_PAINTS";

/// Identifier prefixes a paint body may not use: the canvas program's own names.
const RESERVED_PREFIXES: [&str; 5] = ["ipp", "IPP_", "a_", "v_", "u_"];

/// The slot and parameter block a painted primitive carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GuiPaintLanes {
    /// One-based slot of the paint's function in the canvas program.
    pub slot: u32,
    /// First vector of the instance's block in `u_paint_blocks`.
    pub block: u32,
}

impl GuiPaintLanes {
    /// The slot lane: `block * GUI_PAINT_BLOCK_STRIDE + slot`, an exact integer.
    pub fn packed(self) -> f32 {
        self.block as f32 * GUI_PAINT_BLOCK_STRIDE + self.slot as f32
    }
}

/// The body and declared parameters a loaded paint definition retains for building
/// the canvas program, which needs them again whenever its set of paints changes.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CanvasPaintSource {
    pub body: Arc<str>,
    /// Recipe features: presentation context and optional scene lighting.
    pub features: u32,
    /// Parameters in name order, each one vector of the instance's block.
    pub parameters: Arc<[(String, ShaderParameterKind)]>,
}

/// Why a canvas entity's paint draws its box's colour instead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanvasPaintFallbackReason {
    /// The shader definition is unset or pending, failed to load or compile alone,
    /// or is being recovered after context loss.
    Unavailable,
    /// The shader definition has no paint body.
    NotAPaint,
    /// Every slot of the canvas program holds a paint drawn in this frame.
    SlotLimit,
    /// The canvas's paint parameter array has no room for this instance's block.
    UniformBudget,
    /// The named declared parameter has no numeric property of its type.
    Parameter(String),
    /// The canvas program failed to build with this paint; the compiler message.
    Program(String),
}

impl std::fmt::Display for CanvasPaintFallbackReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => f.write_str("paint shader unavailable"),
            Self::NotAPaint => f.write_str("shader definition has no paint body"),
            Self::SlotLimit => write!(
                f,
                "all {CANVAS_PAINT_SLOTS} canvas paint slots hold paints drawn this frame"
            ),
            Self::UniformBudget => write!(
                f,
                "the canvas's {CANVAS_PAINT_VECTORS} paint parameter vectors are taken"
            ),
            Self::Parameter(name) => write!(f, "parameter {name} has no property of its type"),
            Self::Program(message) => write!(f, "canvas program rejected the paint: {message}"),
        }
    }
}

/// A canvas entity's paint that draws its colour, with the reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanvasPaintFallback {
    /// Authored shader-definition source of the paint.
    pub source: String,
    /// Why the paint cannot draw.
    pub reason: CanvasPaintFallbackReason,
}

/// Reject a paint body that is not the statements of one function: preprocessor
/// directives, which would leak into the rest of the canvas program, braces that
/// close the function early or leave it open, `discard`, which would drop the
/// border and glow the renderer keeps, and the canvas program's reserved
/// identifier prefixes. Compilation checks everything else.
pub(crate) fn check_paint_body(body: &str) -> Result<(), String> {
    if body.trim().is_empty() {
        return Err("A paint body is empty".into());
    }

    let mut code = String::with_capacity(body.len());
    let mut rest = body;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("//") {
            rest = after.find('\n').map_or("", |end| &after[end..]);
            code.push(' ');
        } else if let Some(after) = rest.strip_prefix("/*") {
            let end = after
                .find("*/")
                .ok_or("A paint body has an unterminated comment")?;
            rest = &after[end + 2..];
            code.push(' ');
        } else {
            let character = rest.chars().next().expect("nonempty rest");
            code.push(character);
            rest = &rest[character.len_utf8()..];
        }
    }

    if code.contains('#') {
        return Err("A paint body cannot hold preprocessor directives".into());
    }
    let mut depth = 0_i64;
    for character in code.chars() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth < 0 {
                    return Err("A paint body cannot close its function".into());
                }
            }
            _ => {}
        }
    }
    if depth != 0 {
        return Err("A paint body leaves a brace open".into());
    }

    let mut identifiers = code.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'));
    if let Some(identifier) = identifiers.find(|word| {
        !word.starts_with(|c: char| c.is_ascii_digit())
            && (*word == "discard"
                || RESERVED_PREFIXES
                    .iter()
                    .any(|prefix| word.starts_with(prefix)))
    }) {
        return Err(if identifier == "discard" {
            "A paint returns a colour; it cannot discard".into()
        } else {
            format!("A paint body cannot use the reserved name {identifier}")
        });
    }
    Ok(())
}

/// The canvas program's paint parameter array.
fn parameter_array() -> String {
    format!("uniform vec4 u_paint_blocks[{CANVAS_PAINT_VECTORS}];\n")
}

/// One paint as the function `name`, with its parameters as `p_<name>` macros
/// reading the instance's block from `ipp_block`, undefined after it.
fn paint_function(name: &str, source: &CanvasPaintSource) -> String {
    let mut text = String::new();
    for (index, (parameter, kind)) in source.parameters.iter().enumerate() {
        let swizzle = match kind {
            ShaderParameterKind::F32 => ".x",
            ShaderParameterKind::Vec2 => ".xy",
            ShaderParameterKind::Vec3 => ".xyz",
            _ => "",
        };
        text.push_str(&format!(
            "#define p_{parameter} (u_paint_blocks[ipp_block + {index}]{swizzle})\n"
        ));
    }
    let context = source.features & 64 != 0;
    if context && source.features & 8 != 0 {
        text.push_str("#define sceneLightDirection(i) ippPaintLightDirection(i, ipp_canvas)\n#define sceneLightRadiance(i) ippPaintLightRadiance(i, ipp_canvas)\n");
    } else if context {
        text.push_str(
            "#define sceneLightDirection(i) vec3(0,0,1)\n#define sceneLightRadiance(i) vec3(0)\n",
        );
    }
    let inputs = if context {
        format!(
            "vec3 viewDirection = ippPaintView(ipp_canvas);\nint sceneLightCount = {};\n",
            if source.features & 8 != 0 {
                "u_light_count"
            } else {
                "0"
            }
        )
    } else {
        String::new()
    };
    text.push_str(&format!(
        "vec4 {name}(vec2 position, vec2 size, vec4 color, float edge, vec4 data, int ipp_block, vec2 ipp_canvas) {{\n{inputs}{}\n}}\n",
        source.body
    ));
    if context {
        text.push_str("#undef sceneLightDirection\n#undef sceneLightRadiance\n");
    }
    for (parameter, _) in source.parameters.iter() {
        text.push_str(&format!("#undef p_{parameter}\n"));
    }
    text
}

/// A program holding only `source`'s function, which the shader provider compiles
/// and links before it loads a paint.
pub(crate) fn validation_sources(source: &CanvasPaintSource) -> (String, String) {
    let vertex = "#version 300 es\nvoid main() { gl_Position = vec4(0.0, 0.0, 0.0, 1.0); }\n";
    let fragment = format!(
        "#version 300 es\nprecision highp float;\n{}{}out vec4 o_color;\n{}void main() {{\n    vec2 point = gl_FragCoord.xy;\n    o_color = ipp_paint_0(point, point + 1.0, vec4(point, 0.0, 1.0), point.x, vec4(point, 0.0, 1.0), int(u_paint_blocks[0].w), point);\n}}\n",
        parameter_array(),
        presentation_source(source.features & 64 != 0),
        paint_function("ipp_paint_0", source)
    );
    (vertex.into(), fragment)
}

/// The canvas fragment source with the paints of `slots`, slot one first; the static
/// source itself without any.
fn canvas_fragment_source<'a>(
    slots: impl Iterator<Item = (usize, &'a CanvasPaintSource)>,
) -> Cow<'static, str> {
    fragment_source(
        crate::services::render::embedded_shader!("shaders/surface_gui.frag"),
        slots,
    )
}

fn fragment_source<'a>(
    base: &'static str,
    slots: impl Iterator<Item = (usize, &'a CanvasPaintSource)>,
) -> Cow<'static, str> {
    let mut functions = String::new();
    let mut dispatch = String::new();
    let mut presentation = false;
    for (slot, source) in slots {
        presentation |= source.features & 64 != 0;
        let name = format!("ipp_paint_{slot}");
        functions.push_str(&paint_function(&name, source));
        dispatch.push_str(&format!(
            "    if (slot == {slot}) return {name}(position, size, color, edge, data, block, canvasPosition);\n"
        ));
    }
    if functions.is_empty() {
        return Cow::Borrowed(base);
    }

    let generated = format!(
        "#define IPP_CANVAS_PAINTS 1\n{}{}{functions}vec4 ipp_canvas_paint(int slot, int block, vec2 position, vec2 size, vec4 color, float edge, vec4 data, vec2 canvasPosition) {{\n{dispatch}    return color;\n}}\n",
        parameter_array(),
        presentation_source(presentation)
    );
    debug_assert_eq!(base.matches(PAINT_PLACEHOLDER).count(), 1);
    Cow::Owned(base.replacen(PAINT_PLACEHOLDER, &generated, 1))
}

fn presentation_source(enabled: bool) -> String {
    if !enabled {
        return String::new();
    }
    format!(
        "#define IPP_MAX_LIGHTS {}\n{}\n",
        super::super::lighting::lights::MAX_LIGHTS,
        crate::services::render::embedded_shader!("shaders/canvas-presentation.glsl")
    )
}

/// One admitted paint.
struct CanvasPaintSlot {
    shader: AssetKey,
    source: CanvasPaintSource,
    /// Frame that last drew a box with this paint.
    used: u64,
}

/// The canvas program, generated from the static source and the admitted paints,
/// and the slots those paints occupy. One per RenderService and context.
pub(crate) struct CanvasPaintPrograms<D: RenderDevice> {
    slots: [Option<CanvasPaintSlot>; CANVAS_PAINT_SLOTS],
    program: Option<D::Program>,
    path_program: Option<D::Program>,
    path_uploaded: Option<(ipp_core::OutputRef, u64)>,
    /// The slots changed since the program was built.
    stale: bool,
    /// Paints admitted since the last successful build.
    added: Vec<AssetKey>,
    /// Paints the combined program rejected, with the compiler message.
    rejected: BTreeMap<AssetKey, (CanvasPaintSource, String)>,
    frame: u64,
    /// The canvas whose parameter blocks the program holds, and their revision.
    uploaded: Option<(ipp_core::OutputRef, u64)>,
    /// Programs built, for diagnostics and tests.
    builds: u32,
}

impl<D: RenderDevice> Default for CanvasPaintPrograms<D> {
    fn default() -> Self {
        Self {
            slots: Default::default(),
            program: None,
            path_program: None,
            path_uploaded: None,
            stale: false,
            added: Vec::new(),
            rejected: BTreeMap::new(),
            frame: 0,
            uploaded: None,
            builds: 0,
        }
    }
}

impl<D: RenderDevice> CanvasPaintPrograms<D> {
    /// Start a frame: paints drawn before it may give up their slots.
    pub fn begin_frame(&mut self) {
        self.frame += 1;
    }

    /// Canvas programs built so far on this service.
    pub fn builds(&self) -> u32 {
        self.builds
    }

    /// The one-based slot of `shader`'s paint, admitting it when it has none: into
    /// a free slot, or the slot of the paint drawn least recently, before this frame.
    pub fn admit(
        &mut self,
        shader: AssetKey,
        source: &CanvasPaintSource,
    ) -> Result<u32, CanvasPaintFallbackReason> {
        if let Some((_, message)) = self.rejected.get(&shader).or_else(|| {
            self.rejected
                .values()
                .find(|(rejected, _)| rejected == source)
        }) {
            return Err(CanvasPaintFallbackReason::Program(message.clone()));
        }
        let frame = self.frame;
        if let Some(index) = self.slots.iter().position(|slot| {
            slot.as_ref()
                .is_some_and(|slot| slot.shader == shader || slot.source == *source)
        }) {
            self.slots[index].as_mut().expect("occupied slot").used = frame;
            return Ok(index as u32 + 1);
        }

        let index = self
            .slots
            .iter()
            .position(Option::is_none)
            .or_else(|| {
                self.slots
                    .iter()
                    .enumerate()
                    .filter_map(|(index, slot)| Some((index, slot.as_ref()?.used)))
                    .filter(|(_, used)| *used < frame)
                    .min_by_key(|(_, used)| *used)
                    .map(|(index, _)| index)
            })
            .ok_or(CanvasPaintFallbackReason::SlotLimit)?;
        self.slots[index] = Some(CanvasPaintSlot {
            shader,
            source: source.clone(),
            used: frame,
        });
        self.added.push(shader);
        self.stale = true;
        Ok(index as u32 + 1)
    }

    /// The current canvas program, built first when its paints changed, holding the
    /// parameter blocks of `output`.
    pub fn prepare_draw(
        &mut self,
        device: &mut D,
        output: ipp_core::OutputRef,
        blocks: &CanvasPaintBlocks,
    ) -> Result<&D::Program, RenderError> {
        if self.program.is_none() || self.stale {
            self.build(device)?;
        }

        let program = self.program.as_ref().expect("built canvas program");
        if !blocks.values.is_empty() && self.uploaded != Some((output, blocks.revision)) {
            device.set_gui_paint_blocks(program, &blocks.values)?;
            self.uploaded = Some((output, blocks.revision));
        }
        Ok(program)
    }

    /// The analytic program shares admission and parameter blocks with the shape program.
    pub fn prepare_path_draw(
        &mut self,
        device: &mut D,
        output: ipp_core::OutputRef,
        blocks: &CanvasPaintBlocks,
    ) -> Result<Option<&D::Program>, RenderError> {
        if self.program.is_none() || self.stale {
            self.build(device)?;
        }

        let Some(program) = self.path_program.as_ref() else {
            return Ok(None);
        };
        if !blocks.values.is_empty() && self.path_uploaded != Some((output, blocks.revision)) {
            device.set_gui_paint_blocks(program, &blocks.values)?;
            self.path_uploaded = Some((output, blocks.revision));
        }
        Ok(Some(program))
    }

    /// Build the program from the occupied slots. When it fails, reject the paints
    /// admitted since the last successful build and build without them; context
    /// loss returns at once.
    fn build(&mut self, device: &mut D) -> Result<(), RenderError> {
        if let Some(program) = self.program.take() {
            device.delete_program(program);
        }
        if let Some(program) = self.path_program.take() {
            device.delete_program(program);
        }
        self.uploaded = None;
        self.path_uploaded = None;
        let vertex = crate::services::render::embedded_shader!("shaders/surface_gui.vert");
        loop {
            let fragment = canvas_fragment_source(
                self.slots
                    .iter()
                    .enumerate()
                    .filter_map(|(index, slot)| Some((index + 1, &slot.as_ref()?.source))),
            );
            let created = device
                .create_program(vertex, &fragment)
                .and_then(|program| {
                    if self.slots.iter().all(Option::is_none) {
                        return Ok((program, None));
                    }
                    let path_fragment = fragment_source(
                        crate::services::render::embedded_shader!("shaders/surface.frag"),
                        self.slots
                            .iter()
                            .enumerate()
                            .filter_map(|(index, slot)| Some((index + 1, &slot.as_ref()?.source))),
                    );
                    match device.create_program(
                        crate::services::render::embedded_shader!("shaders/surface.vert"),
                        &path_fragment,
                    ) {
                        Ok(path) => Ok((program, Some(path))),
                        Err(error) => {
                            device.delete_program(program);
                            Err(error)
                        }
                    }
                });
            match created {
                Ok((program, path)) => {
                    self.program = Some(program);
                    self.path_program = path;
                    self.stale = false;
                    self.added.clear();
                    self.builds += 1;
                    return Ok(());
                }
                Err(RenderError::ContextLost) => return Err(RenderError::ContextLost),
                Err(error) => {
                    // The static program has no paints left to reject.
                    if self.slots.iter().all(Option::is_none) {
                        return Err(error);
                    }

                    // Without paints added since the last build the set is the last
                    // one that built; should even that fail, drop every paint.
                    let added = if self.added.is_empty() {
                        self.slots
                            .iter()
                            .flatten()
                            .map(|slot| slot.shader)
                            .collect()
                    } else {
                        std::mem::take(&mut self.added)
                    };
                    let message = error.to_string();
                    for shader in added {
                        ipp_core::diagnostic!(
                            Warn,
                            "canvas paint rejected by the canvas program: {message}"
                        );
                        for slot in &mut self.slots {
                            if slot.as_ref().is_some_and(|slot| slot.shader == shader) {
                                let rejected = slot.take().expect("rejected occupied slot");
                                self.rejected
                                    .insert(shader, (rejected.source, message.clone()));
                            }
                        }
                    }
                }
            }
        }
    }

    /// Release the program and every slot after unload or context loss; paints are
    /// admitted and the program rebuilt on next use.
    pub fn clear(&mut self, device: &mut D) {
        if let Some(program) = self.program.take() {
            device.delete_program(program);
        }
        if let Some(program) = self.path_program.take() {
            device.delete_program(program);
        }
        self.path_uploaded = None;
        self.slots = Default::default();
        self.stale = false;
        self.added.clear();
        self.rejected.clear();
        self.uploaded = None;
    }
}

/// One paint instance's block and lanes in a canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CanvasPaintBlock {
    start: u32,
    len: u32,
    lanes: Option<GuiPaintLanes>,
}

/// A frame-local group of instances with identical packed parameters.
struct CanvasPaintGroup {
    slot: u32,
    packed: Vec<[f32; 4]>,
    candidates: Vec<CanvasPaintBlock>,
    start: Option<u32>,
}

/// The parameter blocks of one canvas's paint instances and the lanes its painted
/// primitives carry.
#[derive(Default)]
pub(crate) struct CanvasPaintBlocks {
    blocks: BTreeMap<CanvasTarget, CanvasPaintBlock>,
    values: Vec<[f32; 4]>,
    /// Unique among all blocks of the service while `values` are unchanged.
    revision: u64,
    lanes_changed: bool,
}

impl CanvasPaintBlocks {
    /// The lanes of `paint`'s instance, or none when it draws its colour.
    pub fn lanes(&self, paint: CanvasTarget) -> Option<GuiPaintLanes> {
        self.blocks.get(&paint)?.lanes
    }

    /// Whether the last [`Self::prepare`] changed any instance's lanes, so retained
    /// geometry hashed under the previous lanes must be hashed again.
    pub fn lanes_changed(&self) -> bool {
        self.lanes_changed
    }

    /// Whether the canvas has paint instances to prepare or release.
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// Give this frame's `paints` their lanes and parameter values. `admit` returns
    /// an instance's slot and its paint's source; `report` receives each instance
    /// with its fallback reason, or none when it paints.
    pub fn prepare<'a>(
        &mut self,
        paints: &[CanvasPaintInstance],
        mut admit: impl FnMut(
            &CanvasPaintInstance,
        )
            -> Result<(u32, &'a CanvasPaintSource), CanvasPaintFallbackReason>,
        mut report: impl FnMut(&CanvasPaintInstance, Option<CanvasPaintFallbackReason>),
    ) {
        let previous = std::mem::take(&mut self.blocks);
        let mut values = vec![[0.0; 4]; self.values.len()];
        let mut taken: Vec<(u32, u32)> = Vec::new();
        self.lanes_changed = previous.keys().any(|target| {
            paints
                .binary_search_by_key(target, |paint| paint.target)
                .is_err()
        });

        // Equal parameter blocks share derived GPU storage. Authored paint
        // instances remain independent: a changed member splits before upload.
        let mut groups: Vec<CanvasPaintGroup> = Vec::new();
        let mut by_values = BTreeMap::new();
        let mut admitted = Vec::with_capacity(paints.len());
        for paint in paints {
            let result = admit(paint).and_then(|(slot, source)| {
                let mut packed = Vec::with_capacity(source.parameters.len());
                for (name, kind) in source.parameters.iter() {
                    let value = paint
                        .properties
                        .binary_search_by(|(property, _)| property.as_ref().cmp(name.as_str()))
                        .ok()
                        .map(|found| &paint.properties[found].1)
                        .filter(|value| ShaderParameterKind::from_value(value).ok() == Some(*kind))
                        .and_then(|value| value.floats())
                        .ok_or_else(|| CanvasPaintFallbackReason::Parameter(name.clone()))?;
                    let mut vector = [0.0; 4];
                    vector[..value.len()].copy_from_slice(value);
                    packed.push(vector);
                }

                let key = (
                    slot,
                    packed
                        .iter()
                        .map(|vector| vector.map(f32::to_bits))
                        .collect::<Vec<_>>(),
                );
                let index = *by_values.entry(key).or_insert_with(|| {
                    groups.push(CanvasPaintGroup {
                        slot,
                        packed,
                        candidates: Vec::new(),
                        start: None,
                    });
                    groups.len() - 1
                });
                let group = &mut groups[index];
                if let Some(block) = previous.get(&paint.target).filter(|block| {
                    block.lanes.is_some() && block.len as usize == group.packed.len()
                }) {
                    group.candidates.push(*block);
                }
                Ok(index)
            });
            admitted.push(result);
        }

        // Preserve unchanged aliases first, then prior independent blocks. A
        // changed alias cannot overwrite values still used by another instance.
        for unchanged in [true, false] {
            for CanvasPaintGroup {
                packed,
                candidates,
                start,
                ..
            } in &mut groups
            {
                if start.is_some() {
                    continue;
                }
                for candidate in candidates.iter() {
                    let begin = candidate.start;
                    let len = candidate.len;
                    let old = self.values.get(begin as usize..(begin + len) as usize);
                    if unchanged && old != Some(packed.as_slice()) {
                        continue;
                    }
                    if taken.iter().any(|(at, count)| {
                        len > 0 && *count > 0 && begin < at + count && *at < begin + len
                    }) {
                        continue;
                    }
                    *start = Some(begin);
                    taken.push((begin, len));
                    break;
                }
            }
        }

        for CanvasPaintGroup {
            packed,
            start,
            ..
        } in &mut groups
        {
            let len = packed.len() as u32;
            if start.is_none() {
                *start = first_fit(&taken, len);
                if let Some(begin) = *start {
                    taken.push((begin, len));
                }
            }
            if let Some(begin) = *start {
                let end = begin as usize + packed.len();
                values.resize(values.len().max(end), [0.0; 4]);
                values[begin as usize..end].copy_from_slice(packed);
            }
        }

        for (paint, result) in paints.iter().zip(admitted) {
            let kept = previous.get(&paint.target);
            let outcome = result.and_then(|index| {
                let CanvasPaintGroup {
                    slot,
                    packed,
                    start,
                    ..
                } = &groups[index];
                let start = start.ok_or(CanvasPaintFallbackReason::UniformBudget)?;
                Ok(CanvasPaintBlock {
                    start,
                    len: packed.len() as u32,
                    lanes: Some(GuiPaintLanes {
                        slot: *slot,
                        block: start,
                    }),
                })
            });
            let (block, reason) = match outcome {
                Ok(block) => (block, None),
                Err(reason) => (
                    CanvasPaintBlock {
                        start: 0,
                        len: 0,
                        lanes: None,
                    },
                    Some(reason),
                ),
            };
            self.lanes_changed |= kept.map(|kept| kept.lanes) != Some(block.lanes);
            self.blocks.insert(paint.target, block);
            report(paint, reason);
        }

        let end = self
            .blocks
            .values()
            .filter(|block| block.lanes.is_some())
            .map(|block| (block.start + block.len) as usize)
            .max()
            .unwrap_or(0);
        values.truncate(end);
        if values != self.values {
            self.values = values;
            self.revision = next_blocks_revision();
        }
    }
}

/// A revision no other parameter blocks hold: a canvas's blocks are released
/// and rebuilt with its retained batches, and the program must not take the new
/// blocks' first values for the old ones'.
fn next_blocks_revision() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// The first start of `len` vectors within the array that overlaps none of `taken`.
fn first_fit(taken: &[(u32, u32)], len: u32) -> Option<u32> {
    if len == 0 {
        return Some(0);
    }

    let mut spans: Vec<_> = taken.iter().filter(|(_, len)| *len > 0).copied().collect();
    spans.sort_unstable();
    let mut start = 0;
    for (span, span_len) in spans {
        if span >= start + len {
            break;
        }
        start = start.max(span + span_len);
    }
    (start + len <= CANVAS_PAINT_VECTORS as u32).then_some(start)
}

#[cfg(test)]
#[path = "paint_tests.rs"]
mod tests;
