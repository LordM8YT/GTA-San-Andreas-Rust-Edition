//! Simple physics for parts that are not anchored: they fall straight down
//! when nothing holds them up and land on what is below (the world's solid
//! objects and other parts), stacking on each other. A fall is a move like
//! a script's ([`crate::world_logic::Motion`] with gravity), so servers and
//! games play it the same way and a snapshot carries it.
//!
//! Not yet: tumbling, sliding, being pushed by players or cars.
use crate::collision::CollisionWorld;
use crate::content_pack::{place_layout, AssetResolver};
use crate::world_layout::{Layout, NodeId};
use glam::{Mat3, Vec3};

/// Metres per second squared.
pub const GRAVITY: f32 = 9.81;
/// Falls shorter than this are not worth moving (metres).
const SETTLED: f32 = 0.02;
/// What a body may already overlap and still stand on (metres).
const SLACK: f32 = 0.05;

/// The fixed world unanchored parts land on, and the parts themselves.
pub struct Physics {
    /// Solid objects that never move, in pack (glTF) space.
    statics: CollisionWorld,
    /// Each unanchored part's box around its place (Blender axes), from
    /// its position: (lowest corner, highest corner).
    bodies: Vec<(NodeId, Vec3, Vec3)>,
}
impl std::fmt::Debug for Physics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Physics({} bodies)", self.bodies.len())
    }
}

/// A body's place now: where it stands and whether it may fall now.
#[derive(Clone, Copy, Debug)]
pub struct BodyState {
    pub position: [f32; 3],
    /// Something else moves it (a script's move on its way, or a fall):
    /// `position` is where it will be.
    pub busy: bool,
}

impl Physics {
    /// None when `layout` (in world space, see `Layout::resolved`) has no
    /// unanchored parts.
    pub fn new(layout: &Layout, assets: &mut AssetResolver) -> Option<Self> {
        let flags = layout.effective_all();
        let bodies: Vec<(NodeId, Vec3, Vec3)> = layout
            .nodes
            .iter()
            .filter(|n| !n.anchored && flags.get(&n.id).is_some_and(|f| f.collision))
            .filter_map(|n| {
                let part = n.part.as_ref()?;
                let t = &n.transform;
                // The part's box: its base centre at the origin.
                let half = Vec3::new(
                    part.size[0] * t.scale[0] * 0.5,
                    part.size[1] * t.scale[1] * 0.5,
                    part.size[2] * t.scale[2],
                );
                let turn = Mat3::from_quat(t.rotation_quat());
                let mut lo = Vec3::splat(f32::MAX);
                let mut hi = Vec3::splat(f32::MIN);
                for corner in 0..8 {
                    let c = Vec3::new(
                        if corner & 1 == 0 { -half.x } else { half.x },
                        if corner & 2 == 0 { -half.y } else { half.y },
                        if corner & 4 == 0 { 0.0 } else { half.z },
                    );
                    let p = turn * c;
                    lo = lo.min(p);
                    hi = hi.max(p);
                }
                Some((n.id, lo, hi))
            })
            .collect();
        if bodies.is_empty() {
            return None;
        }
        // Everything else that is solid. Unanchored parts are switchable,
        // so `place_layout` leaves them (and doors) out.
        let mut model = crate::gltf_model::Model::default();
        place_layout(&mut model, layout, assets);
        Some(Self {
            statics: CollisionWorld::from_batches(&model.collision),
            bodies,
        })
    }

    pub fn has_body(&self, node: NodeId) -> bool {
        self.bodies.iter().any(|(id, _, _)| *id == node)
    }

    /// Where each body that should fall lands: (body, height to drop).
    /// `state` gives a body's place now (None: gone or hidden); `source`
    /// maps a script's copy to the part it copies, whose box it has.
    pub fn falls(
        &self,
        ids: &[NodeId],
        state: impl Fn(NodeId) -> Option<BodyState>,
        source: impl Fn(NodeId) -> NodeId,
    ) -> Vec<(NodeId, f32)> {
        let mut placed: Vec<(NodeId, Vec3, Vec3, bool)> = ids
            .iter()
            .filter_map(|id| {
                let original = source(*id);
                let (_, lo, hi) = self.bodies.iter().find(|(b, _, _)| *b == original)?;
                let s = state(*id)?;
                let at = Vec3::from_array(s.position);
                Some((*id, at + *lo, at + *hi, s.busy))
            })
            .collect();
        // Lowest first: what lands first holds up what lands on it.
        placed.sort_by(|a, b| a.1.z.total_cmp(&b.1.z));
        let mut rest: Vec<(Vec3, Vec3)> = Vec::new();
        let mut out = Vec::new();
        for (id, lo, hi, busy) in placed {
            if busy {
                rest.push((lo, hi));
                continue;
            }
            let ground = self.ground(lo, hi, &rest);
            let drop = ground.map_or(0.0, |g| lo.z - g);
            if drop > SETTLED {
                out.push((id, drop));
                let down = Vec3::new(0.0, 0.0, drop);
                rest.push((lo - down, hi - down));
            } else {
                rest.push((lo, hi));
            }
        }
        out
    }

    /// The highest surface under the box (its middle and corners), from
    /// the world and the bodies in `rest`, no higher than its bottom.
    fn ground(&self, lo: Vec3, hi: Vec3, rest: &[(Vec3, Vec3)]) -> Option<f32> {
        let inset = 0.05;
        let (x0, x1) = (lo.x + inset, (hi.x - inset).max(lo.x + inset));
        let (y0, y1) = (lo.y + inset, (hi.y - inset).max(lo.y + inset));
        let points = [
            ((x0 + x1) * 0.5, (y0 + y1) * 0.5),
            (x0, y0),
            (x1, y0),
            (x0, y1),
            (x1, y1),
        ];
        let top = lo.z + SLACK;
        let mut best: Option<f32> = None;
        for (x, y) in points {
            // Pack space: x east, y up, z south.
            if let Some(g) = self.statics.ground_below(Vec3::new(x, top, -y), top) {
                best = Some(best.map_or(g, |b: f32| b.max(g)));
            }
        }
        for (other_lo, other_hi) in rest {
            let overlaps = other_lo.x < hi.x - inset
                && other_hi.x > lo.x + inset
                && other_lo.y < hi.y - inset
                && other_hi.y > lo.y + inset;
            if overlaps && other_hi.z <= top {
                best = Some(best.map_or(other_hi.z, |b| b.max(other_hi.z)));
            }
        }
        best
    }
}

/// How long a fall of `height` metres takes.
pub fn fall_seconds(height: f32) -> f32 {
    (2.0 * height.max(0.0) / GRAVITY).sqrt()
}

#[cfg(test)]
mod tests {
    use crate::world_layout::{AssetRef, Layout, Node, Part, PartShape, Transform};
    use crate::world_logic::Logic;

    fn block(id: u64, size: [f32; 3], at: [f32; 3], anchored: bool) -> Node {
        let mut node = Node::object(
            id,
            format!("Block {id}"),
            AssetRef::Builtin("part".into()),
            Transform::at(at),
        );
        node.part = Some(Part::new(PartShape::Block, size));
        node.anchored = anchored;
        node
    }

    #[test]
    fn unanchored_parts_fall_and_stack() {
        let layout = Layout {
            nodes: vec![
                block(1, [20.0, 20.0, 1.0], [0.0, 0.0, 0.0], true),
                block(2, [2.0, 2.0, 1.0], [0.0, 0.0, 6.0], false),
                block(3, [2.0, 2.0, 0.5], [0.5, 0.0, 12.0], false),
                // Nothing under it: it stays.
                block(4, [1.0, 1.0, 1.0], [80.0, 0.0, 3.0], false),
                // Anchored in the air: stays.
                block(5, [1.0, 1.0, 1.0], [5.0, 5.0, 4.0], true),
            ],
            next_id: 6,
            ..Layout::default()
        };
        let mut assets =
            crate::content_pack::AssetResolver::with_dirs(std::path::Path::new("."), Vec::new());
        let mut logic = Logic::new(&layout);
        logic.set_physics(super::Physics::new(&layout, &mut assets));
        logic.advance(0.0);
        // A 5 m fall takes about a second; the top one falls 11 m.
        logic.advance(0.5);
        let z = |logic: &Logic, id| {
            logic
                .shared()
                .moved()
                .find(|(n, _)| *n == id)
                .map(|(_, p)| p.position[2])
        };
        let halfway = z(&logic, 2).unwrap();
        assert!(halfway < 6.0 && halfway > 1.0, "falling: {halfway}");
        for _ in 0..40 {
            logic.advance(0.1);
        }
        assert!(
            (z(&logic, 2).unwrap() - 1.0).abs() < 0.01,
            "lands on the floor"
        );
        assert!(
            (z(&logic, 3).unwrap() - 2.0).abs() < 0.01,
            "lands on the other block"
        );
        assert_eq!(z(&logic, 4), None, "nothing under it");
        assert_eq!(z(&logic, 5), None, "anchored");
        // The fall goes to players in snapshots, as a fall.
        let snapshot = logic.shared().snapshot();
        assert!(snapshot.moved.iter().any(|m| m.node == 3 && m.fall));
        // Lift it with a move: when the move ends, it falls again.
        let lifted = crate::world_logic::Pose {
            position: [0.5, 0.0, 9.0],
            heading: 0.0,
        };
        logic.apply_script(crate::world_logic::ScriptChange::Move(3, lifted, 0.0));
        for _ in 0..30 {
            logic.advance(0.1);
        }
        assert!((z(&logic, 3).unwrap() - 2.0).abs() < 0.01);
    }
}
