//! Arranging the selection: nudge with the arrow keys, align to the last
//! selected object, and space evenly. Positions are in the world, so
//! objects inside turned models line up in the world too.
use super::BuildMode;
use glam::Vec3;
use sa_editor::history::Command;
use sa_editor::ops;
use sa_scene::world_layout::NodeId;

/// Which side of the boxes lines up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Edge {
    Min,
    Center,
    Max,
}

/// World axes as the Properties panel shows them: 0 east (X), 1 north
/// (Y), 2 up (Z).
pub const AXES: [&str; 3] = ["X (east)", "Y (north)", "Z (up)"];

impl BuildMode {
    /// The selected objects and groups that move, topmost only (an object
    /// in a selected group goes with it), unlocked.
    fn arranged(&self) -> Vec<NodeId> {
        let layout = &self.doc.layout;
        self.selection
            .iter()
            .copied()
            .filter(|id| {
                layout.node(*id).is_some()
                    && !layout.effective_locked(*id)
                    && !layout
                        .ancestry(*id)
                        .skip(1)
                        .any(|a| self.selection.contains(&a.id))
            })
            .collect()
    }

    /// The world box around an object or everything in a group, in world
    /// axes (east, north, up), as drawn.
    fn world_box(&self, id: NodeId) -> Option<(Vec3, Vec3)> {
        let mut ids = self.doc.layout.descendants(id);
        ids.push(id);
        let mut out: Option<(Vec3, Vec3)> = None;
        for object in ids.iter().filter_map(|i| self.gpu.objects.get(i)) {
            // Render space: x east, y up, z south.
            let (a, b) = object.bounds;
            let (lo, hi) = (Vec3::new(a.x, -b.z, a.y), Vec3::new(b.x, -a.z, b.y));
            out = Some(match out {
                None => (lo, hi),
                Some((l, h)) => (l.min(lo), h.max(hi)),
            });
        }
        out
    }

    /// Move `ids` by world offsets (east, north, up), as one undo step.
    fn shift(&mut self, label: &str, moves: Vec<(NodeId, Vec3)>) {
        let moves: Vec<_> = moves
            .into_iter()
            .filter(|(_, d)| d.length_squared() > 1e-10)
            .collect();
        if moves.is_empty() {
            return;
        }
        // A group without a pivot gets one first, so it moves about itself.
        for (id, _) in &moves {
            let unpivoted = self
                .doc
                .layout
                .node(*id)
                .is_some_and(|n| n.is_group() && n.transform.is_unmoved());
            if unpivoted {
                if let Ok(command) = ops::center_pivot(&self.doc, *id) {
                    self.execute(command);
                }
            }
        }
        let mut changes = Vec::new();
        for (id, delta) in moves {
            let Some(before) = self.doc.layout.node(id).cloned() else {
                continue;
            };
            let mut world = self.doc.layout.world_transform(id);
            for (axis, d) in delta.to_array().iter().enumerate() {
                world.position[axis] += d;
            }
            let mut after = before.clone();
            after.transform = self.doc.layout.local_in(before.parent, &world);
            if !after.transform.is_valid() {
                return self.warn("That would move something outside the world");
            }
            changes.push((before, after));
        }
        self.execute(Command::Modify {
            label: label.into(),
            changes,
        });
    }

    /// Move the selection one step along a world direction (the grid
    /// step with snapping on, else 10 cm).
    pub fn nudge(&mut self, direction: Vec3) {
        let step = if self.snap.enabled {
            self.snap.step.max(0.01)
        } else {
            0.1
        };
        let moves = self
            .arranged()
            .into_iter()
            .map(|id| (id, direction * step))
            .collect();
        self.shift("Nudge", moves);
    }

    /// The arrow key's direction in the world: up goes the way the camera
    /// looks (along the nearest world axis), right to its right.
    pub fn arrow_direction(&self, forward: bool, sign: f32) -> Vec3 {
        let look = self.camera.forward();
        // Render to world axes: east = x, north = -z.
        let ahead = Vec3::new(look.x, -look.z, 0.0);
        let axis = if ahead.x.abs() > ahead.y.abs() {
            Vec3::new(ahead.x.signum(), 0.0, 0.0)
        } else {
            Vec3::new(0.0, ahead.y.signum(), 0.0)
        };
        let dir = if forward {
            axis
        } else {
            // Right of the view: ahead turned a quarter clockwise.
            Vec3::new(axis.y, -axis.x, 0.0)
        };
        dir * sign
    }

    /// Line the selection up with the last selected object along `axis`.
    pub fn align(&mut self, axis: usize, edge: Edge) {
        let ids = self.arranged();
        let Some(&target) = self.selection.last() else {
            return;
        };
        if ids.len() < 2 {
            return self
                .warn("Select two or more objects to align; the last one selected stays put");
        }
        let Some(goal) = self.world_box(target).map(|b| side(b, axis, edge)) else {
            return;
        };
        let moves = ids
            .into_iter()
            .filter(|id| *id != target)
            .filter_map(|id| {
                let now = side(self.world_box(id)?, axis, edge);
                let mut d = Vec3::ZERO;
                d[axis] = goal - now;
                Some((id, d))
            })
            .collect();
        self.shift(&format!("Align {}", AXES[axis]), moves);
    }

    /// Space the selection evenly along `axis`, between the two at the
    /// ends (their middles the same distance apart).
    pub fn distribute(&mut self, axis: usize) {
        let mut placed: Vec<(NodeId, f32)> = self
            .arranged()
            .into_iter()
            .filter_map(|id| Some((id, side(self.world_box(id)?, axis, Edge::Center))))
            .collect();
        if placed.len() < 3 {
            return self.warn("Select three or more objects to space them evenly");
        }
        placed.sort_by(|a, b| a.1.total_cmp(&b.1));
        let (first, last) = (placed[0].1, placed[placed.len() - 1].1);
        let gap = (last - first) / (placed.len() - 1) as f32;
        let moves = placed
            .iter()
            .enumerate()
            .map(|(i, (id, at))| {
                let mut d = Vec3::ZERO;
                d[axis] = first + gap * i as f32 - at;
                (*id, d)
            })
            .collect();
        self.shift(&format!("Space evenly along {}", AXES[axis]), moves);
    }
}

fn side((lo, hi): (Vec3, Vec3), axis: usize, edge: Edge) -> f32 {
    match edge {
        Edge::Min => lo[axis],
        Edge::Center => (lo[axis] + hi[axis]) * 0.5,
        Edge::Max => hi[axis],
    }
}
