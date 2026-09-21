use super::*;

#[test]
fn only_changed_active_values_upload_and_epochs_invalidate() {
    let mut cache = RenderUniformCache::default();
    let mut frame = RenderLightingFrame::empty([0.0; 4], [0.0; 3]);
    frame.count = 2;
    frame.lights[..32].fill(1.0);
    assert_eq!(
        cache.lighting(1, &[0.0; 3], &frame),
        CAMERA | AMBIENT | SURFACE | LIGHTS | COUNT
    );
    assert_eq!(cache.lighting(1, &[0.0; 3], &frame), 0);
    frame.lights[100] = 9.0;
    assert_eq!(cache.lighting(1, &[0.0; 3], &frame), 0);
    frame.lights[20] = 2.0;
    assert_eq!(cache.lighting(1, &[0.0; 3], &frame), LIGHTS);
    frame.count = 1;
    assert_eq!(cache.lighting(1, &[0.0; 3], &frame), LIGHTS | COUNT);
    frame.count = 2;
    frame.lights[20] = 3.0;
    assert_eq!(cache.lighting(1, &[0.0; 3], &frame), LIGHTS | COUNT);
    assert_eq!(
        cache.lighting(2, &[0.0; 3], &frame),
        CAMERA | AMBIENT | SURFACE | LIGHTS | COUNT
    );
}

#[cfg(feature = "shadows")]
#[test]
fn shadow_changes_cover_active_indices_and_disabled_then_reenabled_slots() {
    let mut cache = RenderUniformCache::default();
    let mut frame = RenderLightingFrame::empty([0.0; 4], [0.0; 3]);
    frame.count = 3;
    frame.shadow_count = 1;
    frame.shadow_settings[8] = 0.0;
    frame.shadow_matrices[32] = 1.0;
    assert_eq!(
        cache.shadows(1, &frame),
        SHADOW_SAMPLER | SHADOW_MATRICES | SHADOW_SETTINGS
    );
    assert_eq!(cache.shadows(1, &frame), 0);
    frame.shadow_count = 0;
    frame.shadow_settings[8] = -1.0;
    assert_eq!(cache.shadows(1, &frame), SHADOW_SETTINGS);
    frame.shadow_count = 1;
    frame.shadow_settings[8] = 0.0;
    frame.shadow_matrices[32] = 2.0;
    assert_eq!(cache.shadows(1, &frame), SHADOW_MATRICES | SHADOW_SETTINGS);
    assert_eq!(
        cache.shadows(2, &frame),
        SHADOW_SAMPLER | SHADOW_MATRICES | SHADOW_SETTINGS
    );
}
