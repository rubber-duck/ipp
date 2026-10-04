//! The actual async decoders validate equally over owned backing and fragmented
//! single-buffer streams. Transport/rendering scenarios cover Host integration.

use ipp_core::{
    MeshAsset, PoseAsset, SkeletonAsset, SkinAsset, TextureAsset,
    expressions::{ExpressionDeclaration, ExpressionNode, PreparedExpression},
    services::{
        asset_management::{
            drawing::DrawingAsset,
            font::FontAsset,
            shader::{ShaderBackendSource, ShaderDefinition},
        },
        io::{BufferIoReader, IoReadOptions, IoReader, StreamIoReader},
    },
    systems::{animation::AnimationClip, geometry::GeometryDefinition, particles::ParticleCache},
};
use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
};

#[derive(Default)]
struct Ready(AtomicBool);

impl Wake for Ready {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[derive(Clone, Copy, Debug)]
enum Format {
    Mesh,
    Texture,
    Font,
    Drawing,
    Skeleton,
    Pose,
    Skin,
    Shader,
    Expression,
    Animation,
    Geometry,
    Cache,
}

impl Format {
    fn sync(self, bytes: &[u8]) -> Result<String, ()> {
        macro_rules! decode {
            ($ty:ty) => {
                <$ty>::decode(bytes)
                    .map(|asset| format!("{asset:?}"))
                    .map_err(|_| ())
            };
        }
        match self {
            Self::Mesh => decode!(MeshAsset),
            Self::Texture => decode!(TextureAsset),
            Self::Font => decode!(FontAsset),
            Self::Drawing => decode!(DrawingAsset),
            Self::Skeleton => decode!(SkeletonAsset),
            Self::Pose => decode!(PoseAsset),
            Self::Skin => decode!(SkinAsset),
            Self::Shader => decode!(ShaderDefinition),
            Self::Expression => ExpressionDeclaration::decode(bytes)
                .and_then(|declaration| {
                    PreparedExpression::prepare(&declaration)?;
                    Ok(format!("{declaration:?}"))
                })
                .map_err(|_| ()),
            Self::Animation => decode!(AnimationClip),
            Self::Geometry => decode!(GeometryDefinition),
            Self::Cache => decode!(ParticleCache),
        }
    }

    async fn decode(self, reader: &mut dyn IoReader) -> Result<String, String> {
        macro_rules! decode {
            ($ty:ty) => {
                <$ty>::decode_reader(reader)
                    .await
                    .map(|asset| format!("{asset:?}"))
            };
        }
        match self {
            Self::Mesh => decode!(MeshAsset),
            Self::Texture => decode!(TextureAsset),
            Self::Font => decode!(FontAsset),
            Self::Drawing => decode!(DrawingAsset),
            Self::Skeleton => decode!(SkeletonAsset),
            Self::Pose => decode!(PoseAsset),
            Self::Skin => decode!(SkinAsset),
            Self::Shader => decode!(ShaderDefinition),
            Self::Expression => {
                let declaration = ExpressionDeclaration::decode_reader(reader).await?;
                PreparedExpression::prepare_async(&declaration)
                    .await
                    .map_err(|error| format!("{error:?}"))?;
                Ok(format!("{declaration:?}"))
            }
            Self::Animation => decode!(AnimationClip),
            Self::Geometry => decode!(GeometryDefinition),
            Self::Cache => decode!(ParticleCache),
        }
    }
}

fn memory(format: Format, bytes: &[u8]) -> Result<String, String> {
    let mut reader = BufferIoReader::new(bytes.to_vec());
    let ready = Arc::new(Ready::default());
    let waker = Waker::from(ready.clone());
    let mut cx = Context::from_waker(&waker);
    let mut decode = std::pin::pin!(format.decode(&mut reader));
    for _ in 0..10_000 {
        ready.0.store(false, Ordering::SeqCst);
        match decode.as_mut().poll(&mut cx) {
            Poll::Ready(result) => return result,
            Poll::Pending => assert!(
                ready.0.load(Ordering::SeqCst),
                "ready CPU work must schedule its continuation"
            ),
        }
    }
    panic!("decoder never completed");
}

fn fragmented(format: Format, bytes: &[u8], chunks: &[usize]) -> Result<String, String> {
    let (mut reader, input) = StreamIoReader::new(IoReadOptions {
        max_bytes: None,
        recovery: false,
    });
    let ready = Arc::new(Ready::default());
    let waker = Waker::from(ready.clone());
    let mut cx = Context::from_waker(&waker);
    let mut decode = std::pin::pin!(format.decode(&mut reader));
    let mut offset = 0;
    let mut chunk = 0;
    let mut finished = false;
    for _ in 0..10_000 {
        ready.0.store(false, Ordering::SeqCst);
        match decode.as_mut().poll(&mut cx) {
            Poll::Ready(result) => {
                if result.is_ok() {
                    assert!(finished, "no publication before source EOF");
                }
                return result;
            }
            Poll::Pending => {
                if offset < bytes.len() {
                    let mut length = chunks[chunk % chunks.len()].min(bytes.len() - offset);
                    while length != 0 && !input.push(&bytes[offset..offset + length]).unwrap() {
                        length /= 2;
                    }
                    if length == 0 {
                        assert!(
                            ready.0.load(Ordering::SeqCst),
                            "backpressure must leave runnable decoder work"
                        );
                    } else {
                        offset += length;
                        chunk += 1;
                    }
                } else if !finished {
                    input.finish(Ok(()));
                    finished = true;
                } else {
                    assert!(
                        ready.0.load(Ordering::SeqCst),
                        "decoder stalled after final input"
                    );
                }
            }
        }
    }
    panic!("fragmented decoder never completed");
}

fn words(bytes: &mut Vec<u8>, values: &[u32]) {
    for value in values {
        bytes.extend(value.to_le_bytes());
    }
}

fn floats(bytes: &mut Vec<u8>, values: &[f32]) {
    for value in values {
        bytes.extend(value.to_le_bytes());
    }
}

fn fixtures() -> Vec<(Format, Vec<u8>)> {
    let mut fixtures = Vec::new();
    let mut mesh = b"IPPM".to_vec();
    words(&mut mesh, &[3, 3, 3, 1]);
    mesh.extend([0, 1, 0, 0]);
    words(&mut mesh, &[36]);
    floats(&mut mesh, &[0., 0., 0., 1., 0., 0., 0., 1., 0.]);
    for index in [0u16, 1, 2] {
        mesh.extend(index.to_le_bytes());
    }
    fixtures.push((Format::Mesh, mesh));
    for version in [1, 2] {
        let mut mesh = b"IPPM".to_vec();
        words(&mut mesh, &[version, 3, 3]);
        for point in [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]] {
            floats(&mut mesh, &point);
            floats(&mut mesh, &[0.25, 0.5, 1.]);
            if version == 2 {
                floats(&mut mesh, &[0.25, 0.75]);
            }
        }
        for index in [0u16, 1, 2] {
            mesh.extend(index.to_le_bytes());
        }
        fixtures.push((Format::Mesh, mesh));
    }
    let mut mesh = b"IPPM".to_vec();
    words(&mut mesh, &[3, 3, 3, 7]);
    for (semantic, format, width) in [
        (0, 1, 12),
        (1, 1, 12),
        (2, 2, 8),
        (3, 3, 1),
        (4, 1, 12),
        (5, 4, 4),
        (6, 5, 16),
    ] {
        mesh.extend([semantic, format, 0, 0]);
        words(&mut mesh, &[width * 3]);
    }
    floats(&mut mesh, &[0., 0., 0., 1., 0., 0., 0., 1., 0.]);
    floats(&mut mesh, &[0.25, 0.5, 1., 1., 0., 0., 0., 1., 0.]);
    floats(&mut mesh, &[0., 0., 1., 0., 0., 1.]);
    mesh.extend([0, 127, 255]);
    floats(&mut mesh, &[0., 0., 1., 0., 0., 1., 0., 0., 1.]);
    mesh.extend([0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0]);
    floats(
        &mut mesh,
        &[0.75, 0.25, 0., 0., 0.75, 0.25, 0., 0., 0.75, 0.25, 0., 0.],
    );
    for index in [0u16, 1, 2] {
        mesh.extend(index.to_le_bytes());
    }
    fixtures.push((Format::Mesh, mesh));
    let mut texture = b"IPPT".to_vec();
    words(&mut texture, &[3, 2, 1]);
    texture.extend([0, 20, 40, 255, 60, 80, 100, 128]);
    fixtures.push((Format::Texture, texture));
    let mut font = b"IPPF".to_vec();
    words(&mut font, &[1, 1000]);
    floats(&mut font, &[800., -200., 100.]);
    words(&mut font, &[1, 1, 0]);
    floats(&mut font, &[500., 10., 0., 0., 0., 0.]);
    words(&mut font, &[0, 65, 0]);
    fixtures.push((Format::Font, font));
    let mut drawing = b"IPPD".to_vec();
    words(&mut drawing, &[1]);
    floats(&mut drawing, &[0., 0., 10., 10., 0., 0., 10., 10., 0.05]);
    words(&mut drawing, &[1]);
    drawing.extend([10, 20, 30, 128, 1, 0, 0, 0]);
    words(&mut drawing, &[1]);
    drawing.extend([0; 8]);
    words(&mut drawing, &[3]);
    for point in [[10., 0.], [0., 10.], [0., 0.]] {
        drawing.extend([0; 4]);
        floats(&mut drawing, &point);
    }
    fixtures.push((Format::Drawing, drawing));
    for (format, magic) in [(Format::Skeleton, b"IPPS"), (Format::Pose, b"IPPP")] {
        let mut bytes = magic.to_vec();
        words(&mut bytes, &[1, 1]);
        if matches!(format, Format::Skeleton) {
            words(&mut bytes, &[u32::MAX]);
        }
        floats(&mut bytes, &[0., 0., 0., 0., 0., 0., 1., 1., 1., 1.]);
        fixtures.push((format, bytes));
    }
    let mut skin = b"IPPB".to_vec();
    words(&mut skin, &[1, 1, 0]);
    floats(
        &mut skin,
        &[
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ],
    );
    fixtures.push((Format::Skin, skin));
    let mut shader = ShaderDefinition::default();
    shader.backends.insert(
        "glsl-es-300".into(),
        ShaderBackendSource {
            vertex: "// α😀".into(),
            fragment: "return vec4(1.0); // β".into(),
            paint: String::new(),
        },
    );
    fixtures.push((Format::Shader, shader.encode().unwrap()));
    let expression = ExpressionDeclaration {
        inputs: vec![],
        nodes: vec![ExpressionNode::Constant(ipp_core::DynamicValue::F32(2.))],
        output: 0,
    };
    fixtures.push((Format::Expression, expression.encode().unwrap()));
    use ipp_core::systems::animation::{
        AnimationEntityPlacementKey, AnimationInterpolation, AnimationKeyframe, AnimationTrack,
        AnimationTrackTarget, AnimationValue,
    };
    let clip = AnimationClip::new(
        1.,
        vec![AnimationTrack {
            target: AnimationTrackTarget::EntityLink,
            keys: vec![AnimationKeyframe {
                time: 0.,
                value: AnimationValue::EntityPlacement(AnimationEntityPlacementKey::default()),
                interpolation: AnimationInterpolation::Step,
            }],
        }],
    )
    .unwrap();
    fixtures.push((Format::Animation, clip.encode()));
    let geometry = GeometryDefinition::from(ipp_core::systems::geometry::GeometryShape::Sphere {
        center: [0.; 3],
        radius: 1.,
    });
    fixtures.push((Format::Geometry, geometry.encode().unwrap()));
    use ipp_core::systems::particles::{ParticleCacheFrame, ParticleCacheSample};
    let cache = ParticleCache {
        space: 0,
        frames: vec![ParticleCacheFrame {
            time: 0.,
            samples: vec![ParticleCacheSample {
                id: 7,
                birth: 0.,
                death: 2.,
                position: [0.; 3],
                velocity: [0.; 3],
                rotation: [0., 0., 0., 1.],
                size: 1.,
            }],
        }],
    };
    fixtures.push((Format::Cache, cache.encode().unwrap()));
    fixtures
}

#[test]
fn valid_formats_match_at_every_byte_split_and_small_fragments() {
    for (format, bytes) in fixtures() {
        let expected = format.sync(&bytes).unwrap();
        assert_eq!(memory(format, &bytes).unwrap(), expected, "{format:?}");
        for split in 1..bytes.len() {
            assert_eq!(
                fragmented(format, &bytes, &[split, bytes.len()]).unwrap(),
                expected,
                "{format:?} split {split}"
            );
        }
        for chunk in [1, 2, 7] {
            assert_eq!(
                fragmented(format, &bytes, &[chunk]).unwrap(),
                expected,
                "{format:?} chunk {chunk}"
            );
        }
    }
}

#[test]
fn truncation_header_and_trailing_errors_never_publish_partial_output() {
    for (format, bytes) in fixtures() {
        for length in 0..bytes.len() {
            let truncated = &bytes[..length];
            assert!(
                format.sync(truncated).is_err(),
                "{format:?} length {length}"
            );
            assert!(
                memory(format, truncated).is_err(),
                "{format:?} length {length}"
            );
            assert!(
                fragmented(format, truncated, &[1, 5, 2]).is_err(),
                "{format:?} length {length}"
            );
        }
        for offset in [0, 4] {
            let mut invalid = bytes.clone();
            invalid[offset] = 255;
            assert!(format.sync(&invalid).is_err());
            assert!(memory(format, &invalid).is_err());
            assert!(fragmented(format, &invalid, &[1]).is_err());
        }
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(format.sync(&trailing).is_err());
        assert!(memory(format, &trailing).is_err());
        assert!(fragmented(format, &trailing, &[7]).is_err());
    }
}

#[test]
fn malformed_records_keep_sync_validation_parity() {
    for (format, bytes) in fixtures() {
        // Each individual record byte is independently corrupted. Both decoders
        // must agree even where the changed byte remains a valid authored value.
        for offset in 8..bytes.len() {
            let mut changed = bytes.clone();
            changed[offset] ^= 0x80;
            let expected = format.sync(&changed);
            assert_eq!(
                memory(format, &changed).is_ok(),
                expected.is_ok(),
                "{format:?} offset {offset}"
            );
            assert_eq!(
                fragmented(format, &changed, &[3, 1, 9]).is_ok(),
                expected.is_ok(),
                "{format:?} offset {offset}"
            );
        }
    }
}

#[test]
fn long_utf8_strings_cross_decode_quantums_and_ready_work_yields() {
    let mut definition = ShaderDefinition::default();
    definition.backends.insert(
        "glsl-es-300".into(),
        ShaderBackendSource {
            vertex: "a".repeat((64 << 10) - 1) + "😀α終",
            fragment: String::new(),
            paint: String::new(),
        },
    );
    let bytes = definition.encode().unwrap();
    let expected = Format::Shader.sync(&bytes).unwrap();
    assert_eq!(memory(Format::Shader, &bytes).unwrap(), expected);
    assert_eq!(
        fragmented(Format::Shader, &bytes, &[4093, 17, 2000]).unwrap(),
        expected
    );
    let mut invalid = bytes.clone();
    let utf8 = invalid
        .windows(4)
        .position(|bytes| bytes == "😀".as_bytes())
        .unwrap();
    invalid[utf8 + 1] = 0xff;
    assert!(memory(Format::Shader, &invalid).is_err());
    assert!(fragmented(Format::Shader, &invalid, &[4093]).is_err());
}

#[test]
fn cancelling_an_owned_loader_releases_acquisition_without_publication() {
    use ipp_core::services::asset_management::{AssetLoader, AsyncAssetLoader};
    let bytes = fixtures()
        .into_iter()
        .find(|(format, _)| matches!(format, Format::Texture))
        .unwrap()
        .1;
    let (reader, input) = StreamIoReader::new(IoReadOptions {
        max_bytes: None,
        recovery: false,
    });
    let mut loader = AsyncAssetLoader::decode(|mut reader| async move {
        TextureAsset::decode_reader(&mut *reader).await
    });
    loader.start_load(Box::new(reader)).unwrap();
    let waker = Waker::from(Arc::new(Ready::default()));
    let mut cx = Context::from_waker(&waker);
    assert!(loader.poll_load(&mut cx).is_pending());
    assert!(input.push(&bytes).unwrap());
    // Even the complete declared payload cannot publish until the source ends.
    assert!(loader.poll_load(&mut cx).is_pending());
    assert!(loader.take_failed_data().is_none());
    drop(loader);
    assert!(!input.is_open());
}

#[derive(Default)]
struct ExportBudget {
    capacity: std::cell::Cell<usize>,
    cancelled: std::cell::Cell<bool>,
}

impl ipp_core::services::asset_management::export::AssetOutputObserver for ExportBudget {
    fn reserve(&self, bytes: usize) -> Result<(), String> {
        self.capacity.set(bytes);
        self.check()
    }

    fn check(&self) -> Result<(), String> {
        if self.cancelled.get() {
            Err("Output pressure revoked export".into())
        } else {
            Ok(())
        }
    }
}

fn cpu_payload(format: Format, bytes: &[u8]) -> Option<(std::rc::Rc<dyn std::any::Any>, Vec<u8>)> {
    use ipp_core::services::asset_management::writer::AssetEncoder;
    macro_rules! payload {
        ($asset:ty) => {{
            let data = <$asset>::decode(bytes).unwrap();
            let encoded = data.encode_asset(usize::MAX).unwrap();
            (
                std::rc::Rc::new(data) as std::rc::Rc<dyn std::any::Any>,
                encoded,
            )
        }};
    }
    Some(match format {
        Format::Mesh => payload!(MeshAsset),
        Format::Texture => payload!(TextureAsset),
        Format::Skeleton => payload!(SkeletonAsset),
        Format::Pose => payload!(PoseAsset),
        Format::Skin => payload!(SkinAsset),
        Format::Shader => payload!(ShaderDefinition),
        Format::Geometry => payload!(GeometryDefinition),
        Format::Animation => {
            let data = AnimationClip::decode(bytes).unwrap();
            let encoded = data.encode();
            (std::rc::Rc::new(data), encoded)
        }
        Format::Cache => {
            let data = ParticleCache::decode(bytes).unwrap();
            let encoded = data.encode().unwrap();
            (std::rc::Rc::new(data), encoded)
        }
        Format::Expression => {
            let data =
                ipp_core::services::asset_management::expression::ExpressionAsset::decode(bytes)
                    .unwrap();
            let encoded = data.declaration().encode().unwrap();
            (std::rc::Rc::new(data), encoded)
        }
        Format::Font | Format::Drawing => return None,
    })
}

#[test]
fn cooperative_cpu_exports_match_existing_semantic_encoders_and_reload() {
    use ipp_core::services::asset_management::export::{
        AssetCpuSnapshot, cpu_formats, encode_cpu_snapshot,
    };
    for (format, bytes) in fixtures() {
        let Some((data, expected)) = cpu_payload(format, &bytes) else {
            continue;
        };
        let formats = cpu_formats(&*data);
        assert_eq!(formats.len(), 1, "{format:?}");
        let budget = std::rc::Rc::new(ExportBudget::default());
        let snapshot = AssetCpuSnapshot {
            data,
            available: Default::default(),
        };
        let ready = Arc::new(Ready::default());
        let waker = Waker::from(ready.clone());
        let mut cx = Context::from_waker(&waker);
        let mut encode = std::pin::pin!(encode_cpu_snapshot(snapshot, formats[0], budget.clone()));
        let encoded = loop {
            ready.0.store(false, Ordering::SeqCst);
            match encode.as_mut().poll(&mut cx) {
                Poll::Ready(result) => break result.unwrap(),
                Poll::Pending => assert!(ready.0.load(Ordering::SeqCst)),
            }
        };
        assert_eq!(encoded, expected, "{format:?}");
        assert!(budget.capacity.get() >= encoded.len());
        assert_eq!(format.sync(&encoded), format.sync(&bytes), "{format:?}");
        assert_eq!(
            fragmented(format, &encoded, &[1, 7, 31]).unwrap(),
            format.sync(&bytes).unwrap()
        );
    }
}

#[test]
fn unfinished_cpu_export_yields_and_observes_unload_and_pressure() {
    use ipp_core::services::asset_management::export::{
        AssetCpuSnapshot, AssetExportFormat, encode_cpu_snapshot,
    };
    let mut bytes = b"IPPT".to_vec();
    words(&mut bytes, &[3, 512, 512]);
    bytes.resize(16 + 512 * 512 * 4, 173);
    for pressure in [false, true] {
        let data = std::rc::Rc::new(TextureAsset::decode(&bytes).unwrap());
        let available = ipp_core::services::io::IoCancellation::default();
        let budget = std::rc::Rc::new(ExportBudget::default());
        let snapshot = AssetCpuSnapshot {
            data: data.clone(),
            available: available.clone(),
        };
        let ready = Arc::new(Ready::default());
        let waker = Waker::from(ready.clone());
        let mut cx = Context::from_waker(&waker);
        let mut encode = std::pin::pin!(encode_cpu_snapshot(
            snapshot,
            AssetExportFormat::TextureV3,
            budget.clone()
        ));
        assert!(encode.as_mut().poll(&mut cx).is_pending());
        assert!(ready.0.load(Ordering::SeqCst));
        assert!(
            budget.capacity.get() <= 128 << 10,
            "one poll must not assemble the full output"
        );
        if pressure {
            budget.cancelled.set(true);
        } else {
            available.cancel();
        }
        let result = loop {
            if let Poll::Ready(result) = encode.as_mut().poll(&mut cx) {
                break result;
            }
        };
        assert!(result.is_err());
        assert_eq!(
            data.pixels(),
            &bytes[16..],
            "retained memory remains safe after revocation"
        );
    }
}

#[test]
fn semantic_geometry_export_rejects_f32_radius_underflow_and_coordinate_overflow() {
    use ipp_core::services::asset_management::export::{
        AssetCpuSnapshot, AssetExportFormat, encode_cpu_snapshot,
    };
    use ipp_core::systems::geometry::GeometryShape;
    for shape in [
        GeometryShape::Sphere {
            center: [0.0; 3],
            radius: f64::MIN_POSITIVE,
        },
        GeometryShape::Sphere {
            center: [f64::MAX, 0.0, 0.0],
            radius: 1.0,
        },
        GeometryShape::Pill {
            start: [0.0; 3],
            end: [1.0, 0.0, 0.0],
            radius: f64::MIN_POSITIVE,
        },
    ] {
        let geometry = GeometryDefinition::from(shape);
        geometry.validate().unwrap();
        assert!(geometry.encode().is_err());
        let snapshot = AssetCpuSnapshot {
            data: std::rc::Rc::new(geometry),
            available: Default::default(),
        };
        let ready = Arc::new(Ready::default());
        let waker = Waker::from(ready);
        let mut cx = Context::from_waker(&waker);
        let mut future = std::pin::pin!(encode_cpu_snapshot(
            snapshot,
            AssetExportFormat::GeometryV1,
            std::rc::Rc::new(ExportBudget::default())
        ));
        loop {
            if let Poll::Ready(result) = future.as_mut().poll(&mut cx) {
                assert!(result.is_err());
                break;
            }
        }
    }
}
