use super::{SurfacePaint, SurfacePaintTracker};

#[test]
fn retained_paint_requires_completed_revision_and_same_inherited_clip() {
    let mut tracker = SurfacePaintTracker::default();
    let clip = [0.0, 0.0, 100.0, 100.0];
    assert_eq!(
        tracker.paint(5, clip, 1.0),
        SurfacePaint {
            revision: 5,
            opacity: 1.0,
            reusable: false
        }
    );
    tracker.drawn(5, clip, 1.0);
    assert!(tracker.paint(5, clip, 1.0).reuses(5));
    assert!(!tracker.paint(6, clip, 1.0).reusable);
    assert!(!tracker.paint(5, [0.0, 0.0, 50.0, 100.0], 1.0).reusable);
    tracker.drawn(5, [0.0, 0.0, 50.0, 100.0], 1.0);
    assert!(!tracker.paint(5, clip, 1.0).reusable);
    assert!(!tracker.paint(5, clip, 0.5).reusable);
    tracker.drawn(0, clip, 1.0);
    assert!(!tracker.paint(0, clip, 1.0).reusable);
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
