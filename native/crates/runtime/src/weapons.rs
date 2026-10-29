//! Guns the player carries: choosing one, firing, reloading, and what a shot
//! hits. Damage, range and clip size come from the player's own weapon.dat.
use super::State;
use glam::{Quat, Vec3};
use sa_scene::weapon_list::Weapon;

/// Seconds a reload takes.
const RELOAD: f32 = 1.4;
/// Clips of spare ammunition each gun starts with.
const SPARE_CLIPS: u32 = 8;
/// Shotgun pellet spread, as a fraction of the aim direction.
const SPREAD: f32 = 0.06;

#[derive(Clone, Debug)]
pub struct Held {
    pub weapon: Weapon,
    pub loaded: u32,
    pub spare: u32,
}

#[derive(Default)]
pub struct Arsenal {
    pub guns: Vec<Held>,
    /// Index into `guns`, or `None` for bare fists.
    pub current: Option<usize>,
    cooldown: f32,
    reloading: f32,
}

/// What a trigger pull did this frame.
#[derive(Debug, PartialEq)]
pub enum Shot {
    None,
    Fired,
    Empty,
}

impl Arsenal {
    pub fn stocked(guns: Vec<Weapon>) -> Self {
        Self {
            guns: guns
                .into_iter()
                .map(|weapon| Held {
                    loaded: weapon.clip,
                    spare: weapon.clip * SPARE_CLIPS,
                    weapon,
                })
                .collect(),
            ..Self::default()
        }
    }
    pub fn held(&self) -> Option<&Held> {
        self.current.and_then(|index| self.guns.get(index))
    }
    pub fn reloading(&self) -> bool {
        self.reloading > 0.0
    }
    /// Step through fists and every gun, wrapping around.
    pub fn cycle(&mut self, forward: bool) {
        let count = self.guns.len() + 1;
        let position = self.current.map_or(0, |index| index + 1);
        let next = if forward {
            (position + 1) % count
        } else {
            (position + count - 1) % count
        };
        self.current = next.checked_sub(1);
        self.reloading = 0.0;
        self.cooldown = self.cooldown.max(0.25);
    }
    pub fn refill(&mut self) {
        for held in &mut self.guns {
            held.loaded = held.weapon.clip;
            held.spare = held.weapon.clip * SPARE_CLIPS;
        }
    }
    /// Advance timers and fire if the trigger is held and the gun is ready.
    pub fn update(&mut self, trigger: bool, dt: f32) -> Shot {
        self.cooldown = (self.cooldown - dt).max(0.0);
        if self.reloading > 0.0 {
            self.reloading -= dt;
            if self.reloading <= 0.0 {
                self.reloading = 0.0;
                if let Some(held) = self.current.and_then(|index| self.guns.get_mut(index)) {
                    let take = (held.weapon.clip - held.loaded).min(held.spare);
                    held.loaded += take;
                    held.spare -= take;
                }
            }
            return Shot::None;
        }
        let Some(index) = self.current else {
            return Shot::None;
        };
        if !trigger || self.cooldown > 0.0 {
            return Shot::None;
        }
        let held = &mut self.guns[index];
        if held.loaded == 0 {
            self.cooldown = 0.3;
            if held.spare > 0 {
                self.reloading = RELOAD;
            }
            return Shot::Empty;
        }
        held.loaded -= 1;
        self.cooldown = held.weapon.interval();
        if held.loaded == 0 && held.spare > 0 {
            self.reloading = RELOAD;
        }
        Shot::Fired
    }
}

/// Directions for each pellet of a shot: straight for most guns, a fixed
/// cone for shotguns so the spread is the same every time.
pub fn pellet_directions(aim: Vec3, pellets: u32) -> Vec<Vec3> {
    if pellets <= 1 {
        return vec![aim];
    }
    let right = aim.cross(Vec3::Y).normalize_or(Vec3::X);
    let up = right.cross(aim).normalize_or(Vec3::Y);
    (0..pellets)
        .map(|i| {
            let angle = i as f32 / pellets as f32 * std::f32::consts::TAU;
            let ring = if i % 2 == 0 { SPREAD } else { SPREAD * 0.5 };
            (aim + (right * angle.cos() + up * angle.sin()) * ring).normalize()
        })
        .collect()
}

/// A gun's fire and reload clips, from its animation group in anim.img.
pub struct WeaponAnims {
    pub package: std::sync::Arc<sa_assets::ifp::Package>,
    pub fire: Option<String>,
    pub reload: Option<String>,
}

/// Fist punches in ped.ifp, thrown in turn.
const PUNCHES: [&str; 3] = ["fighta_1", "fighta_2", "fighta_3"];
/// Seconds between punches.
const PUNCH_INTERVAL: f32 = 0.45;
/// How far a punch reaches and what it takes off a pedestrian.
const PUNCH_REACH: f32 = 1.4;
const PUNCH_DAMAGE: f32 = 10.0;

/// HAnim id of the right hand in San Andreas peds.
const RIGHT_HAND: i32 = 24;

impl State {
    /// A gunshot for `group`, or the reload click for `None`, at `at`.
    fn play_weapon_sound(&mut self, group: Option<&str>, at: Vec3) {
        let (Some(audio), Some(sounds)) = (&mut self.audio, &self.gameplay_audio.sounds) else {
            return;
        };
        let sound = match group {
            Some(group) => sounds.gunshot(group),
            None => sounds.reload(),
        };
        if let Some(sound) = sound {
            if let Err(error) = audio.play_spatial(sound, at.to_array()) {
                eprintln!("Weapon sound unavailable: {error:#}");
            }
        }
    }

    /// The held gun's animation group, loaded once from anim.img and made
    /// playable on the player's ped.
    fn held_weapon_anims(&mut self) -> Option<&WeaponAnims> {
        let group = self.arsenal.held()?.weapon.group.clone();
        if !self.weapon_anims.contains_key(&group) {
            let loaded = sa_scene::weapon_list::weapon_clips(&self.resource_game_dir, &group)
                .map_err(|e| eprintln!("Weapon animations {group} unavailable: {e:#}"))
                .ok()
                .map(|package| {
                    let (fire, reload) = sa_scene::weapon_list::fire_and_reload(&package, &group);
                    WeaponAnims {
                        package: std::sync::Arc::new(package),
                        fire,
                        reload,
                    }
                });
            self.weapon_anims.insert(group.clone(), loaded);
        }
        let anims = self.weapon_anims.get(&group)?.as_ref()?;
        if let Some((ped, _)) = &mut self.ped {
            ped.share_clips(&anims.package);
        }
        Some(anims)
    }

    /// Plays an upper-body clip on the player, such as a shot or a reload.
    fn overlay_player(&mut self, names: &[&str]) {
        if let Some((ped, _)) = &self.ped {
            self.ped_animator
                .overlay(names, |name| ped.clip_duration(name));
        }
    }

    /// Draw the held gun in CJ's right hand, following the current clip.
    pub(super) fn pose_held_gun(&mut self) {
        self.gun_drawn = None;
        let visible = self.walking && !self.driving && self.third_person && self.ped_visible;
        let Some(model) = self
            .arsenal
            .held()
            .map(|held| held.weapon.model)
            .filter(|_| visible)
        else {
            return;
        };
        if !self.gun_meshes.contains_key(&model) {
            let models = self.weapon_models.get_or_insert_with(|| {
                sa_scene::weapon_list::weapon_models(&self.resource_game_dir)
            });
            let loaded = models.get(&model).cloned().and_then(|(name, txd)| {
                sa_scene::load_weapon_model(&self.resource_game_dir, &name, &txd)
                    .and_then(|scene| {
                        Self::upload_scene(
                            &self.device,
                            &self.queue,
                            &self.image_layout,
                            &self.sampler,
                            &self.mipmaps,
                            scene,
                        )
                    })
                    .map_err(|e| eprintln!("Gun model {name} unavailable: {e:#}"))
                    .ok()
            });
            self.gun_meshes.insert(model, loaded);
        }
        let (Some(Some(batches)), Some((ped, _)), Some(player)) =
            (self.gun_meshes.get(&model), &self.ped, &self.player)
        else {
            return;
        };
        let Ok(hand) = ped.bone_matrix_blend(&self.ped_animator.plays(), RIGHT_HAND) else {
            return;
        };
        let body = glam::Mat4::from_rotation_translation(
            Quat::from_rotation_y(self.ped_yaw + std::f32::consts::PI),
            player.feet,
        ) * hand;
        for batch in batches {
            let mut raw = batch.base.clone();
            for v in raw.as_chunks_mut::<9>().0.iter_mut() {
                let p = body.transform_point3(Vec3::new(v[0], v[1], v[2]));
                v[..3].copy_from_slice(&p.to_array());
            }
            self.queue
                .write_buffer(&batch.buffer, 0, bytemuck::cast_slice(&raw));
        }
        self.gun_drawn = Some(model);
    }

    pub(super) fn update_weapons(&mut self, dt: f32) {
        if self.arsenal.guns.is_empty() && !self.arsenal_loaded {
            let guns = sa_scene::weapon_list::original_guns(&self.resource_game_dir);
            // Keep trying until the install is known; an empty first read
            // used to leave the player with fists for the whole session.
            if !guns.is_empty() || !self.resource_game_dir.as_os_str().is_empty() {
                eprintln!("Loaded {} guns from weapon.dat", guns.len());
                self.arsenal_loaded = true;
                self.arsenal = Arsenal::stocked(guns);
                self.arsenal.cycle(true);
            }
        }
        let on_foot = self.walking && !self.driving && self.menu.page.is_none();
        let trigger = on_foot && self.captured && self.fire_held;
        let was_reloading = self.arsenal.reloading();
        let shot = self.arsenal.update(trigger, dt);
        let muzzle_at = self
            .player
            .as_ref()
            .map_or(self.position, |player| player.feet + Vec3::Y * 1.4);
        let anims = self
            .held_weapon_anims()
            .map(|a| (a.fire.clone(), a.reload.clone()));
        if !was_reloading && self.arsenal.reloading() {
            self.play_weapon_sound(None, muzzle_at);
            if let Some((_, Some(reload))) = &anims {
                self.overlay_player(&[reload]);
            }
        }
        if shot == Shot::Fired {
            match &anims {
                Some((Some(fire), _)) => self.overlay_player(&[fire, "shot_partial"]),
                _ => self.overlay_player(&["shot_partial"]),
            }
        }
        self.punch_cooldown = (self.punch_cooldown - dt).max(0.0);
        if trigger && self.arsenal.current.is_none() && self.punch_cooldown <= 0.0 {
            self.punch(muzzle_at);
        }
        self.menu.weapon_hud = on_foot.then(|| match self.arsenal.held() {
            Some(held) => (
                held.weapon.label(),
                held.loaded,
                held.spare,
                true,
                self.arsenal.reloading(),
            ),
            // Say how to get a gun, or why there is none, instead of
            // showing nothing at all.
            None if self.arsenal.guns.is_empty() => (
                "No guns: weapon.dat not found".to_owned(),
                0,
                0,
                false,
                false,
            ),
            None => ("Fists".to_owned(), 0, 0, false, false),
        });
        if shot != Shot::Fired {
            return;
        }
        let Some(held) = self.arsenal.held() else {
            return;
        };
        let (damage, range, pellets) =
            (held.weapon.damage, held.weapon.range, held.weapon.pellets());
        let origin = self.position;
        let aim = Vec3::new(
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.cos() * self.pitch.cos(),
        );
        let group = held.weapon.group.clone();
        self.play_weapon_sound(Some(&group), muzzle_at);
        self.crowd.scare(muzzle_at);
        self.report_crime(crate::wanted::Crime::Gunfire, muzzle_at);
        for direction in pellet_directions(aim, pellets) {
            // Walls stop bullets: shorten the ray to the first solid thing.
            let end = origin + direction * range;
            let reach = self.collision.as_ref().map_or(range, |world| {
                world.clip_camera(origin, end).distance(origin)
            });
            // Whichever is nearer along the shot, a person or a car, is hit.
            let person = self.crowd.first_hit(origin, direction, reach);
            let car = self.first_car_hit(origin, direction, reach);
            match (person, car) {
                (Some((index, near)), car) if car.is_none_or(|(_, far)| near <= far) => {
                    self.crowd.damage(index, damage);
                }
                (_, Some((which, _))) => self.hurt_car(which, damage),
                _ => {}
            }
        }
    }
}

impl State {
    /// A fist punch: CJ swings, and the nearest pedestrian just in front
    /// takes a hit.
    fn punch(&mut self, chest: Vec3) {
        self.punch_cooldown = PUNCH_INTERVAL;
        let name = PUNCHES[self.punches % PUNCHES.len()];
        self.punches += 1;
        self.overlay_player(&[name, "fighta_1"]);
        let facing = Vec3::new(self.ped_yaw.sin(), 0.0, self.ped_yaw.cos());
        let target = self
            .crowd
            .walkers
            .iter()
            .enumerate()
            .filter(|(_, w)| w.dead_for.is_none())
            .map(|(i, w)| (i, w.feet + Vec3::Y * 1.2 - chest))
            .filter(|(_, to)| to.length() < PUNCH_REACH && to.normalize_or_zero().dot(facing) > 0.5)
            .min_by(|a, b| a.1.length().total_cmp(&b.1.length()))
            .map(|(i, _)| i);
        if let Some(index) = target {
            self.crowd.damage(index, PUNCH_DAMAGE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pistol() -> Weapon {
        Weapon {
            name: "PISTOL".into(),
            range: 35.0,
            model: 346,
            slot: 2,
            group: "colt45".into(),
            clip: 3,
            damage: 25.0,
        }
    }

    #[test]
    fn cycling_goes_through_fists_and_every_gun() {
        let mut arsenal = Arsenal::stocked(vec![pistol(), pistol()]);
        assert!(arsenal.held().is_none());
        arsenal.cycle(true);
        assert_eq!(arsenal.current, Some(0));
        arsenal.cycle(true);
        arsenal.cycle(true);
        assert_eq!(arsenal.current, None, "wraps back to fists");
        arsenal.cycle(false);
        assert_eq!(arsenal.current, Some(1));
    }

    #[test]
    fn firing_respects_the_rate_empties_the_clip_and_reloads() {
        let mut arsenal = Arsenal::stocked(vec![pistol()]);
        assert_eq!(arsenal.update(true, 0.1), Shot::None, "fists do not shoot");
        arsenal.cycle(true);
        arsenal.update(false, 1.0);
        assert_eq!(arsenal.update(true, 0.01), Shot::Fired);
        assert_eq!(arsenal.update(true, 0.01), Shot::None, "too soon");
        let mut shots = 1;
        for _ in 0..200 {
            if arsenal.update(true, 0.05) == Shot::Fired {
                shots += 1;
            }
            if arsenal.reloading() {
                break;
            }
        }
        assert_eq!(shots, 3, "one clip");
        assert_eq!(arsenal.held().unwrap().loaded, 0);
        for _ in 0..40 {
            arsenal.update(false, 0.05);
        }
        let held = arsenal.held().unwrap();
        assert_eq!(held.loaded, 3);
        assert_eq!(held.spare, 3 * SPARE_CLIPS - 3);
    }

    #[test]
    fn shotgun_pellets_spread_around_the_aim() {
        let aim = Vec3::new(0.0, 0.0, 1.0);
        let pellets = pellet_directions(aim, 8);
        assert_eq!(pellets.len(), 8);
        assert!(pellets
            .iter()
            .all(|p| p.dot(aim) > 0.99 && (p.length() - 1.0).abs() < 1e-4));
        assert!(pellets.iter().any(|p| p.x > 0.01) && pellets.iter().any(|p| p.x < -0.01));
        assert_eq!(pellet_directions(aim, 1), vec![aim]);
    }
}
