//! Narrow connection authority and explicit original/semantic asset reads.

use crate::{
    ProtocolError,
    bulk_read::BulkReadDescriptor,
    codec::{Reader, Writer},
    wire::*,
};
use ipp_core::services::asset_management::{AssetSource, AssetTypeId, export::AssetExportFormat};

/// Opaque authority issued by one Host to one physical connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssetReadCapability {
    /// Owning connection, never transferable by copying the token.
    pub connection: u64,
    /// Exact Host grant incarnation.
    pub grant: u64,
}

/// Requested data owner; formats never implicitly select another representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetReadRepresentation {
    /// Exact authorized immutable source bytes, independently of decoding.
    Original,
    /// Complete retained semantic CPU payload.
    Cpu,
    /// Renderer-owned working GPU representation.
    Gpu,
}

/// Explicit allowed supported combinations for one typed immutable source.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AssetReadAccess {
    /// Whether original encoded input is allowed.
    pub original: bool,
    /// Versioned semantic CPU encodings the caller may request.
    pub cpu: Vec<AssetExportFormat>,
    /// Versioned semantic GPU encodings the caller may request.
    pub gpu: Vec<AssetExportFormat>,
}

impl AssetReadAccess {
    /// Permission check; a working payload may still be unavailable.
    pub fn allows(
        &self,
        representation: AssetReadRepresentation,
        format: Option<AssetExportFormat>,
    ) -> bool {
        match (representation, format) {
            (AssetReadRepresentation::Original, None) => self.original,
            (AssetReadRepresentation::Cpu, Some(format)) => self.cpu.contains(&format),
            (AssetReadRepresentation::Gpu, Some(format)) => self.gpu.contains(&format),
            _ => false,
        }
    }
}

/// Control operations carry only authority and references, never encoded asset bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssetExportRequest {
    /// Resolve an existing own/shared grant or an explicitly public Host policy entry.
    /// A source URI by itself never creates authority.
    Find(AssetSource),
    /// Open one exact authorized source or privately encode one working representation.
    Read {
        /// Grant issued for this connection and source lifetime.
        capability: AssetReadCapability,
        /// Explicit requested representation.
        representation: AssetReadRepresentation,
        /// None only for original input; otherwise one versioned semantic encoding.
        format: Option<AssetExportFormat>,
    },
    /// Revoke this connection's grant and its future reads/unfinished exports.
    Revoke(AssetReadCapability),
}

/// Successful control reply; byte delivery proceeds through ordinary bulk read leases.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssetExportResponse {
    /// Existing connection authority and its supported allowed representations.
    Capability {
        /// Exact grant, independent of producer ownership and unused cache demand.
        capability: AssetReadCapability,
        /// Public immutable typed source identity, never the internal routing name.
        source: AssetSource,
        /// Explicit allowed supported combinations.
        access: AssetReadAccess,
    },
    /// Complete detached semantic output, or an original exact source reader lease.
    Read {
        /// Connection-scoped delivery lease.
        read: BulkReadDescriptor,
        /// Representation actually requested and produced.
        representation: AssetReadRepresentation,
        /// Versioned semantic encoding, absent for original input.
        format: Option<AssetExportFormat>,
    },
    /// Grant revocation committed.
    Revoked,
}

fn representation(tag: u8) -> Result<AssetReadRepresentation, ProtocolError> {
    match tag {
        ASSET_REPRESENTATION_ORIGINAL => Ok(AssetReadRepresentation::Original),
        ASSET_REPRESENTATION_CPU => Ok(AssetReadRepresentation::Cpu),
        ASSET_REPRESENTATION_GPU => Ok(AssetReadRepresentation::Gpu),
        tag => Err(ProtocolError::Unsupported(tag)),
    }
}

fn representation_tag(value: AssetReadRepresentation) -> u8 {
    match value {
        AssetReadRepresentation::Original => ASSET_REPRESENTATION_ORIGINAL,
        AssetReadRepresentation::Cpu => ASSET_REPRESENTATION_CPU,
        AssetReadRepresentation::Gpu => ASSET_REPRESENTATION_GPU,
    }
}

fn format(tag: u8) -> Result<AssetExportFormat, ProtocolError> {
    match tag {
        ASSET_FORMAT_MESH_V3 => Ok(AssetExportFormat::MeshV3),
        ASSET_FORMAT_TEXTURE_V3 => Ok(AssetExportFormat::TextureV3),
        ASSET_FORMAT_SKELETON_V1 => Ok(AssetExportFormat::SkeletonV1),
        ASSET_FORMAT_POSE_V1 => Ok(AssetExportFormat::PoseV1),
        ASSET_FORMAT_SKIN_V1 => Ok(AssetExportFormat::SkinV1),
        ASSET_FORMAT_SHADER_V3 => Ok(AssetExportFormat::ShaderV3),
        ASSET_FORMAT_ANIMATION_V4 => Ok(AssetExportFormat::AnimationV4),
        ASSET_FORMAT_GEOMETRY_V1 => Ok(AssetExportFormat::GeometryV1),
        ASSET_FORMAT_PARTICLE_CACHE_V1 => Ok(AssetExportFormat::ParticleCacheV1),
        ASSET_FORMAT_EXPRESSION_V1 => Ok(AssetExportFormat::ExpressionV1),
        tag => Err(ProtocolError::Unsupported(tag)),
    }
}

fn format_tag(value: AssetExportFormat) -> u8 {
    match value {
        AssetExportFormat::MeshV3 => ASSET_FORMAT_MESH_V3,
        AssetExportFormat::TextureV3 => ASSET_FORMAT_TEXTURE_V3,
        AssetExportFormat::SkeletonV1 => ASSET_FORMAT_SKELETON_V1,
        AssetExportFormat::PoseV1 => ASSET_FORMAT_POSE_V1,
        AssetExportFormat::SkinV1 => ASSET_FORMAT_SKIN_V1,
        AssetExportFormat::ShaderV3 => ASSET_FORMAT_SHADER_V3,
        AssetExportFormat::AnimationV4 => ASSET_FORMAT_ANIMATION_V4,
        AssetExportFormat::GeometryV1 => ASSET_FORMAT_GEOMETRY_V1,
        AssetExportFormat::ParticleCacheV1 => ASSET_FORMAT_PARTICLE_CACHE_V1,
        AssetExportFormat::ExpressionV1 => ASSET_FORMAT_EXPRESSION_V1,
    }
}

impl Reader<'_> {
    fn asset_capability(&mut self) -> Result<AssetReadCapability, ProtocolError> {
        let capability = AssetReadCapability {
            connection: self.u64()?,
            grant: self.u64()?,
        };
        if capability.connection == 0 || capability.grant == 0 {
            return Err(ProtocolError::InvalidReference);
        }
        Ok(capability)
    }

    fn asset_export_format(&mut self) -> Result<Option<AssetExportFormat>, ProtocolError> {
        if self.boolean()? {
            Ok(Some(format(self.u8()?)?))
        } else {
            Ok(None)
        }
    }

    fn asset_export_source(&mut self) -> Result<AssetSource, ProtocolError> {
        let source = AssetSource {
            kind: AssetTypeId(self.u16()?),
            uri: self.string()?.into(),
            variant: self.u32()?,
        };
        if source.kind.0 == 0 || source.uri.is_empty() {
            return Err(ProtocolError::InvalidReference);
        }
        Ok(source)
    }

    pub(crate) fn asset_export_request(&mut self) -> Result<AssetExportRequest, ProtocolError> {
        match self.u8()? {
            ASSET_EXPORT_FIND => Ok(AssetExportRequest::Find(self.asset_export_source()?)),
            ASSET_EXPORT_READ => {
                let capability = self.asset_capability()?;
                let representation = representation(self.u8()?)?;
                let format = self.asset_export_format()?;
                if matches!(representation, AssetReadRepresentation::Original) != format.is_none() {
                    return Err(ProtocolError::Malformed("asset representation format"));
                }
                Ok(AssetExportRequest::Read {
                    capability,
                    representation,
                    format,
                })
            }
            ASSET_EXPORT_REVOKE => Ok(AssetExportRequest::Revoke(self.asset_capability()?)),
            tag => Err(ProtocolError::Unsupported(tag)),
        }
    }

    pub(crate) fn asset_export_response(&mut self) -> Result<AssetExportResponse, ProtocolError> {
        match self.u8()? {
            ASSET_EXPORT_CAPABILITY => {
                let capability = self.asset_capability()?;
                let source = self.asset_export_source()?;
                let original = self.boolean()?;
                let count = self.count(10)?;
                let cpu = (0..count)
                    .map(|_| format(self.u8()?))
                    .collect::<Result<_, _>>()?;
                let count = self.count(10)?;
                let gpu = (0..count)
                    .map(|_| format(self.u8()?))
                    .collect::<Result<_, _>>()?;
                Ok(AssetExportResponse::Capability {
                    capability,
                    source,
                    access: AssetReadAccess {
                        original,
                        cpu,
                        gpu,
                    },
                })
            }
            ASSET_EXPORT_OPENED => {
                let reference = crate::bulk_read::BulkReadReference {
                    connection: self.u64()?,
                    read: self.u64()?,
                };
                let length = if self.boolean()? {
                    Some(self.u64()?)
                } else {
                    None
                };
                let representation = representation(self.u8()?)?;
                let format = self.asset_export_format()?;
                if reference.connection == 0 || reference.read == 0 {
                    return Err(ProtocolError::InvalidReference);
                }
                if matches!(representation, AssetReadRepresentation::Original) != format.is_none() {
                    return Err(ProtocolError::Malformed("asset representation format"));
                }
                Ok(AssetExportResponse::Read {
                    read: BulkReadDescriptor {
                        reference,
                        length,
                    },
                    representation,
                    format,
                })
            }
            ASSET_EXPORT_REVOKED => Ok(AssetExportResponse::Revoked),
            tag => Err(ProtocolError::Unsupported(tag)),
        }
    }
}

impl Writer {
    fn asset_capability(&mut self, capability: AssetReadCapability) -> Result<(), ProtocolError> {
        if capability.connection == 0 || capability.grant == 0 {
            return Err(ProtocolError::InvalidReference);
        }
        self.u64(capability.connection)?;
        self.u64(capability.grant)
    }

    fn asset_export_format(
        &mut self,
        value: Option<AssetExportFormat>,
    ) -> Result<(), ProtocolError> {
        self.u8(u8::from(value.is_some()))?;
        if let Some(value) = value {
            self.u8(format_tag(value))?;
        }
        Ok(())
    }

    fn asset_export_source(&mut self, source: &AssetSource) -> Result<(), ProtocolError> {
        if source.kind.0 == 0 || source.uri.is_empty() {
            return Err(ProtocolError::InvalidReference);
        }
        self.u16(source.kind.0)?;
        self.string(&source.uri)?;
        self.u32(source.variant)
    }

    pub(crate) fn asset_export_request(
        &mut self,
        request: &AssetExportRequest,
    ) -> Result<(), ProtocolError> {
        match request {
            AssetExportRequest::Find(source) => {
                self.u8(ASSET_EXPORT_FIND)?;
                self.asset_export_source(source)
            }
            AssetExportRequest::Read {
                capability,
                representation,
                format,
            } => {
                if matches!(representation, AssetReadRepresentation::Original) != format.is_none() {
                    return Err(ProtocolError::Malformed("asset representation format"));
                }
                self.u8(ASSET_EXPORT_READ)?;
                self.asset_capability(*capability)?;
                self.u8(representation_tag(*representation))?;
                self.asset_export_format(*format)
            }
            AssetExportRequest::Revoke(capability) => {
                self.u8(ASSET_EXPORT_REVOKE)?;
                self.asset_capability(*capability)
            }
        }
    }

    pub(crate) fn asset_export_response(
        &mut self,
        response: &AssetExportResponse,
    ) -> Result<(), ProtocolError> {
        match response {
            AssetExportResponse::Capability {
                capability,
                source,
                access,
            } => {
                self.u8(ASSET_EXPORT_CAPABILITY)?;
                self.asset_capability(*capability)?;
                self.asset_export_source(source)?;
                self.u8(u8::from(access.original))?;
                self.count(access.cpu.len(), 10)?;
                for value in &access.cpu {
                    self.u8(format_tag(*value))?;
                }
                self.count(access.gpu.len(), 10)?;
                for value in &access.gpu {
                    self.u8(format_tag(*value))?;
                }
                Ok(())
            }
            AssetExportResponse::Read {
                read,
                representation,
                format,
            } => {
                if read.reference.connection == 0 || read.reference.read == 0 {
                    return Err(ProtocolError::InvalidReference);
                }
                if matches!(representation, AssetReadRepresentation::Original) != format.is_none() {
                    return Err(ProtocolError::Malformed("asset representation format"));
                }
                self.u8(ASSET_EXPORT_OPENED)?;
                self.u64(read.reference.connection)?;
                self.u64(read.reference.read)?;
                self.u8(u8::from(read.length.is_some()))?;
                if let Some(length) = read.length {
                    self.u64(length)?;
                }
                self.u8(representation_tag(*representation))?;
                self.asset_export_format(*format)
            }
            AssetExportResponse::Revoked => self.u8(ASSET_EXPORT_REVOKED),
        }
    }
}

#[cfg(test)]
#[path = "asset_export_tests.rs"]
mod tests;
