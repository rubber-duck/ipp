//! The colour control's arrangement, its pointer-to-value mapping and its
//! colour model against independent expectations.

use super::*;

/// The textbook HSV model: hue sector, then the three products.
fn textbook(hue: f64, saturation: f64, value: f64) -> [f64; 3] {
    let sector = hue.rem_euclid(1.0) * 6.0;
    let index = sector.floor();
    let f = sector - index;
    let p = value * (1.0 - saturation);
    let q = value * (1.0 - saturation * f);
    let t = value * (1.0 - saturation * (1.0 - f));
    match index as u32 % 6 {
        0 => [value, t, p],
        1 => [q, value, p],
        2 => [p, value, t],
        3 => [p, q, value],
        4 => [t, p, value],
        _ => [value, p, q],
    }
}

#[test]
fn an_unsized_control_has_a_square_field_with_the_rails_beside_and_the_swatch_beneath() {
    assert_eq!(intrinsic_size(16.0, true), [240.0, 200.0]);
    assert_eq!(intrinsic_size(16.0, false), [200.0, 200.0]);
    assert_eq!(intrinsic_size(8.0, true), [120.0, 100.0]);

    let layout = GuiColorLayout::new([240.0, 200.0], 16.0, true);
    assert_eq!(layout.field, [8.0, 8.0, 144.0, 144.0]);
    assert_eq!(layout.hue, [168.0, 8.0, 24.0, 144.0]);
    assert_eq!(layout.alpha, Some([208.0, 8.0, 24.0, 144.0]));
    assert_eq!(layout.swatch, [8.0, 168.0, 224.0, 24.0]);
    assert_eq!((layout.marker, layout.thumb), (12.0, 8.0));

    let opaque = GuiColorLayout::new([200.0, 200.0], 16.0, false);
    assert_eq!(opaque.field, [8.0, 8.0, 144.0, 144.0]);
    assert_eq!(opaque.alpha, None);
    assert_eq!(opaque.swatch, [8.0, 168.0, 184.0, 24.0]);
    assert_eq!(opaque.surface(2), None);
}

#[test]
fn resizing_grows_the_field_and_keeps_the_rails_and_swatch_thickness() {
    // Twice as wide and 100 taller at half the type: lengths halve.
    let layout = GuiColorLayout::new([480.0, 300.0], 8.0, true);
    assert_eq!(layout.field, [4.0, 4.0, 432.0, 272.0]);
    assert_eq!(layout.hue, [444.0, 4.0, 12.0, 272.0]);
    assert_eq!(layout.alpha, Some([464.0, 4.0, 12.0, 272.0]));
    assert_eq!(layout.swatch, [4.0, 284.0, 472.0, 12.0]);

    // Too small a box leaves an empty field rather than negative extents.
    let small = GuiColorLayout::new([40.0, 30.0], 16.0, true);
    assert_eq!(small.field[2..], [0.0, 0.0]);
    assert!(small.swatch[2] >= 0.0 && small.swatch[3] >= 0.0);
}

#[test]
fn pointers_address_the_surface_whose_zone_meets_its_neighbours_halfway() {
    let layout = GuiColorLayout::new([240.0, 200.0], 16.0, true);
    for (point, part) in [
        ([80.0, 80.0], Some(0)),
        // The box's margin belongs to the nearest surface.
        ([0.0, 0.0], Some(0)),
        // Halfway across each gap the next surface begins.
        ([159.9, 50.0], Some(0)),
        ([160.0, 50.0], Some(1)),
        ([199.9, 50.0], Some(1)),
        ([200.0, 50.0], Some(2)),
        ([239.9, 159.9], Some(2)),
        // Past the zones' lower halves of the gap lies the swatch.
        ([80.0, 160.0], None),
        ([80.0, 180.0], None),
        ([240.0, 50.0], None),
    ] {
        assert_eq!(layout.part_at(point), part, "{point:?}");
    }
    // Without the alpha rail the hue rail's zone reaches the box's edge.
    let opaque = GuiColorLayout::new([200.0, 200.0], 16.0, false);
    assert_eq!(opaque.part_at([199.9, 50.0]), Some(1));
    assert_eq!(opaque.part_at([200.0, 50.0]), None);
}

#[test]
fn the_field_maps_saturation_rightward_and_value_upward_and_rails_upward() {
    let layout = GuiColorLayout::new([240.0, 200.0], 16.0, true);
    let [x, y, width, height] = layout.field;
    assert_eq!(
        layout.channels_at(0, [x, y]),
        [None, Some(0.0), Some(1.0), None]
    );
    assert_eq!(
        layout.channels_at(0, [x + width, y + height]),
        [None, Some(1.0), Some(0.0), None]
    );
    assert_eq!(
        layout.channels_at(0, [x + width * 0.25, y + height * 0.75]),
        [None, Some(0.25), Some(0.25), None]
    );
    // Past an edge the channel holds at that edge.
    assert_eq!(
        layout.channels_at(0, [x - 30.0, y + height + 30.0]),
        [None, Some(0.0), Some(0.0), None]
    );
    let hue = layout.hue;
    assert_eq!(
        layout.channels_at(1, [hue[0], hue[1] + hue[3]]),
        [Some(0.0), None, None, None]
    );
    assert_eq!(
        layout.channels_at(2, [0.0, hue[1] - 4.0]),
        [None, None, None, Some(1.0)]
    );
    assert_eq!(layout.channels_at(3, [0.0, 0.0]), [None; 4]);
}

#[test]
fn a_press_at_the_painted_marker_or_thumb_maps_back_to_the_value() {
    let layout = GuiColorLayout::new([300.0, 220.0], 12.0, true);
    for (saturation, value) in [(0.0, 0.0), (1.0, 1.0), (0.3, 0.8), (0.75, 0.125)] {
        let [x, y, size, _] = layout.marker_rect(saturation, value);
        assert_eq!(size, 9.0);
        let channels = layout.channels_at(0, [x + size / 2.0, y + size / 2.0]);
        let [_, s, v, _] = channels.map(Option::unwrap_or_default);
        assert!((s - saturation).abs() < 1e-5 && (v - value).abs() < 1e-5);
    }
    for (part, rail) in [(1, layout.hue), (2, layout.alpha.unwrap())] {
        for fraction in [0.0, 0.4, 1.0] {
            let [x, y, width, height] = layout.thumb_rect(rail, fraction);
            assert_eq!((x, width, height), (rail[0], rail[2], 6.0));
            let channels = layout.channels_at(part, [x + width / 2.0, y + height / 2.0]);
            let found = channels.into_iter().flatten().next().unwrap();
            assert!((found - fraction).abs() < 1e-5, "{part} {fraction}");
        }
    }
}

#[test]
fn the_model_is_the_textbook_hsv_colour_on_encoded_values() {
    let mut worst = 0.0f64;
    for hue in 0..=48 {
        for saturation in 0..=8 {
            for value in 0..=8 {
                let [h, s, v] = [
                    hue as f32 / 48.0,
                    saturation as f32 / 8.0,
                    value as f32 / 8.0,
                ];
                let model = hsv_to_srgb(h, s, v);
                let expected = textbook(f64::from(h), f64::from(s), f64::from(v));
                for channel in 0..3 {
                    worst = worst.max((f64::from(model[channel]) - expected[channel]).abs());
                }
            }
        }
    }
    assert!(worst < 1e-6, "worst error {worst}");

    // The sheet's #54F4FF, and hue 1 is red again.
    let hue = (4.0 - 160.0 / 171.0) / 6.0;
    let cyan = hsv_to_srgb(hue, 171.0 / 255.0, 1.0).map(|channel| (channel * 255.0).round());
    assert_eq!(cyan, [84.0, 244.0, 255.0]);
    assert_eq!(hsv_to_srgb(1.0, 1.0, 1.0), [1.0, 0.0, 0.0]);

    // Linear light for solid fills and gradient stops.
    assert_eq!(srgb_to_linear(0.0), 0.0);
    assert!((srgb_to_linear(0.5) - 0.214_041).abs() < 1e-6);
    assert_eq!(hsv_to_linear(0.0, 0.0, 1.0), [1.0; 3]);
}

#[test]
fn steps_stay_within_the_channel() {
    assert!((nudge(0.5, 1.0, COLOR_STEP) - 0.51).abs() < 1e-6);
    assert!((nudge(0.5, -1.0, COLOR_FINE_STEP) - 0.499).abs() < 1e-6);
    assert_eq!(nudge(0.995, 1.0, COLOR_STEP), 1.0);
    assert_eq!(nudge(0.0, -1.0, COLOR_STEP), 0.0);
}
