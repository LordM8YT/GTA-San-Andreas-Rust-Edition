//! Night light decoration from original 2DFX lights: camera-facing coronas
//! and soft light pools on the ground below each lamp. They travel through
//! streaming as ordinary batches; the runtime draws them additively.
use super::{place, Batch, Placement, Vertex};
use sa_assets::Geometry;
use std::collections::HashMap;

pub const CORONA_KEY: &str = "runtime:corona";
pub const POOL_KEY: &str = "runtime:lightpool";
const CORNERS: [[f32; 2]; 6] = [
    [-1.0, -1.0],
    [1.0, -1.0],
    [1.0, 1.0],
    [-1.0, -1.0],
    [1.0, 1.0],
    [-1.0, 1.0],
];

fn batch<'a>(batches: &'a mut HashMap<String, Batch>, key: &str) -> &'a mut Batch {
    batches.entry(key.into()).or_insert_with(|| Batch {
        key: key.into(),
        vertices: Vec::new(),
        alpha: true,
        animated: false,
        uv_animation: None,
        map_door: None,
        mlo_entity: None,
    })
}

/// Coronas store the lamp centre in every vertex and the corner times the
/// corona size in the UV; the vertex shader expands them towards the camera.
/// Colour alpha is 1 for night-only lights and 0.5 for always-lit ones.
pub(super) fn add(
    batches: &mut HashMap<String, Batch>,
    geometries: &[Geometry],
    r: &Placement,
    origin: [f32; 2],
) {
    for light in geometries.iter().flat_map(|g| &g.lights) {
        let centre = place(light.position, r, origin);
        let mut color = light.color.map(|c| f32::from(c) / 255.0);
        color[3] = if light.always { 0.5 } else { 1.0 };
        if light.corona {
            let size = light.corona_size.clamp(0.2, 6.0);
            batch(batches, CORONA_KEY)
                .vertices
                .extend(CORNERS.map(|[x, y]| Vertex {
                    position: centre,
                    uv: [x * size, y * size],
                    color,
                }));
        }
        let radius = if light.shadow_size > 0.0 {
            light.shadow_size
        } else {
            light.range * 0.35
        }
        .min(12.0);
        if radius > 0.5 && light.shadow_distance > 0.0 {
            let ground = centre[1] - light.shadow_distance + 0.05;
            batch(batches, POOL_KEY)
                .vertices
                .extend(CORNERS.map(|[x, z]| Vertex {
                    position: [centre[0] + x * radius, ground, centre[2] + z * radius],
                    uv: [x, z],
                    color,
                }));
        }
    }
}

/// The glow of a Build Mode [`PointLight`], in the object's own glTF space
/// (the node's transform places it): a corona at the lamp and a pool on the
/// ground at the object's base, in the corona/pool encoding above.
///
/// [`PointLight`]: crate::world_layout::PointLight
pub fn point_light_model(light: &crate::world_layout::PointLight) -> crate::gltf_model::Model {
    use crate::world_layout::PointLight;
    let centre = crate::world_layout::blender_to_gltf(light.offset).to_array();
    let color = [light.color[0], light.color[1], light.color[2], 1.0];
    let size = light.size.clamp(0.1, PointLight::MAX_SIZE);
    let mut batches = HashMap::new();
    batch(&mut batches, CORONA_KEY)
        .vertices
        .extend(CORNERS.map(|[x, y]| Vertex {
            position: centre,
            uv: [x * size, y * size],
            color,
        }));
    let radius = light.radius.min(PointLight::MAX_RADIUS);
    if radius > 0.1 {
        batch(&mut batches, POOL_KEY)
            .vertices
            .extend(CORNERS.map(|[x, z]| Vertex {
                position: [centre[0] + x * radius, 0.05, centre[2] + z * radius],
                uv: [x, z],
                color,
            }));
    }
    let mut drawn: Vec<Batch> = batches.into_values().collect();
    drawn.sort_by(|a, b| a.key.cmp(&b.key));
    crate::gltf_model::Model {
        textures: drawn
            .iter()
            .map(|b| (b.key.clone(), crate::builtin_assets::white()))
            .collect(),
        drawn,
        collision: Vec::new(),
        triangles: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sa_assets::light::Light;
    #[test]
    fn build_mode_lights_glow_at_their_lamp_and_pool_on_the_ground() {
        let mut light = crate::world_layout::PointLight::lamp();
        light.offset = [0.0, 1.0, 4.0];
        let model = point_light_model(&light);
        assert!(model.collision.is_empty(), "lights are never solid");
        let corona = model.drawn.iter().find(|b| b.key == CORONA_KEY).unwrap();
        // Blender (0, 1, 4) is glTF (0, 4, -1).
        assert_eq!(corona.vertices[0].position, [0.0, 4.0, -1.0]);
        assert_eq!(corona.vertices[2].uv, [1.0, 1.0]);
        let pool = model.drawn.iter().find(|b| b.key == POOL_KEY).unwrap();
        assert!(pool.vertices.iter().all(|v| v.position[1] == 0.05));
        assert!(model.textures.contains_key(CORONA_KEY));
        assert!(model.textures.contains_key(POOL_KEY));
        light.radius = 0.0;
        assert_eq!(point_light_model(&light).drawn.len(), 1, "no pool");
    }

    #[test]
    fn lamp_gets_a_corona_and_a_ground_pool() {
        let geometry = Geometry {
            positions: Vec::new(),
            normals: Vec::new(),
            uvs: Vec::new(),
            colors: Vec::new(),
            triangles: Vec::new(),
            materials: Vec::new(),
            road_signs: Vec::new(),
            lights: vec![Light {
                position: [0.0, 0.0, 8.0],
                color: [255, 220, 150, 255],
                range: 15.0,
                corona_size: 1.5,
                shadow_size: 5.0,
                shadow_distance: 8.0,
                corona: true,
                always: false,
            }],
        };
        let r = Placement {
            id: 0,
            scale: 1.0,
            solid: true,
            interior: 0,
            lod: -1,
            is_lod: false,
            uid: 0,
            parent: 0,
            pos: [10.0, 20.0, 5.0],
            quat: [0.0, 0.0, 0.0, 1.0],
            mlo_entity: None,
        };
        let mut batches = HashMap::new();
        add(&mut batches, &[geometry], &r, [0.0, 0.0]);
        let corona = &batches[CORONA_KEY].vertices;
        assert_eq!(corona.len(), 6);
        assert_eq!(corona[0].position, [10.0, 13.0, -20.0]);
        assert_eq!(corona[2].uv, [1.5, 1.5]);
        let pool = &batches[POOL_KEY].vertices;
        assert!(pool.iter().all(|v| (v.position[1] - 5.05).abs() < 1e-4));
        assert_eq!(pool[1].position[0], 15.0);
    }
}
