//! Retained per-Surface GPU storage of GUI boxes and glyph quads.
//!
//! Every retained box batch and atlas glyph batch of one Surface occupies a slot of one
//! GPU vertex buffer, in painter order. Boxes and glyphs share the vertex layout and
//! program, and each vertex carries its clip, so consecutive slots draw as one range:
//! a run of boxes and text under different clips needs a new draw only where the atlas
//! page changes or a non-GUI item intervenes.
//!
//! Slots keep their positions while their pieces fit before the next retained slot, so
//! a changed piece rewrites only its own slot and unchanged frames write nothing.
//! Fresh slots reserve room to grow. Vertices between slots are all zero: degenerate
//! triangles that rasterize nothing, so drawing across a gap never shows stale
//! content. A Surface whose pieces no longer fit, or that uses a small fraction of its
//! storage, moves into newly allocated storage.

use std::collections::HashMap;

use super::gui_batch::GuiVertex;
use crate::{RenderDevice, RenderError, RenderStats};
use ipp_core::systems::surface::SurfacePrimitiveIdentity;

/// Stable identity of one retained piece within its Surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum GuiPieceKey {
    /// Box batch starting with this primitive identity.
    Boxes(SurfacePrimitiveIdentity),
    /// Atlas page batch `index` of the text run with this identity.
    Glyphs(SurfacePrimitiveIdentity, u32),
}

/// Where a piece's vertices come from when its slot is written.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum GuiPieceSource {
    /// Consecutive box identities, as a range of the submission's box list.
    Boxes(std::ops::Range<usize>),
    /// Atlas page batch `index` of a text run.
    Glyphs(SurfacePrimitiveIdentity, u32),
}

/// One retained batch of the Surface being submitted, in painter order.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GuiPiece {
    pub key: GuiPieceKey,
    /// Content identity: equal hashes of one key hold identical vertices.
    pub hash: u64,
    /// Vertex count, a multiple of three.
    pub len: usize,
    /// Atlas page the piece samples; `None` for boxes.
    pub page: Option<usize>,
    pub source: GuiPieceSource,
}

/// Placement of one piece in storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GuiSlot {
    key: GuiPieceKey,
    hash: u64,
    start: usize,
    len: usize,
}

/// Fewest vertices a fresh slot reserves to grow: one box strip set, or four quads.
const MIN_SLOT_GROWTH: usize = 24;

/// Most vertices a fresh slot reserves to grow.
const MAX_SLOT_GROWTH: usize = 96;

/// Storage holding more than this many vertices is replaced when its pieces use less
/// than a quarter of it.
const SHRINK_THRESHOLD: usize = 4096;

/// Vertices a fresh slot of `len` reserves after its piece, in whole quads.
fn slot_growth(len: usize) -> usize {
    (len / 4)
        .clamp(MIN_SLOT_GROWTH, MAX_SLOT_GROWTH)
        .div_ceil(6)
        * 6
}

/// Retained GPU storage of one Surface's GUI pieces.
pub(crate) struct GuiSurfaceStorage<D: RenderDevice> {
    gpu: D::GuiBatch,
    /// Allocated vertices.
    capacity: usize,
    /// Slots of the last committed submission, in painter and storage order.
    slots: Vec<GuiSlot>,
    /// Vertices at and beyond this index were never written and remain zero.
    written: usize,
    /// Frame that last committed this storage.
    pub seen: u64,
}

/// One write of a commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GuiWrite {
    /// Piece `index` at `start`.
    Piece {
        index: usize,
        start: usize,
    },
    /// `len` zero vertices at `start`.
    Clear {
        start: usize,
        len: usize,
    },
}

impl GuiWrite {
    fn range(&self, pieces: &[GuiPiece]) -> std::ops::Range<usize> {
        match *self {
            Self::Piece {
                index,
                start,
            } => start..start + pieces[index].len,
            Self::Clear {
                start,
                len,
            } => start..start + len,
        }
    }
}

/// Scratch buffers reused by commits.
#[derive(Default)]
pub(crate) struct GuiCommitScratch {
    anchors: Vec<Option<usize>>,
    old_index: HashMap<GuiPieceKey, usize>,
    starts: Vec<usize>,
    writes: Vec<GuiWrite>,
    vertices: Vec<GuiVertex>,
}

impl<D: RenderDevice> GuiSurfaceStorage<D> {
    /// Allocated GPU bytes.
    pub fn bytes(&self) -> usize {
        self.capacity * std::mem::size_of::<GuiVertex>()
    }

    /// Release the GPU storage.
    pub fn delete(self, device: &mut D) {
        device.delete_gui_batch(self.gpu);
    }

    /// Draw pieces `range` of the last commit, one draw per atlas page change.
    #[allow(clippy::too_many_arguments)]
    pub fn draw<'t>(
        &self,
        device: &mut D,
        program: &D::Program,
        pieces: &[GuiPiece],
        range: std::ops::Range<usize>,
        atlas: impl Fn(usize) -> Option<&'t D::Texture>,
        mvp: &[f32; 16],
        stats: &mut RenderStats,
    ) -> Result<(), RenderError>
    where
        D::Texture: 't,
    {
        let mut first = range.start;
        let mut page = None;
        for index in range.clone() {
            let piece_page = pieces[index].page;
            if piece_page.is_some() && page.is_some() && piece_page != page {
                self.draw_slots(device, program, first..index, page, &atlas, mvp, stats)?;
                first = index;
                page = None;
            }
            page = page.or(piece_page);
        }

        self.draw_slots(device, program, first..range.end, page, &atlas, mvp, stats)
    }

    /// Draw consecutive slots as one range sampling at most one atlas page.
    #[allow(clippy::too_many_arguments)]
    fn draw_slots<'t>(
        &self,
        device: &mut D,
        program: &D::Program,
        slots: std::ops::Range<usize>,
        page: Option<usize>,
        atlas: &impl Fn(usize) -> Option<&'t D::Texture>,
        mvp: &[f32; 16],
        stats: &mut RenderStats,
    ) -> Result<(), RenderError>
    where
        D::Texture: 't,
    {
        let Some(last) = slots.end.checked_sub(1).map(|index| self.slots[index]) else {
            return Ok(());
        };
        let texture =
            match page {
                Some(page) => Some(atlas(page).ok_or_else(|| {
                    RenderError::RenderDevice("atlas page texture missing".into())
                })?),
                None => None,
            };

        let first = self.slots[slots.start].start;
        device.draw_gui_batch(
            program,
            &self.gpu,
            texture,
            mvp,
            first,
            last.start + last.len - first,
        )?;
        stats.draw_calls += 1;
        for slot in &self.slots[slots.clone()] {
            stats.triangles += (slot.len / 3) as u32;
        }
        stats.gui_batches += slots.len() as u32;
        Ok(())
    }
}

/// Commit `pieces` to a Surface's storage, allocating or replacing it as needed.
///
/// `fill` appends the vertices of piece `index` to its buffer. Unchanged pieces are not
/// written. On failure the storage contents are unknown, so the storage is released
/// and `storage` left empty.
pub(crate) fn commit_surface_storage<D: RenderDevice>(
    device: &mut D,
    storage: &mut Option<GuiSurfaceStorage<D>>,
    pieces: &[GuiPiece],
    fill: &mut dyn FnMut(usize, &mut Vec<GuiVertex>),
    scratch: &mut GuiCommitScratch,
    stats: &mut RenderStats,
) -> Result<(), RenderError> {
    // Unchanged submissions keep every slot and write nothing.
    if let Some(current) = storage.as_ref()
        && current.slots.len() == pieces.len()
        && current.slots.iter().zip(pieces).all(|(slot, piece)| {
            slot.key == piece.key && slot.hash == piece.hash && slot.len == piece.len
        })
    {
        return Ok(());
    }

    let used: usize = pieces.iter().map(|piece| piece.len).sum();
    let placed = storage.as_ref().is_some_and(|current| {
        (current.capacity <= SHRINK_THRESHOLD || used * 4 >= current.capacity)
            && plan_slots(&current.slots, pieces, current.capacity, scratch)
    });

    let result = if placed {
        let current = storage.as_mut().expect("placed storage exists");
        plan_writes(current, pieces, scratch);
        write_planned(device, current, pieces, fill, scratch, stats)
    } else {
        replace_storage(device, storage, pieces, fill, scratch, stats)
    };

    if result.is_err()
        && let Some(failed) = storage.take()
    {
        failed.delete(device);
    }

    result
}

/// Choose piece starts in `scratch.starts`, keeping retained slots in place where
/// possible. Returns `false` when the pieces do not fit `capacity`.
fn plan_slots(
    old: &[GuiSlot],
    pieces: &[GuiPiece],
    capacity: usize,
    scratch: &mut GuiCommitScratch,
) -> bool {
    scratch.old_index.clear();
    for (index, slot) in old.iter().enumerate() {
        scratch.old_index.entry(slot.key).or_insert(index);
    }

    // Anchor pieces to the retained slots of their keys, in increasing storage order.
    scratch.anchors.clear();
    let mut last = None;
    for piece in pieces {
        let anchor = scratch
            .old_index
            .get(&piece.key)
            .copied()
            .filter(|&index| last.is_none_or(|last| index > last));
        if anchor.is_some() {
            last = anchor;
        }
        scratch.anchors.push(anchor);
    }

    scratch.starts.clear();
    let mut cursor = 0;
    let mut next = 0;
    for (index, piece) in pieces.iter().enumerate() {
        loop {
            // The next anchored piece bounds this one; releasing that anchor extends it.
            next = next.max(index + 1);
            while next < pieces.len() && scratch.anchors[next].is_none() {
                next += 1;
            }
            let limit = scratch
                .anchors
                .get(next)
                .copied()
                .flatten()
                .map_or(capacity, |anchor| old[anchor].start);

            let retained = scratch.anchors[index]
                .map(|anchor| old[anchor].start)
                .filter(|&start| start >= cursor && start + piece.len <= limit);
            let start = retained.unwrap_or(cursor);
            if start + piece.len <= limit {
                scratch.starts.push(start);
                cursor = start + piece.len;
                break;
            }

            if next >= pieces.len() {
                return false;
            }
            scratch.anchors[next] = None;
        }
    }

    true
}

/// Record the writes that move `storage` from its slots to `scratch.starts`, then
/// adopt the new slots. `scratch.old_index` maps keys to the current slots.
fn plan_writes<D: RenderDevice>(
    storage: &mut GuiSurfaceStorage<D>,
    pieces: &[GuiPiece],
    scratch: &mut GuiCommitScratch,
) {
    scratch.writes.clear();
    let old = &storage.slots;
    let old_end = old.last().map_or(0, |slot| slot.start + slot.len);
    let mut next_old = 0;
    let mut gap_start = 0;

    for (index, piece) in pieces.iter().enumerate() {
        let start = scratch.starts[index];

        // Clear whatever old slots, or unzeroed storage past the old end, left in the
        // gap before this piece. Old slots are sorted and disjoint and all precede that
        // tail, so the stale ranges arrive in order.
        let gap = gap_start..start;
        while old
            .get(next_old)
            .is_some_and(|slot| slot.start + slot.len <= gap.start)
        {
            next_old += 1;
        }
        for slot in &old[next_old..] {
            if slot.start >= gap.end {
                break;
            }
            push_clear(
                &mut scratch.writes,
                slot.start.max(gap.start)..(slot.start + slot.len).min(gap.end),
            );
        }
        push_clear(
            &mut scratch.writes,
            old_end.max(gap.start)..storage.written.min(gap.end),
        );

        let unchanged = scratch.old_index.get(&piece.key).is_some_and(|&slot| {
            old[slot]
                == GuiSlot {
                    key: piece.key,
                    hash: piece.hash,
                    start,
                    len: piece.len,
                }
        });
        if !unchanged {
            scratch.writes.push(GuiWrite::Piece {
                index,
                start,
            });
        }
        gap_start = start + piece.len;
    }

    storage.slots.clear();
    storage.slots.extend(
        pieces
            .iter()
            .zip(&scratch.starts)
            .map(|(piece, &start)| GuiSlot {
                key: piece.key,
                hash: piece.hash,
                start,
                len: piece.len,
            }),
    );
}

/// Append a clear of `range`, extending a clear that ends where it starts.
fn push_clear(writes: &mut Vec<GuiWrite>, range: std::ops::Range<usize>) {
    if range.is_empty() {
        return;
    }

    if let Some(GuiWrite::Clear {
        start,
        len,
    }) = writes.last_mut()
        && *start + *len == range.start
    {
        *len += range.len();
        return;
    }

    writes.push(GuiWrite::Clear {
        start: range.start,
        len: range.len(),
    });
}

/// Allocate storage for `pieces` with room to grow, write them and release the old
/// storage.
fn replace_storage<D: RenderDevice>(
    device: &mut D,
    storage: &mut Option<GuiSurfaceStorage<D>>,
    pieces: &[GuiPiece],
    fill: &mut dyn FnMut(usize, &mut Vec<GuiVertex>),
    scratch: &mut GuiCommitScratch,
    stats: &mut RenderStats,
) -> Result<(), RenderError> {
    scratch.starts.clear();
    let mut end = 0;
    for piece in pieces {
        scratch.starts.push(end);
        end += piece.len + slot_growth(piece.len);
    }
    // Room for appended work, so a growing Surface does not replace its storage on
    // every addition.
    let capacity = end + (end / 4).div_ceil(6) * 6;

    if let Some(old) = storage.take() {
        old.delete(device);
    }
    let gpu = device.create_gui_batch(capacity)?;
    let replacement = storage.insert(GuiSurfaceStorage {
        gpu,
        capacity,
        slots: Vec::new(),
        written: 0,
        seen: 0,
    });

    scratch.old_index.clear();
    plan_writes(replacement, pieces, scratch);
    write_planned(device, replacement, pieces, fill, scratch, stats)
}

/// Issue the planned writes, merging adjacent ones into single uploads.
fn write_planned<D: RenderDevice>(
    device: &mut D,
    storage: &mut GuiSurfaceStorage<D>,
    pieces: &[GuiPiece],
    fill: &mut dyn FnMut(usize, &mut Vec<GuiVertex>),
    scratch: &mut GuiCommitScratch,
    stats: &mut RenderStats,
) -> Result<(), RenderError> {
    let mut index = 0;
    while index < scratch.writes.len() {
        let start = scratch.writes[index].range(pieces).start;
        scratch.vertices.clear();
        let mut end = start;
        while let Some(write) = scratch.writes.get(index)
            && write.range(pieces).start == end
        {
            match *write {
                GuiWrite::Piece {
                    index: piece,
                    ..
                } => {
                    let before = scratch.vertices.len();
                    fill(piece, &mut scratch.vertices);
                    debug_assert_eq!(scratch.vertices.len() - before, pieces[piece].len);
                    stats.gui_allocations += 1;
                }
                GuiWrite::Clear {
                    len,
                    ..
                } => {
                    scratch
                        .vertices
                        .resize(scratch.vertices.len() + len, GuiVertex::EMPTY);
                }
            }
            end = write.range(pieces).end;
            index += 1;
        }

        if end > storage.capacity || scratch.vertices.len() != end - start {
            return Err(RenderError::RenderDevice(
                "GUI piece vertices do not match their slot".into(),
            ));
        }
        device.write_gui_batch(&mut storage.gpu, start, &scratch.vertices)?;
        storage.written = storage.written.max(end);
        stats.uploaded_bytes = stats
            .uploaded_bytes
            .saturating_add(std::mem::size_of_val(scratch.vertices.as_slice()) as u32);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ipp_core::systems::gui::GuiNodeId;
    use ipp_core::systems::surface::{GuiPrimitiveId, GuiPrimitivePart};

    fn key(node: u32) -> GuiPieceKey {
        GuiPieceKey::Boxes(SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
            root_incarnation: 1,
            node: GuiNodeId(node),
            lifetime: 1,
            part: GuiPrimitivePart::Background,
        }))
    }

    fn piece(node: u32, len: usize) -> GuiPiece {
        GuiPiece {
            key: key(node),
            hash: 0,
            len,
            page: None,
            source: GuiPieceSource::Boxes(0..0),
        }
    }

    fn slots(layout: &[(u32, usize, usize)]) -> Vec<GuiSlot> {
        layout
            .iter()
            .map(|&(node, start, len)| GuiSlot {
                key: key(node),
                hash: 0,
                start,
                len,
            })
            .collect()
    }

    fn plan(old: &[GuiSlot], pieces: &[GuiPiece], capacity: usize) -> Option<Vec<usize>> {
        let mut scratch = GuiCommitScratch::default();
        plan_slots(old, pieces, capacity, &mut scratch).then_some(scratch.starts)
    }

    #[test]
    fn growth_within_reserved_room_keeps_every_later_slot_in_place() {
        let old = slots(&[(1, 0, 12), (2, 36, 12), (3, 72, 12)]);
        let pieces = [piece(1, 18), piece(2, 12), piece(3, 12)];
        assert_eq!(plan(&old, &pieces, 108), Some(vec![0, 36, 72]));
    }

    #[test]
    fn inserted_and_split_pieces_pack_into_the_room_before_the_next_slot() {
        let old = slots(&[(1, 0, 24), (2, 48, 12)]);
        let pieces = [piece(1, 6), piece(9, 6), piece(8, 12), piece(2, 12)];
        assert_eq!(plan(&old, &pieces, 84), Some(vec![0, 6, 12, 48]));
    }

    #[test]
    fn overgrown_pieces_move_only_the_slots_they_reach() {
        let old = slots(&[(1, 0, 12), (2, 36, 12), (3, 72, 12)]);
        let pieces = [piece(1, 48), piece(2, 12), piece(3, 12)];
        assert_eq!(plan(&old, &pieces, 108), Some(vec![0, 48, 72]));
    }

    #[test]
    fn reordered_pieces_keep_painter_order_in_storage() {
        let old = slots(&[(1, 0, 6), (2, 30, 6), (3, 60, 6)]);
        let pieces = [piece(3, 6), piece(1, 6), piece(2, 6)];
        let starts = plan(&old, &pieces, 90).unwrap();
        assert!(
            starts.windows(2).all(|pair| pair[0] + 6 <= pair[1]),
            "{starts:?}"
        );
    }

    #[test]
    fn pieces_beyond_the_capacity_need_new_storage() {
        let old = slots(&[(1, 0, 12)]);
        assert_eq!(plan(&old, &[piece(1, 12), piece(2, 30)], 36), None);
        assert_eq!(slot_growth(6), 24);
        assert_eq!(slot_growth(200), 54);
        assert_eq!(slot_growth(10_000), 96);
    }
}
