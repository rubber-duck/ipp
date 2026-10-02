use super::{SurfacePaint, SurfacePaintTracker};

#[test]
fn retained_paint_requires_completed_revision_and_same_inherited_clip() {
    let mut tracker = SurfacePaintTracker::default();
    let clip = [0.0, 0.0, 100.0, 100.0];
    assert_eq!(
        tracker.paint(5, None, clip, 1.0),
        SurfacePaint {
            revision: 5,
            opacity: 1.0,
            reusable: false,
            patched: 0,
            kept: 0,
        }
    );
    tracker.drawn(5, clip, 1.0);
    assert!(tracker.paint(5, None, clip, 1.0).reuses(5));
    assert!(!tracker.paint(6, None, clip, 1.0).reusable);
    assert!(
        !tracker
            .paint(5, None, [0.0, 0.0, 50.0, 100.0], 1.0)
            .reusable
    );
    tracker.drawn(5, [0.0, 0.0, 50.0, 100.0], 1.0);
    assert!(!tracker.paint(5, None, clip, 1.0).reusable);
    assert!(!tracker.paint(5, None, clip, 0.5).reusable);
    tracker.drawn(0, clip, 1.0);
    assert!(!tracker.paint(0, None, clip, 1.0).reusable);
}

#[test]
fn a_patched_revision_reuses_only_the_entries_it_kept_from_the_drawn_revision() {
    let mut tracker = SurfacePaintTracker::default();
    let clip = [0.0, 0.0, 100.0, 100.0];
    tracker.drawn(5, clip, 1.0);

    // Revision 6 replaced some entries of the drawn revision 5 in place: the
    // Surface is not reusable as a whole, a kept entry reuses hashes from 5 and
    // a replaced one hashes again.
    let paint = tracker.paint(6, Some(5), clip, 1.0);
    assert!(!paint.reusable);
    assert!(!paint.reuses(5), "the Surface as a whole reuses nothing");
    assert!(paint.entry(false).reuses(5));
    assert!(!paint.entry(true).reuses(5));
    assert!(!paint.entry(false).reuses(4));

    // Changes from a revision the Surface did not draw, or drew under another
    // clip or opacity, reuse nothing.
    assert!(!tracker.paint(7, Some(6), clip, 1.0).entry(false).reuses(6));
    assert!(
        !tracker
            .paint(6, Some(5), [0.0, 0.0, 50.0, 100.0], 1.0)
            .entry(false)
            .reuses(5)
    );
    assert!(!tracker.paint(6, Some(5), clip, 0.5).entry(false).reuses(5));

    // Invalidated paint inputs hash every entry again.
    let mut invalidated = paint;
    invalidated.invalidate();
    assert!(!invalidated.entry(false).reuses(5));
}

#[test]
fn completed_submission_retires_only_removed_or_unseen_submitted_runs() {
    use ipp_core::EntityId;
    use std::collections::BTreeSet;
    let drawn = EntityId::from_bits(1);
    let skipped = EntityId::from_bits(2);
    let removed = EntityId::from_bits(3);
    let live = BTreeSet::from([drawn, skipped]);
    let submitted = BTreeSet::from([drawn]);
    let frame = super::RetainedSurfaceSubmission {
        live: &live,
        submitted: &submitted,
    };
    assert!(!frame.is_stale(drawn, true));
    assert!(frame.is_stale(drawn, false));
    assert!(!frame.is_stale(skipped, false));
    assert!(frame.is_stale(removed, true));
}
