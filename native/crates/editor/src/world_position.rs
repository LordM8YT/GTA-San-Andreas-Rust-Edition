//! A Build Mode position as server scripts write it: San Andreas world
//! coordinates in a `vector3(...)`/`vector4(...)` to paste into a resource's
//! `Config.Locations`, like the coordinates FiveM scripts use.
//!
//! A pack's origin (Blender 0, 0, 0) sits at [`ANCHOR`] in the world, so
//! world = (anchor x + east, anchor y + north, up).
use sa_scene::content_pack::ANCHOR;
use sa_scene::world_layout::Transform;

/// World coordinates of a point in pack (Blender) axes.
pub fn world(position: [f32; 3]) -> [f32; 3] {
    [
        ANCHOR[0] + position[0],
        ANCHOR[1] + position[1],
        position[2],
    ]
}

/// Heading in degrees (0-360, 0 = north, counter-clockwise like the game)
/// from a Blender Z rotation.
pub fn heading(rotation_z: f32) -> f32 {
    let h = rotation_z.rem_euclid(360.0);
    // -0.01 rounds to 360.0 at one decimal; show 0 instead.
    if h >= 359.95 {
        0.0
    } else {
        h
    }
}

/// `vector4(x, y, z, heading)` for an object's transform.
pub fn vector4(transform: &Transform) -> String {
    let [x, y, z] = world(transform.position);
    format!(
        "vector4({x:.2}, {y:.2}, {z:.2}, {:.1})",
        heading(transform.rotation[2])
    )
}

/// `vector3(x, y, z)` for a point in pack axes.
pub fn vector3(position: [f32; 3]) -> String {
    let [x, y, z] = world(position);
    format!("vector3({x:.2}, {y:.2}, {z:.2})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_points_become_world_vectors() {
        let mut t = Transform::at([10.0, -20.5, 0.2]);
        t.rotation[2] = -90.0;
        let x = ANCHOR[0] + 10.0;
        let y = ANCHOR[1] - 20.5;
        assert_eq!(vector4(&t), format!("vector4({x:.2}, {y:.2}, 0.20, 270.0)"));
        assert_eq!(
            vector3([0.0, 0.0, 1.0]),
            format!("vector3({:.2}, {:.2}, 1.00)", ANCHOR[0], ANCHOR[1])
        );
        assert_eq!(heading(720.0), 0.0);
        assert_eq!(heading(-0.01), 0.0);
        assert_eq!(heading(45.0), 45.0);
    }
}
