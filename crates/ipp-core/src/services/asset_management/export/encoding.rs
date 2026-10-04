//! Versioned semantic encoding from the actual retained CPU type.

use super::{AssetExportFormat, AssetOutput};
use crate::services::asset_management::{
    expression::ExpressionAsset,
    mesh::MeshAsset,
    shader::ShaderDefinition,
    skeleton::{PoseAsset, SkeletonAsset},
    skin_binding::SkinAsset,
    texture::TextureAsset,
};
use crate::systems::{
    animation::AnimationClip,
    geometry::{GeometryDefinition, GeometryShape},
    particles::ParticleCache,
};
use std::any::Any;

macro_rules! asset_formats {
    ($($format:ident => $asset:ty),* $(,)?) => {
        /// Explicit combinations for which this payload contains a complete semantic representation.
        pub fn cpu_formats(data: &dyn Any) -> Vec<AssetExportFormat> {
            let mut formats = Vec::new();
            $(if data.is::<$asset>() { formats.push(AssetExportFormat::$format); })*
            formats
        }
    };
}

asset_formats! {
    MeshV3 => MeshAsset,
    TextureV3 => TextureAsset,
    SkeletonV1 => SkeletonAsset,
    PoseV1 => PoseAsset,
    SkinV1 => SkinAsset,
    ShaderV3 => ShaderDefinition,
    AnimationV4 => AnimationClip,
    GeometryV1 => GeometryDefinition,
    ParticleCacheV1 => ParticleCache,
    ExpressionV1 => ExpressionAsset,
}

pub(super) async fn encode(
    data: &dyn Any,
    format: AssetExportFormat,
    out: &mut AssetOutput,
) -> Result<(), String> {
    macro_rules! payload {
        ($type:ty) => {
            data.downcast_ref::<$type>()
                .ok_or("Requested format has no CPU representation")?
        };
    }
    match format {
        AssetExportFormat::MeshV3 => mesh(payload!(MeshAsset), out).await,
        AssetExportFormat::TextureV3 => {
            let texture = payload!(TextureAsset);
            header(out, b"IPPT", 3, &[texture.width(), texture.height()]).await?;
            out.write(texture.pixels()).await
        }
        AssetExportFormat::SkeletonV1 => {
            let skeleton = payload!(SkeletonAsset);
            header(out, b"IPPS", 1, &[count(skeleton.joints().len())?]).await?;
            for joint in skeleton.joints() {
                out.u32(joint.parent.map_or(u32::MAX, |index| index as u32))
                    .await?;
                transform(out, &joint.rest).await?;
            }
            Ok(())
        }
        AssetExportFormat::PoseV1 => {
            let pose = payload!(PoseAsset);
            header(out, b"IPPP", 1, &[count(pose.joints().len())?]).await?;
            for joint in pose.joints() {
                transform(out, joint).await?;
            }
            Ok(())
        }
        AssetExportFormat::SkinV1 => {
            let skin = payload!(SkinAsset);
            header(out, b"IPPB", 1, &[count(skin.joints().len())?]).await?;
            for joint in skin.joints() {
                out.count(joint.joint).await?;
                out.floats(&joint.inverse_bind).await?;
            }
            Ok(())
        }
        AssetExportFormat::ShaderV3 => {
            let shader = payload!(ShaderDefinition);
            // Registered immutable definitions have already passed semantic validation.
            header(out, b"IPPH", 3, &[shader.recipe.features]).await?;
            out.text(&shader.recipe.backend).await?;
            out.u32(shader.required_attributes).await?;
            out.count(shader.parameters.len()).await?;
            for (name, kind) in &shader.parameters {
                out.text(name).await?;
                out.byte(*kind as u8).await?;
            }
            out.count(shader.backends.len()).await?;
            for (name, backend) in &shader.backends {
                out.text(name).await?;
                out.text(&backend.vertex).await?;
                out.text(&backend.fragment).await?;
                out.text(&backend.paint).await?;
            }
            Ok(())
        }
        AssetExportFormat::AnimationV4 => payload!(AnimationClip).encode_output(out).await,
        AssetExportFormat::GeometryV1 => geometry(payload!(GeometryDefinition), out).await,
        AssetExportFormat::ParticleCacheV1 => cache(payload!(ParticleCache), out).await,
        AssetExportFormat::ExpressionV1 => {
            payload!(ExpressionAsset)
                .declaration()
                .encode_output(out)
                .await
        }
    }
}

fn count(value: usize) -> Result<u32, String> {
    u32::try_from(value).map_err(|_| "Asset count overflow".into())
}

pub(crate) async fn header(
    out: &mut AssetOutput,
    magic: &[u8; 4],
    version: u32,
    words: &[u32],
) -> Result<(), String> {
    out.write(magic).await?;
    out.u32(version).await?;
    for word in words {
        out.u32(*word).await?;
    }
    Ok(())
}

pub(crate) async fn transform(
    out: &mut AssetOutput,
    t: &crate::components::Transform,
) -> Result<(), String> {
    out.floats(&[t.x, t.y, t.z, t.qx, t.qy, t.qz, t.qw, t.sx, t.sy, t.sz])
        .await
}

async fn mesh(mesh: &MeshAsset, out: &mut AssetOutput) -> Result<(), String> {
    enum Stream<'a> {
        Floats(&'a [f32]),
        Bytes(&'a [u8]),
    }
    let mut streams = vec![(0u8, 1u8, Stream::Floats(mesh.positions().as_flattened()))];
    if let Some(values) = mesh.colors() {
        streams.push((1, 1, Stream::Floats(values.as_flattened())));
    }
    if let Some(values) = mesh.uvs() {
        streams.push((2, 2, Stream::Floats(values.as_flattened())));
    }
    if let Some(values) = mesh.texture_weights() {
        streams.push((3, 3, Stream::Bytes(values)));
    }
    if let Some(values) = mesh.normals() {
        streams.push((4, 1, Stream::Floats(values.as_flattened())));
    }
    if let Some(values) = mesh.joint_indices() {
        streams.push((5, 4, Stream::Bytes(values.as_flattened())));
    }
    if let Some(values) = mesh.joint_weights() {
        streams.push((6, 5, Stream::Floats(values.as_flattened())));
    }
    header(
        out,
        b"IPPM",
        3,
        &[
            count(mesh.vertex_count())?,
            count(mesh.indices().len())?,
            count(streams.len())?,
        ],
    )
    .await?;
    for (semantic, format, stream) in &streams {
        out.write(&[*semantic, *format, 0, 0]).await?;
        out.count(match stream {
            Stream::Floats(values) => std::mem::size_of_val(*values),
            Stream::Bytes(values) => values.len(),
        })
        .await?;
    }
    for (_, _, stream) in streams {
        match stream {
            Stream::Floats(values) => out.floats(values).await?,
            Stream::Bytes(values) => out.write(values).await?,
        }
    }
    for index in mesh.indices() {
        out.write(&index.to_le_bytes()).await?;
    }
    Ok(())
}

async fn geometry(geometry: &GeometryDefinition, out: &mut AssetOutput) -> Result<(), String> {
    if geometry.parts.is_empty() {
        return Err("Invalid empty geometry export".into());
    }
    header(out, b"IPPG", 1, &[count(geometry.parts.len())?]).await?;
    for part in &geometry.parts {
        part.validate()
            .map_err(|error| format!("Invalid geometry export: {error:?}"))?;
        let (tag, values) = match part.shape {
            GeometryShape::Box {
                min,
                max,
            } => (0, [min[0], min[1], min[2], max[0], max[1], max[2], 0.0]),
            GeometryShape::Sphere {
                center,
                radius,
            } => (1, [center[0], center[1], center[2], radius, 0.0, 0.0, 0.0]),
            GeometryShape::Pill {
                start,
                end,
                radius,
            } => (
                2,
                [start[0], start[1], start[2], end[0], end[1], end[2], radius],
            ),
        };
        let converted = values.map(|value| value as f32);
        let narrowed = converted.map(f64::from);
        let shape = match tag {
            0 => GeometryShape::Box {
                min: [narrowed[0], narrowed[1], narrowed[2]],
                max: [narrowed[3], narrowed[4], narrowed[5]],
            },
            1 => GeometryShape::Sphere {
                center: [narrowed[0], narrowed[1], narrowed[2]],
                radius: narrowed[3],
            },
            _ => GeometryShape::Pill {
                start: [narrowed[0], narrowed[1], narrowed[2]],
                end: [narrowed[3], narrowed[4], narrowed[5]],
                radius: narrowed[6],
            },
        };
        shape
            .validate()
            .map_err(|_| "Geometry cannot be represented in GeometryV1")?;
        out.u32(tag).await?;
        out.floats(&converted).await?;
        transform(out, &part.transform).await?;
        for joint in part.joints.unwrap_or([u32::MAX; 2]) {
            out.u32(joint).await?;
        }
    }
    Ok(())
}

async fn cache(cache: &ParticleCache, out: &mut AssetOutput) -> Result<(), String> {
    header(out, b"IPPC", 1, &[cache.space, count(cache.frames.len())?]).await?;
    let mut offset = 16usize
        .checked_add(
            cache
                .frames
                .len()
                .checked_mul(12)
                .ok_or("Cache output overflow")?,
        )
        .ok_or("Cache output overflow")?;
    for frame in &cache.frames {
        out.write(&frame.time.to_le_bytes()).await?;
        out.count(offset).await?;
        out.count(frame.samples.len()).await?;
        offset = offset
            .checked_add(
                frame
                    .samples
                    .len()
                    .checked_mul(60)
                    .ok_or("Cache output overflow")?,
            )
            .ok_or("Cache output overflow")?;
    }
    for frame in &cache.frames {
        for p in &frame.samples {
            out.write(&p.id.to_le_bytes()).await?;
            out.floats(&[p.birth, p.death]).await?;
            out.floats(&p.position).await?;
            out.floats(&p.velocity).await?;
            out.floats(&p.rotation).await?;
            out.floats(&[p.size]).await?;
        }
    }
    Ok(())
}

/// Write the versioned IPPT header for a validated RGBA8 payload.
/// GPU exporters fill its exact payload range without owning encoding semantics.
pub async fn write_texture_header(
    output: &mut AssetOutput,
    texture: crate::TextureHeader,
) -> Result<(), String> {
    let bytes =
        crate::services::asset_management::texture::pixel_bytes(texture.width, texture.height)
            .map_err(|error| error.to_string())?;
    if bytes != texture.pixel_bytes {
        return Err("Texture export header length mismatch".into());
    }
    header(output, b"IPPT", 3, &[texture.width, texture.height]).await
}
