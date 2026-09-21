use super::ParticleRuntimeState;
use crate::{
    ErrorReason,
    components::schema::ComponentLifecycle,
    services::asset_management::service::{AssetDemandSelection, validate_source},
};
use ipp_schema_derive::SchemaComponent;
use std::collections::BTreeSet;

/// ParticleEmitter authored settings.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, SchemaComponent)]
pub struct ParticleEmitter {
    /// Emit new particles; disabling drains existing particles.
    pub enabled: bool,
    /// Deterministic birth seed.
    pub seed: u32,
    /// Change this value to restart the live simulation.
    pub restart: u32,
    /// Maximum simultaneously living particles; excess births are discarded.
    pub capacity: u32,
    /// Births per second.
    pub rate: f32,
    /// Emission duration in seconds; zero is continuous.
    pub duration: f32,
    /// Delay from restart until emission starts, in seconds.
    pub delay: f32,
    /// Particles born at restart.
    pub burst: u32,
    /// Zero simulates locally, one in World coordinates.
    pub space: u32,
    /// Point=0, box=1, sphere volume=2, static mesh faces=3.
    pub shape: u32,
    /// Immutable emission mesh for shape 3.
    pub source: String,
    /// Emission mesh variant.
    pub variant: u32,
    /// Box half extent or sphere radius along X.
    pub extent_x: f32,
    /// Box half extent along Y.
    pub extent_y: f32,
    /// Box half extent along Z.
    pub extent_z: f32,
    /// Maximum lifetime in seconds.
    pub lifetime: f32,
    /// Fractional lifetime randomness, zero through one.
    pub lifetime_random: f32,
    /// Initial speed along local +Y or face normal.
    pub speed: f32,
    /// Fractional speed randomness.
    pub speed_random: f32,
    /// Cone half angle in radians, zero through pi.
    pub spread: f32,
    /// Initial uniform size.
    pub size: f32,
    /// Fractional size randomness.
    pub size_random: f32,
    /// Initial rotation randomness in radians.
    pub rotation_random: f32,
    /// Angular velocity around local Z in radians per second.
    pub spin: f32,
    /// Acceleration in simulation coordinates.
    pub acceleration_x: f32,
    /// Acceleration in simulation coordinates.
    pub acceleration_y: f32,
    /// Acceleration in simulation coordinates.
    pub acceleration_z: f32,
    /// Linear velocity drag per second.
    pub drag: f32,
    /// Private evaluated state, reconstructed after replacement or load.
    #[schema(ignore)]
    pub runtime: ParticleRuntimeState,
}

impl Default for ParticleEmitter {
    fn default() -> Self {
        Self {
            enabled: true,
            seed: 1,
            restart: 0,
            capacity: 10000,
            rate: 10.0,
            duration: 0.0,
            delay: 0.0,
            burst: 0,
            space: 0,
            shape: 0,
            source: String::new(),
            variant: 0,
            extent_x: 1.0,
            extent_y: 1.0,
            extent_z: 1.0,
            lifetime: 2.0,
            lifetime_random: 0.0,
            speed: 1.0,
            speed_random: 0.0,
            spread: 0.0,
            size: 0.1,
            size_random: 0.0,
            rotation_random: 0.0,
            spin: 0.0,
            acceleration_x: 0.0,
            acceleration_y: 0.0,
            acceleration_z: 0.0,
            drag: 0.0,
            runtime: Default::default(),
        }
    }
}

impl ComponentLifecycle for ParticleEmitter {
    fn preserve_runtime(&mut self, previous: &mut Self) {
        if self.seed == previous.seed
            && self.space == previous.space
            && self.restart == previous.restart
        {
            self.runtime = std::mem::take(&mut previous.runtime);
        }
    }

    fn asset_references() -> &'static [crate::components::schema::ComponentAssetReference] {
        &[crate::components::schema::ComponentAssetReference {
            kind: 16,
            source_offset: std::mem::offset_of!(Self, source) as u32,
            variant_offset: std::mem::offset_of!(Self, variant) as u32,
        }]
    }

    fn resource_demand(&self, demand: &mut BTreeSet<AssetDemandSelection>) {
        if !self.source.is_empty() && self.shape == 3 {
            demand.insert(AssetDemandSelection::new(
                crate::services::asset_management::AssetTypeId(16),
                &self.source,
                self.variant,
            ));
        }
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        validate_source(&self.source)?;
        if [
            self.rate,
            self.duration,
            self.extent_x,
            self.extent_y,
            self.extent_z,
            self.lifetime,
            self.lifetime_random,
            self.speed,
            self.speed_random,
            self.spread,
            self.size,
            self.size_random,
            self.rotation_random,
            self.spin,
            self.acceleration_x,
            self.acceleration_y,
            self.acceleration_z,
            self.drag,
        ]
        .iter()
        .any(|v| !v.is_finite())
        {
            return Err(ErrorReason::InvalidValue);
        }
        if self.capacity == 0
            || self.space > 1
            || self.shape > 3
            || self.lifetime <= 0.0
            || self.size <= 0.0
            || self.rate < 0.0
            || self.drag < 0.0
            || self.duration < 0.0
            || self.delay < 0.0
            || !self.delay.is_finite()
            || self.speed < 0.0
            || [self.extent_x, self.extent_y, self.extent_z]
                .iter()
                .any(|v| *v < 0.0)
            || [self.lifetime_random, self.speed_random, self.size_random]
                .iter()
                .any(|v| !(0.0..=1.0).contains(v))
            || !(0.0..=std::f32::consts::PI).contains(&self.spread)
            || self.rotation_random < 0.0
        {
            return Err(ErrorReason::InvalidValue);
        }
        Ok(())
    }
}

/// ParticlePlayback authored settings.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, SchemaComponent)]
pub struct ParticlePlayback {
    /// Immutable IPPC particle cache.
    pub source: String,
    /// Cache variant.
    pub variant: u32,
    /// Animatable absolute cache sample time in seconds.
    pub time: f32,
    /// Private evaluated state, reconstructed after replacement or load.
    #[schema(ignore)]
    pub runtime: ParticleRuntimeState,
}

impl Default for ParticlePlayback {
    fn default() -> Self {
        Self {
            source: String::new(),
            variant: 0,
            time: 0.0,
            runtime: Default::default(),
        }
    }
}

impl ComponentLifecycle for ParticlePlayback {
    fn supports_numeric_property(offset: u32) -> bool {
        offset == std::mem::offset_of!(Self, time) as u32
    }

    fn validate_numeric_properties(
        &self,
        fields: &[(u32, crate::components::schema::FieldValue)],
    ) -> Result<(), ErrorReason> {
        self.validate()?;
        for (offset, field) in fields {
            if !Self::supports_numeric_property(*offset) {
                return Err(ErrorReason::InvalidField);
            }
            match field {
                crate::components::schema::FieldValue::F32(time) if time.is_finite() => {}
                _ => return Err(ErrorReason::InvalidValue),
            }
        }
        Ok(())
    }

    fn preserve_runtime(&mut self, previous: &mut Self) {
        if self.source == previous.source && self.variant == previous.variant {
            self.runtime = std::mem::take(&mut previous.runtime);
        }
    }

    fn asset_references() -> &'static [crate::components::schema::ComponentAssetReference] {
        &[crate::components::schema::ComponentAssetReference {
            kind: 15,
            source_offset: std::mem::offset_of!(Self, source) as u32,
            variant_offset: std::mem::offset_of!(Self, variant) as u32,
        }]
    }

    fn resource_demand(&self, demand: &mut BTreeSet<AssetDemandSelection>) {
        if !self.source.is_empty() {
            demand.insert(AssetDemandSelection::new(
                crate::services::asset_management::AssetTypeId(15),
                &self.source,
                self.variant,
            ));
        }
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        validate_source(&self.source)?;
        if [self.time].iter().any(|v| !v.is_finite()) {
            return Err(ErrorReason::InvalidValue);
        }
        Ok(())
    }
}

/// ParticleSprite authored settings.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, SchemaComponent)]
pub struct ParticleSprite {
    /// Optional RGB sprite texture; radial opacity is procedural.
    pub source: String,
    /// Texture variant.
    pub variant: u32,
    /// Linear red tint.
    pub r: f32,
    /// Linear green tint.
    pub g: f32,
    /// Linear blue tint.
    pub b: f32,
    /// Opacity at birth.
    pub opacity: f32,
    /// Opacity at death.
    pub end_opacity: f32,
    /// Size multiplier at death.
    pub end_size: f32,
    /// Straight alpha=0, additive=1.
    pub blend: u32,
    /// Camera facing=0, velocity aligned=1.
    pub alignment: u32,
}

impl Default for ParticleSprite {
    fn default() -> Self {
        Self {
            source: String::new(),
            variant: 0,
            r: 1.0,
            g: 1.0,
            b: 1.0,
            opacity: 1.0,
            end_opacity: 0.0,
            end_size: 1.0,
            blend: 0,
            alignment: 0,
        }
    }
}

impl ComponentLifecycle for ParticleSprite {
    fn required_components() -> &'static [u16] {
        &[crate::ComponentValue::BOUNDING_GEOMETRY]
    }

    fn asset_references() -> &'static [crate::components::schema::ComponentAssetReference] {
        &[crate::components::schema::ComponentAssetReference {
            kind: 2,
            source_offset: std::mem::offset_of!(Self, source) as u32,
            variant_offset: std::mem::offset_of!(Self, variant) as u32,
        }]
    }

    fn resource_demand(&self, demand: &mut BTreeSet<AssetDemandSelection>) {
        if !self.source.is_empty() {
            demand.insert(AssetDemandSelection::new(
                crate::services::asset_management::AssetTypeId(2),
                &self.source,
                self.variant,
            ));
        }
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        validate_source(&self.source)?;
        if [
            self.r,
            self.g,
            self.b,
            self.opacity,
            self.end_opacity,
            self.end_size,
        ]
        .iter()
        .any(|v| !v.is_finite())
        {
            return Err(ErrorReason::InvalidValue);
        }
        if self.blend > 1
            || self.alignment > 1
            || self.end_size < 0.0
            || [self.r, self.g, self.b, self.opacity, self.end_opacity]
                .iter()
                .any(|v| !(0.0..=1.0).contains(v))
        {
            return Err(ErrorReason::InvalidValue);
        }
        Ok(())
    }
}

/// ParticleMesh authored settings.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, SchemaComponent, Default)]
pub struct ParticleMesh {
    /// Immutable particle mesh; materials are sibling components.
    pub source: String,
    /// Mesh variant.
    pub variant: u32,
}

impl ComponentLifecycle for ParticleMesh {
    fn required_components() -> &'static [u16] {
        &[crate::ComponentValue::BOUNDING_GEOMETRY]
    }

    fn asset_references() -> &'static [crate::components::schema::ComponentAssetReference] {
        &[crate::components::schema::ComponentAssetReference {
            kind: 1,
            source_offset: std::mem::offset_of!(Self, source) as u32,
            variant_offset: std::mem::offset_of!(Self, variant) as u32,
        }]
    }

    fn resource_demand(&self, demand: &mut BTreeSet<AssetDemandSelection>) {
        if !self.source.is_empty() {
            demand.insert(AssetDemandSelection::new(
                crate::services::asset_management::AssetTypeId(1),
                &self.source,
                self.variant,
            ));
        }
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        validate_source(&self.source)?;
        Ok(())
    }
}
