//! Draw order of a Surface's retained GUI work: shapes first, then glyphs, cut where
//! a later shape would cover earlier text.
//!
//! Shapes and glyphs draw from separate storage with separate programs, so drawing a
//! painter-ordered run piece by piece would alternate programs at every label. Within
//! a run of consecutive retained work, [`GuiDrawOrder`] instead draws all shapes in
//! one draw and then the glyphs, splitting the glyphs only where the atlas page
//! changes. Moving a shape before text painted ahead of it changes the picture only
//! where the two overlap, so the run is cut, and a new segment begins, at the first
//! shape whose paint bounds overlap glyphs earlier in the segment. An ordinary panel
//! whose labels sit on earlier backgrounds is therefore two draws; a dialog, toast,
//! caret or focus glow painted over earlier text adds a cut. Greedy cutting at the
//! first overlap yields the fewest segments, since every part of a valid segment is
//! valid too.
//!
//! Bounds are in canvas logical units: a shape's covered rectangles, including glow,
//! intersected with its clip, and a glyph batch's quads intersected with its clip and
//! grown by one atlas texel. Glyph quads already hold a blank texel around their
//! coverage, and a run's resolution band keeps a texel near one projected pixel, so
//! the extra texel covers the half-pixel antialiased fringe a shape or clip edge
//! paints beyond its bounds whatever the canvas's units. The segment's glyph bounds
//! are kept in a coarse grid over the canvas once there are more than a few, so a
//! large canvas costs time linear in its items rather than in their square.

use super::gui_records::GuiRecordKind;

/// Cells per axis of the grid indexing a segment's glyph bounds.
const GRID_CELLS: usize = 16;

/// Glyph bounds a segment tests one by one before it indexes them in the grid.
const LINEAR_RECTS: usize = 16;

/// Grid link terminator.
const NONE: u32 = u32::MAX;

/// Bounds containing every finite rectangle, for items whose bounds are not finite.
const EVERYWHERE: [f32; 4] = [
    f32::NEG_INFINITY,
    f32::NEG_INFINITY,
    f32::INFINITY,
    f32::INFINITY,
];

/// One painter-ordered unit of retained GUI work and its place in storage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum GuiDrawItem {
    /// One shape's records `first..first + count` of shape storage, painting within
    /// `bounds`, or nothing when `None`.
    Shape {
        first: usize,
        count: usize,
        bounds: Option<[f32; 4]>,
    },
    /// One glyph batch's records `first..first + count` of glyph storage, sampling
    /// atlas `page` and painting within `bounds`, or nothing when `None`.
    Glyphs {
        first: usize,
        count: usize,
        page: usize,
        bounds: Option<[f32; 4]>,
    },
}

/// One draw: records `first..end` of one storage, sampling `page` for glyphs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GuiDraw {
    pub kind: GuiRecordKind,
    pub first: usize,
    pub end: usize,
    pub page: Option<usize>,
    /// Records of the drawn items, without the empty room between storage slots.
    pub records: usize,
}

/// Planner of one run's draws, reusing its buffers across runs and frames.
#[derive(Default)]
pub(crate) struct GuiDrawOrder {
    draws: Vec<GuiDraw>,
    /// The current segment's shape draw.
    shapes: Option<GuiDraw>,
    /// The current segment's glyph draws, one per atlas page change.
    glyphs: Vec<GuiDraw>,
    text: GuiTextIndex,
}

impl GuiDrawOrder {
    /// Start a run whose items lie mostly within `domain` `[x0, y0, x1, y1]`, the
    /// canvas's content rectangle; items outside it are still ordered correctly.
    pub fn begin(&mut self, domain: [f32; 4]) {
        self.draws.clear();
        self.shapes = None;
        self.glyphs.clear();
        self.text.reset(domain);
    }

    /// Append the next item in painter order. Items of one kind arrive in storage
    /// order.
    pub fn push(&mut self, item: GuiDrawItem) {
        match item {
            GuiDrawItem::Shape {
                first,
                count,
                bounds,
            } => {
                if let Some(bounds) = bounds
                    && self.text.overlaps(finite(bounds))
                {
                    self.end_segment();
                }

                extend(&mut self.shapes, GuiRecordKind::Shape, None, first, count);
            }
            GuiDrawItem::Glyphs {
                first,
                count,
                page,
                bounds,
            } => {
                let mut last = self.glyphs.pop();
                if last.is_some_and(|draw| draw.page != Some(page)) {
                    self.glyphs.extend(last.take());
                }
                extend(&mut last, GuiRecordKind::Glyph, Some(page), first, count);
                self.glyphs.extend(last);
                if let Some(bounds) = bounds {
                    self.text.insert(bounds);
                }
            }
        }
    }

    /// The run's draws: each segment's shapes, then its glyphs.
    pub fn finish(&mut self) -> &[GuiDraw] {
        self.end_segment();
        &self.draws
    }

    fn end_segment(&mut self) {
        self.draws.extend(self.shapes.take());
        self.draws.append(&mut self.glyphs);
        self.text.clear();
    }
}

/// Extend `draw` by records `first..first + count`, or start it there.
fn extend(
    draw: &mut Option<GuiDraw>,
    kind: GuiRecordKind,
    page: Option<usize>,
    first: usize,
    count: usize,
) {
    match draw {
        Some(draw) => {
            debug_assert!(first >= draw.end, "items arrive in storage order");
            draw.end = first + count;
            draw.records += count;
        }
        None => {
            *draw = Some(GuiDraw {
                kind,
                first,
                end: first + count,
                page,
                records: count,
            });
        }
    }
}

/// `bounds`, or bounds covering everything when they are not finite.
fn finite(bounds: [f32; 4]) -> [f32; 4] {
    if bounds.iter().all(|value| value.is_finite()) {
        bounds
    } else {
        EVERYWHERE
    }
}

/// Whether two rectangles `[x0, y0, x1, y1]` share interior area.
fn intersects(a: [f32; 4], b: [f32; 4]) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

/// Glyph bounds of the current segment.
struct GuiTextIndex {
    rects: Vec<[f32; 4]>,
    /// Union of `rects`, rejecting most shapes without a lookup.
    union: Option<[f32; 4]>,
    /// Domain origin and grid cells per logical unit; `None` without a usable domain.
    grid: Option<([f32; 2], [f32; 2])>,
    /// Whether `rects` are entered in the grid.
    gridded: bool,
    /// First link of each cell.
    heads: Vec<u32>,
    /// `(rect, next link)` per cell entry.
    links: Vec<(u32, u32)>,
    /// Cells whose heads are set.
    touched: Vec<u32>,
}

impl Default for GuiTextIndex {
    fn default() -> Self {
        Self {
            rects: Vec::new(),
            union: None,
            grid: None,
            gridded: false,
            heads: vec![NONE; GRID_CELLS * GRID_CELLS],
            links: Vec::new(),
            touched: Vec::new(),
        }
    }
}

impl GuiTextIndex {
    fn reset(&mut self, domain: [f32; 4]) {
        self.clear();
        let size = [domain[2] - domain[0], domain[3] - domain[1]];
        self.grid = (domain.iter().all(|value| value.is_finite())
            && size[0] > 0.0
            && size[1] > 0.0)
            .then(|| {
                (
                    [domain[0], domain[1]],
                    [GRID_CELLS as f32 / size[0], GRID_CELLS as f32 / size[1]],
                )
            });
    }

    fn clear(&mut self) {
        for cell in self.touched.drain(..) {
            self.heads[cell as usize] = NONE;
        }
        self.links.clear();
        self.rects.clear();
        self.union = None;
        self.gridded = false;
    }

    fn insert(&mut self, rect: [f32; 4]) {
        let rect = finite(rect);
        self.union = Some(match self.union {
            Some(union) => [
                union[0].min(rect[0]),
                union[1].min(rect[1]),
                union[2].max(rect[2]),
                union[3].max(rect[3]),
            ],
            None => rect,
        });
        self.rects.push(rect);

        if self.gridded {
            self.enter(self.rects.len() - 1);
        } else if self.rects.len() > LINEAR_RECTS && self.grid.is_some() {
            self.gridded = true;
            for index in 0..self.rects.len() {
                self.enter(index);
            }
        }
    }

    fn overlaps(&self, rect: [f32; 4]) -> bool {
        if !self.union.is_some_and(|union| intersects(union, rect)) {
            return false;
        }
        if !self.gridded {
            return self.rects.iter().any(|other| intersects(*other, rect));
        }

        let ([x0, x1], [y0, y1]) = self.cells(rect);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let mut link = self.heads[y * GRID_CELLS + x];
                while link != NONE {
                    let (index, next) = self.links[link as usize];
                    if intersects(self.rects[index as usize], rect) {
                        return true;
                    }
                    link = next;
                }
            }
        }
        false
    }

    /// Enter rectangle `index` in every cell it touches.
    fn enter(&mut self, index: usize) {
        let ([x0, x1], [y0, y1]) = self.cells(self.rects[index]);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let cell = y * GRID_CELLS + x;
                if self.heads[cell] == NONE {
                    self.touched.push(cell as u32);
                }
                self.links.push((index as u32, self.heads[cell]));
                self.heads[cell] = (self.links.len() - 1) as u32;
            }
        }
    }

    /// Inclusive cell ranges `[x0, x1]` and `[y0, y1]` of `rect`; parts outside the
    /// domain fall into its edge cells.
    fn cells(&self, rect: [f32; 4]) -> ([usize; 2], [usize; 2]) {
        let (origin, scale) = self.grid.expect("a gridded index has a domain");
        let cell = |value: f32, axis: usize| {
            let cell = ((value - origin[axis]) * scale[axis]).floor();
            // NaN and infinities clamp to the edge cells.
            if cell >= GRID_CELLS as f32 {
                GRID_CELLS - 1
            } else if cell > 0.0 {
                cell as usize
            } else {
                0
            }
        };
        (
            [cell(rect[0], 0), cell(rect[2], 0)],
            [cell(rect[1], 1), cell(rect[3], 1)],
        )
    }
}

#[cfg(test)]
#[path = "gui_draw_order_tests.rs"]
mod tests;
