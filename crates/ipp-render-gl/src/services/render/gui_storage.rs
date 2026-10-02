//! Retained per-Surface GPU storage of GUI shape or glyph records.
//!
//! Every retained shape batch of one Surface occupies a slot of its shape storage, and
//! every atlas glyph batch a slot of its glyph storage, each in painter order. Each
//! record carries its clip, so consecutive slots of one storage draw as one range of
//! instances; the [draw order](super::gui_draw_order) decides where a run of shapes
//! and text splits into draws.
//!
//! Slots keep their positions while their pieces fit before the next retained slot, so
//! a changed piece rewrites only its own slot and unchanged frames write nothing. A text
//! edit therefore writes only glyph storage and a shape change only shape storage.
//! Fresh slots reserve room to grow. Records between slots are all zero: empty
//! rectangles that rasterize nothing, so drawing across a gap never shows stale
//! content. A Surface whose pieces no longer fit, or that uses a small fraction of its
//! storage, moves into newly allocated storage.

use std::collections::HashMap;

use super::gui_records::GuiRecord;
use crate::services::render::frame_statistics::RenderFrameWork;
use crate::{RenderDevice, RenderError};
use ipp_core::systems::canvas::CanvasPrimitiveId;

/// Stable identity of one retained piece within its Surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum GuiPieceKey {
    /// Shape batch starting with this primitive identity.
    Boxes(CanvasPrimitiveId),
    /// Atlas page batch `index` of the text run with this identity.
    Glyphs(CanvasPrimitiveId, u32),
}

/// Where a piece's records come from when its slot is written.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum GuiPieceSource {
    /// Consecutive shape identities, as a range of the submission's shape list.
    Boxes(std::ops::Range<usize>),
    /// Atlas page batch `index` of a text run.
    Glyphs(CanvasPrimitiveId, u32),
}

/// One retained batch of the Surface being submitted, in painter order.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GuiPiece {
    pub key: GuiPieceKey,
    /// Content identity: equal hashes of one key hold identical records.
    pub hash: u64,
    /// Record count.
    pub len: usize,
    /// Atlas page the piece samples; `None` for shapes.
    pub page: Option<usize>,
    /// Paint bounds `[x0, y0, x1, y1]` of a glyph batch within its clip, or `None`
    /// when it paints nothing; shape pieces order by their shapes' own bounds.
    pub bounds: Option<[f32; 4]>,
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

/// Fewest records a fresh slot reserves to grow: one outline box's strips.
const MIN_SLOT_GROWTH: usize = 4;

/// Most records a fresh slot reserves to grow.
const MAX_SLOT_GROWTH: usize = 16;

/// Storage holding more than this many records is replaced when its pieces use less
/// than a quarter of it.
const SHRINK_THRESHOLD: usize = 1024;

/// Records a fresh slot of `len` reserves after its piece.
fn slot_growth(len: usize) -> usize {
    (len / 4).clamp(MIN_SLOT_GROWTH, MAX_SLOT_GROWTH)
}

/// Retained GPU storage of one Surface's GUI pieces of one record kind.
pub(crate) struct GuiSurfaceStorage<D: RenderDevice, R: GuiRecord> {
    gpu: D::GuiBatch,
    /// Allocated records.
    capacity: usize,
    /// Slots of the last committed submission, in painter and storage order.
    slots: Vec<GuiSlot>,
    /// Records at and beyond this index were never written and remain zero.
    written: usize,
    record: std::marker::PhantomData<R>,
}

/// One write of a commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GuiWrite {
    /// Piece `index` at `start`.
    Piece {
        index: usize,
        start: usize,
    },
    /// `len` zero records at `start`.
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
pub(crate) struct GuiCommitScratch<R: GuiRecord> {
    anchors: Vec<Option<usize>>,
    old_index: HashMap<GuiPieceKey, usize>,
    starts: Vec<usize>,
    writes: Vec<GuiWrite>,
    records: Vec<R>,
}

impl<R: GuiRecord> Default for GuiCommitScratch<R> {
    fn default() -> Self {
        Self {
            anchors: Vec::new(),
            old_index: HashMap::new(),
            starts: Vec::new(),
            writes: Vec::new(),
            records: Vec::new(),
        }
    }
}

impl<D: RenderDevice, R: GuiRecord> GuiSurfaceStorage<D, R> {
    /// Allocated GPU bytes.
    pub fn bytes(&self) -> usize {
        self.capacity * std::mem::size_of::<R>()
    }

    /// Release the GPU storage.
    pub fn delete(self, device: &mut D) {
        device.delete_gui_batch(self.gpu);
    }

    /// First record of committed piece `index`.
    pub fn slot_start(&self, index: usize) -> usize {
        self.slots[index].start
    }

    /// Draw records `first..end` of the last commit as instanced quads, sampling
    /// `atlas` for glyphs.
    pub fn draw(
        &self,
        device: &mut D,
        program: &D::Program,
        atlas: Option<&D::Texture>,
        mvp: &[f32; 16],
        records: std::ops::Range<usize>,
    ) -> Result<(), RenderError> {
        device.draw_gui_batch(program, &self.gpu, atlas, mvp, records.start, records.len())
    }
}

/// Commit `pieces` to a Surface's storage, allocating or replacing it as needed.
///
/// `fill` appends the records of piece `index` to its buffer. Unchanged pieces are not
/// written. On failure the storage contents are unknown, so the storage is released
/// and `storage` left empty.
pub(crate) fn commit_surface_storage<D: RenderDevice, R: GuiRecord>(
    device: &mut D,
    storage: &mut Option<GuiSurfaceStorage<D, R>>,
    pieces: &[GuiPiece],
    fill: &mut dyn FnMut(usize, &mut Vec<R>),
    scratch: &mut GuiCommitScratch<R>,
    stats: &mut RenderFrameWork,
) -> Result<(), RenderError> {
    // A Surface without pieces of this kind keeps no storage for them.
    if pieces.is_empty() {
        if let Some(current) = storage.take() {
            current.delete(device);
        }
        return Ok(());
    }

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
fn plan_slots<R: GuiRecord>(
    old: &[GuiSlot],
    pieces: &[GuiPiece],
    capacity: usize,
    scratch: &mut GuiCommitScratch<R>,
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
fn plan_writes<D: RenderDevice, R: GuiRecord>(
    storage: &mut GuiSurfaceStorage<D, R>,
    pieces: &[GuiPiece],
    scratch: &mut GuiCommitScratch<R>,
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
fn replace_storage<D: RenderDevice, R: GuiRecord>(
    device: &mut D,
    storage: &mut Option<GuiSurfaceStorage<D, R>>,
    pieces: &[GuiPiece],
    fill: &mut dyn FnMut(usize, &mut Vec<R>),
    scratch: &mut GuiCommitScratch<R>,
    stats: &mut RenderFrameWork,
) -> Result<(), RenderError> {
    scratch.starts.clear();
    let mut end = 0;
    for piece in pieces {
        scratch.starts.push(end);
        end += piece.len + slot_growth(piece.len);
    }
    // Room for appended work, so a growing Surface does not replace its storage on
    // every addition.
    let capacity = end + end / 4;

    if let Some(old) = storage.take() {
        old.delete(device);
    }
    let gpu = device.create_gui_batch(R::KIND, capacity)?;
    let replacement = storage.insert(GuiSurfaceStorage {
        gpu,
        capacity,
        slots: Vec::new(),
        written: 0,
        record: std::marker::PhantomData,
    });

    scratch.old_index.clear();
    plan_writes(replacement, pieces, scratch);
    write_planned(device, replacement, pieces, fill, scratch, stats)
}

/// Issue the planned writes, merging adjacent ones into single uploads.
fn write_planned<D: RenderDevice, R: GuiRecord>(
    device: &mut D,
    storage: &mut GuiSurfaceStorage<D, R>,
    pieces: &[GuiPiece],
    fill: &mut dyn FnMut(usize, &mut Vec<R>),
    scratch: &mut GuiCommitScratch<R>,
    stats: &mut RenderFrameWork,
) -> Result<(), RenderError> {
    let mut index = 0;
    while index < scratch.writes.len() {
        let start = scratch.writes[index].range(pieces).start;
        scratch.records.clear();
        let mut end = start;
        while let Some(write) = scratch.writes.get(index)
            && write.range(pieces).start == end
        {
            match *write {
                GuiWrite::Piece {
                    index: piece,
                    ..
                } => {
                    let before = scratch.records.len();
                    fill(piece, &mut scratch.records);
                    debug_assert_eq!(scratch.records.len() - before, pieces[piece].len);
                    stats.statistics.gui_allocations += 1;
                }
                GuiWrite::Clear {
                    len,
                    ..
                } => {
                    scratch
                        .records
                        .resize(scratch.records.len() + len, R::EMPTY);
                }
            }
            end = write.range(pieces).end;
            index += 1;
        }

        if end > storage.capacity || scratch.records.len() != end - start {
            return Err(RenderError::RenderDevice(
                "GUI piece records do not match their slot".into(),
            ));
        }
        device.write_gui_batch(&mut storage.gpu, start, &scratch.records)?;
        storage.written = storage.written.max(end);
        stats.uploaded(std::mem::size_of_val(scratch.records.as_slice()));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ipp_core::systems::canvas::CanvasPart;

    fn key(node: u32) -> GuiPieceKey {
        GuiPieceKey::Boxes(CanvasPrimitiveId {
            target: ipp_core::systems::canvas::CanvasTarget {
                entity: ipp_core::EntityId::from_bits(u64::from(node)),
                component: ipp_core::ComponentValue::CANVAS_BOX,
                incarnation: 1,
            },
            part: CanvasPart::Background,
        })
    }

    fn piece(node: u32, len: usize) -> GuiPiece {
        GuiPiece {
            key: key(node),
            hash: 0,
            len,
            page: None,
            bounds: None,
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
        let mut scratch =
            GuiCommitScratch::<crate::services::render::gui_records::GuiGlyphRecord>::default();
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
        assert_eq!(slot_growth(1), 4);
        assert_eq!(slot_growth(36), 9);
        assert_eq!(slot_growth(10_000), 16);
    }
}
