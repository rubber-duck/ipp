//! Reusable asset scenarios; EGL setup and capture remain with the environment.

use std::{f64::consts::TAU, path::Path};

use ipp_core::{
    Command, ComponentValue, EntityRef, FieldValue, FieldWrite, WorldContext,
    components::{MeshInstance, Transform, UnlitMaterial, UnlitTexture},
};
use ipp_render_gl::{RenderDevice, RenderService};

use super::world::{HEIGHT, WIDTH, apply, coverage, deliver, matches, save};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

type Point = [f64; 3];

pub fn run<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    fixtures: &Path,
    mut capture: impl FnMut() -> Result<Vec<u8>>,
    output: &Path,
) -> Result<()> {
    super::world::render_frame(
        renderer,
        &mut super::world::empty_world(&mut ipp_core::HostRuntime::new()),
        WIDTH,
        HEIGHT,
    )?;

    let mut evidence =
        String::from("shape,solid_pixels,outline_pixels,contour_pixels,curve_samples\n");
    for name in ["cube", "sphere", "pill"] {
        let mut solid_host = ipp_core::HostRuntime::new();
        let mut solid = load(&mut solid_host, renderer, fixtures, name)?;
        let stats = super::world::render_frame(renderer, &mut solid, WIDTH, HEIGHT)?;
        assert_eq!(stats.draw_calls, 1);
        let pixels = capture()?;
        save(output, &format!("shape-{name}"), &pixels)?;
        let filled = coverage(&pixels).0;
        assert!(filled > 1500, "{name}: missing solid silhouette ({filled})");
        assert!(filled < (WIDTH * HEIGHT / 2) as usize);

        // Every generator's UVs must support the ordinary texture path.
        let texture = std::fs::read(fixtures.join("checker.texture"))?;
        let entity = EntityRef::Handle(solid.render_items()[0].entity);
        apply(
            &mut solid,
            vec![Command::InsertComponent {
                entity,
                component: ComponentValue::UNLIT_TEXTURE,
                fields: vec![FieldWrite {
                    offset: std::mem::offset_of!(UnlitTexture, source) as u32,
                    value: FieldValue::String("fixture:///fixture.texture".into()),
                }],
            }],
        )?;
        assert!(solid.render_items().is_empty());
        deliver!(renderer, solid_host, solid, &[], Some(&texture))?;
        super::world::render_frame(renderer, &mut solid, WIDTH, HEIGHT)?;
        let textured = capture()?;
        save(output, &format!("shape-{name}-checker"), &textured)?;
        for rgb in [[255, 0, 0], [0, 255, 0], [0, 0, 255], [0, 0, 0]] {
            let count = textured
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| matches(p, &[rgb[0], rgb[1], rgb[2], 255]))
                .count();
            assert!(
                count > filled / 10,
                "{name}: missing checker RGB {rgb:?}: {count}/{filled}"
            );
        }

        // Discard GPU cache from the previous world before reusing its key.
        super::world::render_frame(
            renderer,
            &mut super::world::empty_world(&mut ipp_core::HostRuntime::new()),
            WIDTH,
            HEIGHT,
        )?;
        let mut outline_host = ipp_core::HostRuntime::new();
        let mut outline = load(
            &mut outline_host,
            renderer,
            fixtures,
            &format!("{name}-outline"),
        )?;
        super::world::render_frame(renderer, &mut outline, WIDTH, HEIGHT)?;
        let pixels = capture()?;
        save(output, &format!("shape-{name}-outline"), &pixels)?;
        let wire = coverage(&pixels).0;
        assert!(
            wire > 150 && wire < filled / 2,
            "{name}: contours should have open interiors ({wire}/{filled})"
        );
        let curves = curves(name);
        let (on_curve, samples) = assert_contours(&pixels, &curves, name);
        evidence.push_str(&format!("{name},{filled},{wire},{on_curve},{samples}\n"));

        // Ordinary immutable assets remain cached, then disappear with the world.
        assert_eq!(
            super::world::render_frame(renderer, &mut outline, WIDTH, HEIGHT)?.uploaded_bytes,
            0
        );
        super::world::render_frame(
            renderer,
            &mut super::world::empty_world(&mut ipp_core::HostRuntime::new()),
            WIDTH,
            HEIGHT,
        )?;
        assert_eq!(coverage(&capture()?).0, 0);
    }

    std::fs::write(output.join("shape-samples.csv"), evidence)?;
    planes(renderer, fixtures, &mut capture, output)?;
    Ok(())
}

fn planes<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    fixtures: &Path,
    capture: &mut impl FnMut() -> Result<Vec<u8>>,
    output: &Path,
) -> Result<()> {
    let other_world_programs = renderer.cached_program_count();
    let mut world_host = ipp_core::HostRuntime::new();
    let mut world = load(&mut world_host, renderer, fixtures, "plane")?;
    super::world::render_frame(renderer, &mut world, WIDTH, HEIGHT)?;
    let filled = capture()?;
    save(output, "plane", &filled)?;
    let color_count = |pixels: &[u8], color| {
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| matches(p, &color))
            .count()
    };
    assert!(
        color_count(&filled, [137, 137, 137, 255]) > 2500,
        "grey square must be visible"
    );
    assert!(
        color_count(&filled, [255; 4]) > 60,
        "normal arrow must contrast with square"
    );
    assert_arrow(&filled, false);

    // The arrow must stay independent of arbitrary sampled colors, while the
    // square changes through ordinary texture references and material fields.
    let entity = EntityRef::Handle(world.render_items()[0].entity);
    let texture_type = ComponentValue::UNLIT_TEXTURE;
    let material_type = ComponentValue::UNLIT_MATERIAL;
    let arrow_mask: Vec<_> = filled
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .filter_map(|(i, pixel)| matches(pixel, &[255; 4]).then_some(i))
        .collect();
    for (asset_id, bytes) in [
        (1, std::fs::read(fixtures.join("checker.texture"))?),
        (2, {
            let mut bytes = b"IPPT".to_vec();
            for value in [3u32, 1, 1] {
                bytes.extend(value.to_le_bytes());
            }
            bytes.extend([64, 160, 224, 255]);
            bytes
        }),
    ] {
        if asset_id == 1 {
            apply(
                &mut world,
                vec![Command::InsertComponent {
                    entity,
                    component: texture_type,
                    fields: vec![FieldWrite {
                        offset: std::mem::offset_of!(UnlitTexture, source) as u32,
                        value: FieldValue::String("fixture:///fixture.texture".into()),
                    }],
                }],
            )?;
        } else {
            apply(
                &mut world,
                vec![Command::SetField {
                    entity,
                    component: texture_type,
                    field: FieldWrite {
                        offset: std::mem::offset_of!(UnlitTexture, source) as u32,
                        value: FieldValue::String(format!("fixture:///fixture-{asset_id}.texture")),
                    },
                }],
            )?;
        }
        deliver!(renderer, world_host, world, &[], Some(&bytes))?;
        super::world::render_frame(renderer, &mut world, WIDTH, HEIGHT)?;
        let textured = capture()?;
        save(output, &format!("plane-texture-{asset_id}"), &textured)?;
        assert!(
            filled
                .as_chunks::<4>()
                .0
                .iter()
                .zip(textured.as_chunks::<4>().0)
                .filter(|(a, b)| a != b)
                .count()
                > 2000,
            "square must receive texture"
        );
        for i in &arrow_mask {
            assert_eq!(
                textured.as_chunks::<4>().0[*i],
                [255; 4],
                "texture asset_id {asset_id} must leave arrow solid"
            );
        }

        // Release the old recipe only after the embedding Host has finished
        // mandatory invalidation with no outstanding World borrow.
        let world_id = world.id();
        drop(world);
        world_host.flush_resource_lifecycle();
        world = world_host
            .world_mut(world_id)
            .expect("fixture World remains live");

        assert_eq!(
            renderer.cached_program_count(),
            other_world_programs + 1,
            "only the active plane recipe remains resident beside other Worlds"
        );
    }
    let tint = [0.25, 0.5, 0.75];
    let material_offsets = [
        std::mem::offset_of!(UnlitMaterial, r),
        std::mem::offset_of!(UnlitMaterial, g),
        std::mem::offset_of!(UnlitMaterial, b),
    ];
    apply(
        &mut world,
        material_offsets
            .into_iter()
            .zip(tint)
            .map(|(offset, value)| Command::SetField {
                entity,
                component: material_type,
                field: super::world::float(offset, value),
            })
            .collect(),
    )?;
    let expected = [137, 188, 225, 255];
    for textured in [true, false] {
        if !textured {
            apply(
                &mut world,
                vec![Command::RemoveComponent {
                    entity,
                    component: texture_type,
                }],
            )?;
        }
        super::world::render_frame(renderer, &mut world, WIDTH, HEIGHT)?;
        let pixels = capture()?;
        save(
            output,
            if textured {
                "plane-tinted-texture"
            } else {
                "plane-tinted-solid"
            },
            &pixels,
        )?;
        for i in &arrow_mask {
            assert!(
                matches(&pixels.as_chunks::<4>().0[*i], &expected),
                "material tint must affect solid arrow"
            );
        }
    }
    apply(
        &mut world,
        material_offsets
            .into_iter()
            .map(|offset| Command::SetField {
                entity,
                component: material_type,
                field: super::world::float(offset, 1.0),
            })
            .collect(),
    )?;

    // Rotating the actual entity rotates its normal. The single-sided square
    // faces away after a half turn, but its 3D arrow must remain visible.
    let entity = EntityRef::Handle(world.render_items()[0].entity);
    let transform = ComponentValue::TRANSFORM;
    apply(
        &mut world,
        vec![
            Command::SetField {
                entity,
                component: transform,
                field: super::world::float(std::mem::offset_of!(Transform, qy), 1.0),
            },
            Command::SetField {
                entity,
                component: transform,
                field: super::world::float(std::mem::offset_of!(Transform, qw), 0.0),
            },
        ],
    )?;
    super::world::render_frame(renderer, &mut world, WIDTH, HEIGHT)?;
    let rotated = capture()?;
    save(output, "plane-rotated", &rotated)?;
    assert_eq!(
        color_count(&rotated, [137, 137, 137, 255]),
        0,
        "back face must be culled"
    );
    assert_arrow(&rotated, true);

    super::world::render_frame(
        renderer,
        &mut super::world::empty_world(&mut ipp_core::HostRuntime::new()),
        WIDTH,
        HEIGHT,
    )?;
    let mut outline_host = ipp_core::HostRuntime::new();
    let mut outline = load(&mut outline_host, renderer, fixtures, "plane-outline")?;
    super::world::render_frame(renderer, &mut outline, WIDTH, HEIGHT)?;
    let pixels = capture()?;
    save(output, "plane-outline", &pixels)?;
    assert!(
        coverage(&pixels).0 < coverage(&filled).0 / 2,
        "outline keeps the square open"
    );
    assert_arrow(&pixels, false);
    let mut paths = vec![];
    for axis in 0..2 {
        for side in [-1.0, 1.0] {
            paths.push(
                (0..=64)
                    .map(|i| {
                        let mut p = [0.0; 3];
                        p[axis] = -1.0 + 2.0 * i as f64 / 64.0;
                        p[1 - axis] = side;
                        p
                    })
                    .collect(),
            );
        }
    }
    paths.push(
        (0..=64)
            .map(|i| [0.0, 0.0, 0.1 + 1.25 * i as f64 / 64.0])
            .collect(),
    );
    paths.push(
        (0..=64)
            .map(|i| {
                let a = TAU * i as f64 / 64.0;
                [0.1 * a.cos(), 0.1 * a.sin(), 1.15]
            })
            .collect(),
    );
    // Independent cone generators bound the projected head's filled triangles.
    for side in 0..8 {
        let a = TAU * side as f64 / 8.0;
        paths.push(vec![[0.1 * a.cos(), 0.1 * a.sin(), 1.15], [0.0, 0.0, 1.35]]);
    }
    let (pixels_on_paths, samples) = assert_contours(&pixels, &paths, "plane-outline");
    std::fs::write(
        output.join("plane-samples.txt"),
        format!(
            "contour_pixels={pixels_on_paths}\ncurve_samples={samples}\npositive_normal_and_rotated_normal=passed\n"
        ),
    )?;
    super::world::render_frame(
        renderer,
        &mut super::world::empty_world(&mut ipp_core::HostRuntime::new()),
        WIDTH,
        HEIGHT,
    )?;
    assert_eq!(coverage(&capture()?).0, 0);
    Ok(())
}

fn assert_arrow(pixels: &[u8], rotated: bool) {
    for index in 2..=20 {
        let z = (0.1 + 1.25 * index as f64 / 20.0)
            * if rotated {
                -1.0
            } else {
                1.0
            };
        let [x, y] = project([0.0, 0.0, z]);
        let (x, y) = (x.round() as i32, y.round() as i32);
        let hit = (-2..=2).any(|dy| {
            (-2..=2).any(|dx| {
                let (x, y) = (x + dx, y + dy);
                x >= 0
                    && y >= 0
                    && x < WIDTH as i32
                    && y < HEIGHT as i32
                    && matches(
                        pixels[(y as usize * WIDTH as usize + x as usize) * 4..][..4]
                            .try_into()
                            .unwrap(),
                        &[255; 4],
                    )
            })
        });
        assert!(
            hit,
            "normal arrow missing at projected +Z/-Z sample {index}, rotated={rotated}"
        );
    }
}

fn load<'a, D: RenderDevice>(
    host: &'a mut ipp_core::HostRuntime,
    renderer: &mut RenderService<D>,
    fixtures: &Path,
    name: &str,
) -> Result<WorldContext<'a>> {
    renderer.install(host)?;
    let mut world = super::world::fixture_world(host)?;
    apply(
        &mut world,
        vec![
            Command::Create {
                alias: 1,
                metadata: Default::default(),
            },
            Command::InsertComponent {
                entity: EntityRef::Alias(1),
                component: ComponentValue::TRANSFORM,
                fields: vec![],
            },
            Command::InsertComponent {
                entity: EntityRef::Alias(1),
                component: ComponentValue::UNLIT_MATERIAL,
                fields: vec![],
            },
            Command::InsertComponent {
                entity: EntityRef::Alias(1),
                component: ComponentValue::MESH_INSTANCE,
                fields: vec![FieldWrite {
                    offset: std::mem::offset_of!(MeshInstance, source) as u32,
                    value: FieldValue::String(format!("fixture:///{name}.mesh")),
                }],
            },
        ],
    )?;
    assert!(world.render_items().is_empty());
    let id = world.id();
    drop(world);
    super::world::deliver_to_host(
        renderer,
        host,
        id,
        &std::fs::read(fixtures.join(format!("{name}.mesh")))?,
        None,
    )?;
    Ok(host.world_mut(id).unwrap())
}

// Analytic centreline paths, independent of decoded mesh vertices and indices.
fn curves(name: &str) -> Vec<Vec<Point>> {
    match name {
        "cube" => {
            let mut paths = Vec::new();
            for axis in 0..3 {
                for a in [-1.0, 1.0] {
                    for b in [-1.0, 1.0] {
                        let mut p = [0.0; 3];
                        p[axis] = -1.0;
                        p[(axis + 1) % 3] = a;
                        p[(axis + 2) % 3] = b;
                        let mut q = p;
                        q[axis] = 1.0;
                        paths.push(
                            (0..=64)
                                .map(|i| {
                                    std::array::from_fn(|j| p[j] + (q[j] - p[j]) * i as f64 / 64.0)
                                })
                                .collect(),
                        );
                    }
                }
            }

            paths
        }
        "sphere" => (0..3)
            .map(|axis| {
                (0..=128)
                    .map(|i| {
                        let angle = TAU * i as f64 / 128.0;
                        let mut p = [0.0; 3];
                        p[(axis + 1) % 3] = angle.cos();
                        p[(axis + 2) % 3] = angle.sin();
                        p
                    })
                    .collect()
            })
            .collect(),
        "pill" => {
            let radius = 0.65;
            let half_body = 1.4 - radius;
            let mut paths: Vec<Vec<Point>> = [-half_body, half_body]
                .into_iter()
                .map(|y| {
                    (0..=128)
                        .map(|i| {
                            let a = TAU * i as f64 / 128.0;
                            [radius * a.cos(), y, radius * a.sin()]
                        })
                        .collect()
                })
                .collect();
            for axis in [0, 2] {
                let mut profile = Vec::new();
                for (offset, begin) in [(half_body, 0.0), (-half_body, std::f64::consts::PI)] {
                    for i in 0..=64 {
                        let a = begin + std::f64::consts::PI * i as f64 / 64.0;
                        let mut p = [0.0; 3];
                        p[axis] = radius * a.cos();
                        p[1] = offset + radius * a.sin();
                        profile.push(p);
                    }
                }
                profile.push(profile[0]);
                paths.push(profile);
            }

            paths
        }
        _ => unreachable!(),
    }
}

fn project(p: Point) -> [f64; 2] {
    // Fixed camera (3,2,5), Y up, looking at origin, vertical field of view 45°.
    let right = [5.0 / 34.0f64.sqrt(), 0.0, -3.0 / 34.0f64.sqrt()];
    let forward = [
        -3.0 / 38.0f64.sqrt(),
        -2.0 / 38.0f64.sqrt(),
        -5.0 / 38.0f64.sqrt(),
    ];
    let up = [
        right[1] * forward[2] - right[2] * forward[1],
        right[2] * forward[0] - right[0] * forward[2],
        right[0] * forward[1] - right[1] * forward[0],
    ];
    let relative = [p[0] - 3.0, p[1] - 2.0, p[2] - 5.0];
    let dot = |v: Point| relative.iter().zip(v).map(|(a, b)| a * b).sum::<f64>();
    let focal = HEIGHT as f64 / (2.0 * (std::f64::consts::PI / 8.0).tan());
    [
        WIDTH as f64 / 2.0 + focal * dot(right) / dot(forward),
        HEIGHT as f64 / 2.0 - focal * dot(up) / dot(forward),
    ]
}

fn assert_contours(pixels: &[u8], curves: &[Vec<Point>], name: &str) -> (usize, usize) {
    let paths: Vec<Vec<_>> = curves
        .iter()
        .map(|c| c.iter().copied().map(project).collect())
        .collect();
    let lit = |x: usize, y: usize| {
        !matches(
            pixels[y * WIDTH as usize * 4 + x * 4..][..4]
                .try_into()
                .unwrap(),
            &[10, 14, 20, 255],
        )
    };
    let mut on_curve = 0;
    let mut off_curve = 0;
    for y in 0..HEIGHT as usize {
        for x in 0..WIDTH as usize {
            if !lit(x, y) {
                continue;
            }

            let p = [x as f64 + 0.5, y as f64 + 0.5];
            let near = paths.iter().flat_map(|c| c.windows(2)).any(|s| {
                let d = [s[1][0] - s[0][0], s[1][1] - s[0][1]];
                let length = d[0] * d[0] + d[1] * d[1];
                let t = (((p[0] - s[0][0]) * d[0] + (p[1] - s[0][1]) * d[1]) / length.max(1e-12))
                    .clamp(0.0, 1.0);
                (p[0] - s[0][0] - t * d[0]).hypot(p[1] - s[0][1] - t * d[1]) <= 2.5
            });
            if near {
                on_curve += 1;
            } else {
                off_curve += 1;
            }
        }
    }

    assert_eq!(
        off_curve, 0,
        "{name}: unexpected lines outside clean analytic contours"
    );

    let mut samples = 0;
    for p in paths.iter().flatten() {
        let x = p[0].round() as i32;
        let y = p[1].round() as i32;
        let found = (-2..=2).any(|dy| {
            (-2..=2).any(|dx| {
                let (x, y) = (x + dx, y + dy);
                x >= 0
                    && y >= 0
                    && x < WIDTH as i32
                    && y < HEIGHT as i32
                    && lit(x as usize, y as usize)
            })
        });
        assert!(found, "{name}: missing contour near {p:?}");
        samples += 1;
    }

    (on_curve, samples)
}
