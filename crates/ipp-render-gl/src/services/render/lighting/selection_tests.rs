use super::*;

/// Scalar reference ranking for prepared light influence.
fn influence(candidate: &Candidate, origin: [f64; 3], bounds: Option<[[f64; 3]; 2]>) -> f64 {
    let (_, model, light) = candidate;
    let luminance =
        0.2126 * f64::from(light.r) + 0.7152 * f64::from(light.g) + 0.0722 * f64::from(light.b);
    let energy = luminance * f64::from(light.intensity);
    if energy <= 0.0 || !energy.is_finite() {
        return 0.0;
    }
    if light.kind == 0 {
        return energy;
    }
    let center = bounds.map_or(origin, |b| {
        std::array::from_fn(|i| (b[0][i] + b[1][i]) * 0.5)
    });
    let radius = bounds.map_or(0.0, |b| {
        (0..3)
            .map(|i| ((b[1][i] - b[0][i]) * 0.5).powi(2))
            .sum::<f64>()
            .sqrt()
    });
    let delta: [f64; 3] = std::array::from_fn(|i| center[i] - f64::from(model[12 + i]));
    let distance = delta.iter().map(|v| v * v).sum::<f64>().sqrt();
    let nearest = (distance - radius).max(0.01);
    let range = f64::from(light.range);
    let mut attenuation = (1.0 - (nearest / range).powi(4)).clamp(0.0, 1.0) / nearest.powi(2);
    if light.kind == 2 && distance > radius {
        let forward = [
            -f64::from(model[8]),
            -f64::from(model[9]),
            -f64::from(model[10]),
        ];
        let length = forward.iter().map(|v| v * v).sum::<f64>().sqrt();
        let cosine = (delta.iter().zip(forward).map(|(a, b)| a * b).sum::<f64>()
            / (distance * length))
            .clamp(-1.0, 1.0);
        let angle = (cosine.acos() - (radius / distance).min(1.0).asin()).max(0.0);
        let inner = f64::from(light.inner_cone).cos();
        let outer = f64::from(light.outer_cone).cos();
        attenuation *= ((angle.cos() - outer) / (inner - outer).max(1e-6))
            .clamp(0.0, 1.0)
            .powi(2);
    }
    // Missing bounds never prove exclusion. Keep an approximate weak candidate
    // even when its origin lies outside the light's range or cone.
    if bounds.is_none() {
        attenuation = attenuation.max(f64::EPSILON / nearest.powi(2));
    }
    let score = energy * attenuation;
    if score.is_finite() {
        score
    } else {
        0.0
    }
}

fn select(
    candidates: &[Candidate],
    origin: [f64; 3],
    enclosure: Option<[[f64; 3]; 2]>,
    previous: &[EntityId],
) -> Vec<RankedLight> {
    let mut selected = Vec::with_capacity(MAX_LIGHTS + 1);
    select_into(candidates, origin, enclosure, previous, &mut selected);
    selected
}

fn select_into(
    candidates: &[Candidate],
    origin: [f64; 3],
    enclosure: Option<[[f64; 3]; 2]>,
    previous: &[EntityId],
    selected: &mut Vec<RankedLight>,
) {
    selected.clear();
    for (index, candidate) in candidates.iter().enumerate() {
        let score = influence(candidate, origin, enclosure);
        if score > 0.0 {
            retain_best(
                selected,
                RankedLight {
                    index,
                    score,
                    rank: score
                        * if previous.contains(&candidate.0) {
                            1.1
                        } else {
                            1.0
                        },
                    entity: candidate.0,
                },
                MAX_LIGHTS,
            );
        }
    }
}

fn point(id: u64, x: f32) -> Candidate {
    let mut matrix = [0.0; 16];
    for i in 0..4 {
        matrix[i * 5] = 1.0;
    }
    matrix[12] = x;
    (
        crate::services::render::test_support::test_entity(id),
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
        (1..=8)
            .map(crate::services::render::test_support::test_entity)
            .collect::<Vec<_>>()
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
    let previous: Vec<_> = (1..=8)
        .map(crate::services::render::test_support::test_entity)
        .collect();
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
