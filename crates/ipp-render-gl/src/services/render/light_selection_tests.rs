use super::*;

fn point(id: u64, x: f32) -> Candidate {
    let mut matrix = [0.0; 16];
    for i in 0..4 {
        matrix[i * 5] = 1.0;
    }
    matrix[12] = x;
    (
        EntityId::from_bits(id),
        matrix,
        Light {
            kind: 1,
            range: 10.0,
            ..Default::default()
        },
    )
}

#[test]
fn separated_objects_choose_distinct_lights_and_large_bounds_preserve_contribution() {
    let candidates: Vec<_> = (1..=24)
        .map(|i| {
            point(
                i,
                if i <= 12 {
                    -100.0
                } else {
                    100.0
                },
            )
        })
        .collect();
    for x in [-100.0, 100.0] {
        let selected = select(
            &candidates,
            [x, 0.0, 0.0],
            Some([[x - 1.0, -1.0, -1.0], [x + 1.0, 1.0, 1.0]]),
            &[],
        );
        assert_eq!(selected.len(), MAX_LIGHTS);
        assert!(
            selected
                .iter()
                .all(|light| candidates[light.index].1[12] == x as f32)
        );
    }
    assert_eq!(
        influence(&candidates[0], [0.0; 3], Some([[-1.0; 3], [1.0; 3]])),
        0.0
    );
    assert!(influence(&candidates[0], [0.0; 3], Some([[-101.0; 3], [101.0; 3]])) > 0.0);
    assert!(
        influence(&candidates[0], [0.0; 3], None) > 0.0,
        "unknown bounds do not prove exclusion"
    );
}

#[test]
fn selection_hysteresis_is_ten_percent_with_identity_ties_and_immediate_zero_removal() {
    let mut candidates: Vec<_> = (1..=9)
        .map(|i| {
            let mut candidate = point(i, 0.0);
            candidate.2.kind = 0;
            candidate
        })
        .collect();
    let original: Vec<_> = select(&candidates, [0.0; 3], None, &[])
        .iter()
        .map(|light| light.entity)
        .collect();
    assert_eq!(
        original,
        (1..=8).map(EntityId::from_bits).collect::<Vec<_>>()
    );
    candidates[8].2.intensity = 1.09;
    assert!(
        select(&candidates, [0.0; 3], None, &original)
            .iter()
            .all(|light| light.index != 8)
    );
    candidates[8].2.intensity = 1.11;
    assert!(
        select(&candidates, [0.0; 3], None, &original)
            .iter()
            .any(|light| light.index == 8)
    );
    candidates[0].2.intensity = 0.0;
    assert!(
        select(&candidates, [0.0; 3], None, &original)
            .iter()
            .all(|light| light.index != 0)
    );
}

#[test]
fn spotlight_falloff_uses_the_conservative_enclosure_and_directional_lights_ignore_distance() {
    let mut candidate = point(1, 0.0);
    candidate.2.kind = 2;
    assert!(
        influence(
            &candidate,
            [0.0, 0.0, -5.0],
            Some([[-0.1, -0.1, -5.1], [0.1, 0.1, -4.9]])
        ) > 0.0
    );
    assert_eq!(
        influence(
            &candidate,
            [0.0, 0.0, 5.0],
            Some([[-0.1, -0.1, 4.9], [0.1, 0.1, 5.1]])
        ),
        0.0
    );
    assert!(influence(&candidate, [0.0; 3], Some([[-6.0; 3], [6.0; 3]])) > 0.0);
    candidate.2.kind = 0;
    assert_eq!(
        influence(&candidate, [1e6; 3], None),
        influence(&candidate, [0.0; 3], None)
    );
}

#[test]
fn prepared_influence_matches_scalar_ranking_across_bounds_cones_and_history() {
    let candidates: Vec<_> = (1..=29)
        .map(|i| {
            let mut candidate = point(i, (i as f32 - 15.0) * 0.7);
            candidate.1[13] = (i % 5) as f32;
            candidate.1[8] = i as f32 * 0.03;
            candidate.1[10] = 0.2 + (i % 7) as f32;
            candidate.2.kind = (i % 3) as u32;
            candidate.2.intensity = (i % 11) as f32 * 0.2;
            candidate.2.range = 2.0 + i as f32;
            candidate.2.inner_cone = 0.1 + (i % 5) as f32 * 0.07;
            candidate.2.outer_cone = 0.7;
            candidate
        })
        .collect();
    let prepared: Vec<_> = candidates.iter().map(LightInfluence::new).collect();
    let mut previous = Vec::new();
    let mut actual = Vec::new();
    for step in 0..100 {
        let origin = [step as f64 * 0.13 - 6.0, 1.0, -5.0];
        for radius in [None, Some(0.0), Some(0.01), Some(1.0), Some(20.0)] {
            let bounds = radius.map(|r| [origin.map(|v| v - r), origin.map(|v| v + r)]);
            let expected = select(&candidates, origin, bounds, &previous);
            select_prepared_into(&prepared, origin, bounds, &previous, &mut actual);
            assert_eq!(actual.len(), expected.len());
            for (a, b) in actual.iter().zip(&expected) {
                assert_eq!(
                    (a.entity, a.score.to_bits(), a.rank.to_bits()),
                    (b.entity, b.score.to_bits(), b.rank.to_bits())
                );
            }
            previous.clear();
            previous.extend(expected.iter().map(|light| light.entity));
        }
    }
}

#[test]
fn algebraic_spotlight_scores_match_angular_reference_and_reject_distant_bounds() {
    let mut candidate = point(1, 0.0);
    candidate.2.kind = 2;
    candidate.2.inner_cone = 0.2;
    candidate.2.outer_cone = 0.7;
    let prepared = LightInfluence::new(&candidate);
    for distance in [0.1, 1.0, 5.0, 9.9, 15.0] {
        for radius in [0.0, 0.01, 0.2, 1.0] {
            for angle_index in 0..201 {
                let angle = angle_index as f64 * std::f64::consts::PI / 200.0;
                let origin = [distance * angle.sin(), 0.0, -distance * angle.cos()];
                for enclosure in [
                    None,
                    Some([origin.map(|v| v - radius), origin.map(|v| v + radius)]),
                ] {
                    let actual = prepared.influence(&ObjectInfluence::new(origin, enclosure));
                    let expected = influence(&candidate, origin, enclosure);
                    assert!(
                        (actual - expected).abs() <= 1e-10 * expected.abs().max(1.0),
                        "distance {distance}, radius {radius}, angle {angle}: {actual} vs {expected}"
                    );
                }
            }
        }
    }
    assert!(
        prepared
            .contact(&ObjectInfluence::new(
                [100.0, 0.0, 0.0],
                Some([[99.0, -1.0, -1.0], [101.0, 1.0, 1.0]])
            ))
            .is_none()
    );
}

#[test]
fn fitting_light_sets_skip_scoring_and_overflow_preserves_hysteresis() {
    let mut candidates: Vec<_> = (1..=9)
        .map(|id| {
            let mut candidate = point(id, 0.0);
            candidate.2.kind = 0;
            candidate
        })
        .collect();
    candidates[8].2.intensity = 1.09;
    let prepared: Vec<_> = candidates.iter().map(LightInfluence::new).collect();
    let previous: Vec<_> = (1..=8).map(EntityId::from_bits).collect();
    let mut selected = Vec::new();
    select_prepared_into(&prepared[..8], [0.0; 3], None, &previous, &mut selected);
    assert!(selected.iter().all(|light| light.score.is_nan()));
    select_prepared_into(&prepared, [0.0; 3], None, &previous, &mut selected);
    assert!(
        selected
            .iter()
            .all(|light| light.score.is_finite() && light.index < 8)
    );
}
