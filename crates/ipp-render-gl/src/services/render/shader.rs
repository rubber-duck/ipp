//! Typed feature validation and snippet selection, separate from text evaluation.

use crate::{RenderError, services::render::template};

// Recipe fields occupy bits 0..=11, including the shadow-pass bit 9.
pub(super) const PROGRAM_RECIPE_COUNT: usize = 1 << 12;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RenderShaderConfig {
    #[cfg(feature = "mesh-poses")]
    posed: bool,
    #[cfg(feature = "particles")]
    instanced: bool,
    #[cfg(feature = "particles")]
    sprite: bool,
    lit: bool,
    normals: bool,
    #[cfg(feature = "shadows")]
    shadow: bool,
    #[cfg(feature = "skeletal-animation")]
    skinned: bool,
    debug_geometry: bool,
    solid: bool,
    textured: bool,
    weighted: bool,
}

// Optional fields disappear independently in lean feature combinations.
#[allow(clippy::needless_update)]
impl RenderShaderConfig {
    pub(super) fn recipe_bits(self) -> u32 {
        let mut bits = u32::from(self.lit);
        bits |= u32::from(self.normals) << 1;
        bits |= u32::from(self.debug_geometry) << 2;
        bits |= u32::from(self.solid) << 3;
        bits |= u32::from(self.textured) << 4;
        bits |= u32::from(self.weighted) << 5;
        #[cfg(feature = "mesh-poses")]
        {
            bits |= u32::from(self.posed) << 6;
        }
        #[cfg(feature = "skeletal-animation")]
        {
            bits |= u32::from(self.skinned) << 7;
        }
        #[cfg(feature = "shadows")]
        {
            bits |= u32::from(self.shadow) << 8;
        }
        #[cfg(feature = "particles")]
        {
            bits |= u32::from(self.instanced) << 10;
            bits |= u32::from(self.sprite) << 11;
        }
        bits
    }

    pub(super) fn from_recipe_bits(bits: u32) -> Self {
        Self {
            #[cfg(feature = "particles")]
            instanced: bits & (1 << 10) != 0,
            #[cfg(feature = "particles")]
            sprite: bits & (1 << 11) != 0,
            lit: bits & (1 << 0) != 0,
            normals: bits & (1 << 1) != 0,
            debug_geometry: bits & (1 << 2) != 0,
            solid: bits & (1 << 3) != 0,
            textured: bits & (1 << 4) != 0,
            weighted: bits & (1 << 5) != 0,
            #[cfg(feature = "mesh-poses")]
            posed: bits & (1 << 6) != 0,
            #[cfg(feature = "skeletal-animation")]
            skinned: bits & (1 << 7) != 0,
            #[cfg(feature = "shadows")]
            shadow: bits & (1 << 8) != 0,
        }
    }

    #[cfg(feature = "particles")]
    pub(crate) fn with_particles(mut self, instanced: bool, sprite: bool) -> Self {
        self.instanced = instanced;
        self.sprite = sprite;
        self
    }

    pub(crate) fn new(textured: bool, weighted: bool) -> Self {
        Self {
            textured,
            weighted: textured && weighted,
            ..Self::default()
        }
    }

    pub(crate) fn with_debug_geometry(self, enabled: bool) -> Self {
        if enabled {
            Self {
                debug_geometry: true,
                ..Self::default()
            }
        } else {
            self
        }
    }

    pub(crate) fn with_solid_fallback(mut self, solid: bool) -> Self {
        self.solid = solid;
        self
    }

    pub(crate) fn with_lighting(self, shadow: bool, normals: bool) -> Self {
        let _ = shadow;
        Self {
            lit: true,
            normals,
            #[cfg(feature = "shadows")]
            shadow,
            ..self
        }
    }

    #[cfg(feature = "skeletal-animation")]
    pub(crate) fn with_skinning(mut self, skinned: bool) -> Self {
        self.skinned = skinned;
        self
    }

    #[cfg(feature = "mesh-poses")]
    pub(crate) fn with_mesh_pose(mut self, posed: bool) -> Self {
        self.posed = posed;
        self
    }

    pub(crate) fn sources(self) -> Result<(String, String), RenderError> {
        if self.debug_geometry {
            return Ok((
                crate::services::render::embedded_shader!("shaders/debug-vertex.glsl").into(),
                crate::services::render::embedded_shader!("shaders/debug-fragment.glsl").into(),
            ));
        }

        #[allow(unused_mut)]
        let mut values = vec![
            (
                "texture_declarations",
                crate::services::render::embedded_shader!("shaders/texture-fragment.glsl"),
            ),
            ("texture_body", "sampled = texture(u_texture, v_uv).rgb;"),
            (
                "weight_declarations",
                crate::services::render::embedded_shader!("shaders/weight-fragment.glsl"),
            ),
            (
                "weight_body",
                "sampled = mix(vec3(1.0), sampled, v_weight);",
            ),
        ];
        // Feature-specific snippets are absent from lean renderer artifacts.
        #[cfg(feature = "shadows")]
        values.extend([
            (
                "shadow_declarations",
                crate::services::render::embedded_shader!("shaders/shadow-sampling.glsl"),
            ),
            (
                "shadow_body",
                "if (u_surface.z > 0.5) shadow = visibility_from_shadow(i, nl, n);",
            ),
        ]);
        #[cfg(feature = "shadows")]
        let shadow = self.shadow;
        #[cfg(not(feature = "shadows"))]
        let shadow = false;
        let conditions = [
            ("solid", self.solid),
            ("vertex_color", !self.solid),
            ("texture", self.textured),
            ("weight", self.weighted),
            ("shadow", shadow),
            ("normals", self.normals),
            ("flat_normals", !self.normals),
        ];
        if self.lit {
            return Ok((
                self.vertex_source(crate::services::render::embedded_shader!(
                    "shaders/lit.vert"
                ))?,
                template::evaluate(
                    crate::services::render::embedded_shader!("shaders/lit.frag"),
                    &values,
                    &conditions,
                )
                .map_err(|error| RenderError::RenderDevice(error.0))?,
            ));
        }

        let expand = |source, values: &[(&str, &str)]| {
            template::evaluate(source, values, &conditions)
                .map_err(|error| RenderError::RenderDevice(error.0))
        };
        #[allow(unused_mut)]
        let mut fragment = expand(
            crate::services::render::embedded_shader!("unlit.frag"),
            &values,
        )?;
        #[cfg(feature = "particles")]
        if self.sprite {
            fragment = fragment.replace("out vec4 out_color;", "in vec2 v_particle_uv;\nin float v_particle_opacity;\nout vec4 out_color;")
                .replace("vec4(linear_rgb, 1.0)", "vec4(linear_rgb, v_particle_opacity * (1.0 - smoothstep(0.35, 0.5, length(v_particle_uv - vec2(0.5)))))");
        }
        Ok((
            self.vertex_source(crate::services::render::embedded_shader!("unlit.vert"))?,
            fragment,
        ))
    }

    #[cfg(feature = "shadows")]
    pub(crate) fn shadow_sources(self) -> Result<(String, String), RenderError> {
        Ok((
            self.vertex_source(crate::services::render::embedded_shader!(
                "shaders/shadow.vert"
            ))?,
            crate::services::render::embedded_shader!("shaders/shadow.frag").into(),
        ))
    }

    pub(crate) fn custom_vertex_source(self, source: &str) -> Result<String, RenderError> {
        self.vertex_source(source)
    }

    fn vertex_source(self, source: &str) -> Result<String, RenderError> {
        let conditions = [("texture", self.textured), ("weight", self.weighted)];
        #[cfg(feature = "skeletal-animation")]
        let skinned = self.skinned;
        #[cfg(not(feature = "skeletal-animation"))]
        let skinned = false;
        #[cfg(feature = "mesh-poses")]
        let posed = self.posed;
        #[cfg(not(feature = "mesh-poses"))]
        let posed = false;
        let conditions = [
            ("pose", posed),
            ("normals", self.normals),
            conditions[0],
            conditions[1],
            ("skin", skinned),
            ("rigid", !skinned),
        ];
        let vertex = [
            #[cfg(feature = "mesh-poses")]
            (
                "pose_declarations",
                crate::services::render::embedded_shader!("shaders/pose-vertex.glsl"),
            ),
            #[cfg(feature = "mesh-poses")]
            (
                "pose_body",
                "local_position = mix(a_position, a_pose_position, u_pose_weight);",
            ),
            #[cfg(feature = "mesh-poses")]
            (
                "pose_normal_declaration",
                "layout(location = 8) in vec3 a_pose_normal;",
            ),
            #[cfg(feature = "mesh-poses")]
            (
                "pose_normal_body",
                crate::services::render::embedded_shader!("shaders/pose-normal.glsl"),
            ),
            #[cfg(feature = "skeletal-animation")]
            (
                "skin_declarations",
                crate::services::render::embedded_shader!("shaders/skin-vertex.glsl"),
            ),
            (
                "texture_declarations",
                crate::services::render::embedded_shader!("shaders/texture-vertex.glsl"),
            ),
            ("texture_body", "v_uv = a_uv;"),
            (
                "weight_declarations",
                crate::services::render::embedded_shader!("shaders/weight-vertex.glsl"),
            ),
            ("weight_body", "v_weight = a_weight;"),
        ];
        let result = template::evaluate(source, &vertex, &conditions)
            .map_err(|error| RenderError::RenderDevice(error.0))?;
        #[cfg(feature = "particles")]
        if self.instanced {
            let declarations = "layout(location = 9) in mat4 a_instance_model;\nlayout(location = 13) in vec4 a_instance_data;\n#define IPP_INSTANCED 1\nmat4 ippInstanceModel() { return a_instance_model; }\nmat3 ippInstanceNormal() { return transpose(inverse(mat3(a_instance_model))); }\n";
            let result = result
                .replace(
                    "uniform mat4 u_mvp;",
                    &format!("{declarations}uniform mat4 u_mvp;"),
                )
                .replace(
                    "u_mvp * vec4(local_position, 1.0)",
                    "u_mvp * a_instance_model * vec4(local_position, 1.0)",
                )
                .replace(
                    "u_mvp * skinned_position",
                    "u_mvp * a_instance_model * skinned_position",
                )
                .replace("u_mvp * position", "u_mvp * a_instance_model * position")
                .replace(
                    "u_model * position",
                    "u_model * a_instance_model * position",
                )
                .replace(
                    "(u_normal * vec4(authored, 0.0)).xyz",
                    "mat3(u_normal) * ippInstanceNormal() * authored",
                );
            if self.sprite {
                return Ok(result.replace("out vec3 v_color;", "out vec3 v_color;\nout vec2 v_particle_uv;\nout float v_particle_opacity;")
                    .replace("v_color = a_color;", "v_color = a_color; v_particle_uv = a_position.xy + vec2(0.5); v_particle_opacity = a_instance_data.x;"));
            }
            return Ok(result);
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::RenderShaderConfig;

    #[test]
    fn lighting_composes_texture_weights_normals_and_deformation() {
        for normals in [false, true] {
            let config = RenderShaderConfig::new(true, true).with_lighting(true, normals);
            #[cfg(feature = "skeletal-animation")]
            let config = config.with_skinning(true);
            #[cfg(feature = "mesh-poses")]
            let config = config.with_mesh_pose(true);
            let (vertex, fragment) = config.sources().unwrap();
            assert!(vertex.contains("v_uv = a_uv"));
            assert!(vertex.contains("v_weight = a_weight"));
            assert!(fragment.contains("v_color * u_material * sampled"));
            assert!(fragment.contains("texture(u_texture, v_uv)"));
            assert!(fragment.contains("mix(vec3(1.0), sampled, v_weight)"));
            assert_eq!(vertex.contains("in vec3 a_normal"), normals);
            #[cfg(feature = "skeletal-animation")]
            assert!(vertex.contains("skinned_position(local_position)"));
            #[cfg(feature = "mesh-poses")]
            assert!(vertex.contains("mix(a_position, a_pose_position, u_pose_weight)"));
        }
    }

    #[test]
    fn configuration_normalizes_inactive_weights_and_composes_snippets() {
        assert_eq!(
            RenderShaderConfig::new(false, true),
            RenderShaderConfig::default()
        );
        let (vertex, fragment) = RenderShaderConfig::new(true, true).sources().unwrap();
        assert_eq!(vertex.matches("in vec2 a_uv").count(), 1);
        assert_eq!(fragment.matches("sampler2D u_texture").count(), 1);
        assert!(fragment.contains("mix(vec3(1.0), sampled, v_weight)"));
        let (_, fragment) = RenderShaderConfig::default().sources().unwrap();
        assert!(!fragment.contains("sampler2D"));
        assert!(!fragment.contains("v_weight"));
    }
}

#[cfg(test)]
mod embedded_tests {
    #[test]
    fn embedded_shaders_drop_comments_and_blank_lines() {
        let raw = include_str!("shaders/lit.frag");
        let embedded = embedded_shader!("shaders/lit.frag");
        assert!(embedded.len() < raw.len());
        assert!(embedded.starts_with("#version 300 es\n"));
        assert!(!embedded.contains("//"));
        assert!(
            embedded
                .lines()
                .all(|line| line == line.trim() && !line.is_empty())
        );
    }

    #[test]
    fn embedded_shaders_keep_composition_placeholders() {
        let embedded = embedded_shader!("shaders/custom.vert");
        assert!(embedded.starts_with("// CUSTOM_DECLARATIONS\n"));
    }
}

#[cfg(test)]
mod debug_tests {
    use super::RenderShaderConfig;

    #[test]
    fn debug_variant_has_only_position_and_uniform_color() {
        let config = RenderShaderConfig::default().with_debug_geometry(true);
        let (vertex, fragment) = config.sources().unwrap();
        assert!(vertex.contains("a_position"));
        assert!(!vertex.contains("a_color"));
        assert!(fragment.contains("clamp(u_material, 0.0, 1.0)"));
        assert!(!fragment.contains("v_color"));
        assert!(!fragment.contains("texture"));
        assert_ne!(config, RenderShaderConfig::default());
        assert_eq!(
            RenderShaderConfig::new(true, true).with_debug_geometry(true),
            config
        );
    }
}
