//! The wanted level: up to six stars for crimes the police notice.
//!
//! As in SA, the police mostly react to what they see. A crime close to a
//! cop or a police car counts in full; out of their sight only the worst
//! (killing, blowing cars up) count, at a quarter, so a long rampage still
//! brings them in the end. Once the player is wanted, every crime counts.
//! Staying out of every cop's sight for long enough loses a star at a time;
//! the stars flash while nobody can see the player.

/// Most stars a player can have.
pub const MAX_LEVEL: u8 = 6;
/// Points needed for each star.
const THRESHOLDS: [u32; MAX_LEVEL as usize] = [50, 180, 550, 1200, 2400, 4600];
/// Seconds out of sight before the stars start to flash.
const FLASH_AFTER: f32 = 2.0;
/// Shots this close together count as one burst of gunfire.
const GUNFIRE_GAP: f32 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Crime {
    /// Firing a gun.
    Gunfire,
    HitPerson,
    KillPerson,
    HitCop,
    KillCop,
    /// Shooting or ramming a car.
    DamageCar,
    /// Shooting, ramming or stealing a police car.
    HitPoliceCar,
    BlowUpCar,
    BlowUpPolice,
    CarJack,
}

impl Crime {
    fn points(self) -> u32 {
        match self {
            Self::Gunfire => 15,
            Self::HitPerson => 10,
            Self::KillPerson => 60,
            Self::HitCop => 50,
            Self::KillCop => 150,
            Self::DamageCar => 5,
            Self::HitPoliceCar => 25,
            Self::BlowUpCar => 80,
            Self::BlowUpPolice => 200,
            Self::CarJack => 30,
        }
    }
    /// Crimes bad enough to be reported even with no cop around.
    fn reported(self) -> bool {
        matches!(
            self,
            Self::KillPerson | Self::KillCop | Self::BlowUpCar | Self::BlowUpPolice
        )
    }
    /// Crimes against the police themselves, which always earn a star.
    fn against_police(self) -> bool {
        matches!(
            self,
            Self::HitCop | Self::KillCop | Self::HitPoliceCar | Self::BlowUpPolice
        )
    }
}

#[derive(Clone, Debug, Default)]
pub struct Wanted {
    points: u32,
    level: u8,
    /// Seconds since a cop last saw the player.
    unseen: f32,
    /// Seconds since the last shot that counted.
    since_gunfire: f32,
}

/// Seconds out of sight it takes to lose a star.
fn evade_seconds(level: u8) -> f32 {
    10.0 + 5.0 * f32::from(level)
}

/// Stars for `points`.
fn level_for(points: u32) -> u8 {
    THRESHOLDS.iter().filter(|&&t| points >= t).count() as u8
}

impl Wanted {
    pub fn level(&self) -> u8 {
        self.level
    }
    /// The stars flash while the player is out of every cop's sight.
    pub fn flashing(&self) -> bool {
        self.level > 0 && self.unseen > FLASH_AFTER
    }
    /// A crime, `witnessed` when a cop was close enough to see it. Returns
    /// true when it raised the wanted level.
    pub fn commit(&mut self, crime: Crime, witnessed: bool) -> bool {
        if crime == Crime::Gunfire {
            if self.since_gunfire < GUNFIRE_GAP {
                return false;
            }
            self.since_gunfire = 0.0;
        }
        let witnessed = witnessed || self.level > 0;
        let points = match (witnessed, crime.reported()) {
            (true, _) => crime.points(),
            (false, true) => crime.points() / 4,
            (false, false) => return false,
        };
        let before = self.level;
        let cap = THRESHOLDS[usize::from(MAX_LEVEL) - 1] * 2;
        self.points = (self.points + points).min(cap);
        let mut level = level_for(self.points);
        if witnessed && crime.against_police() {
            level = level.max(1);
        }
        self.set_level(level.max(before));
        if witnessed {
            self.unseen = 0.0;
        }
        self.level > before
    }
    /// Advance the clock; `seen` when a cop can see the player now.
    pub fn update(&mut self, seen: bool, dt: f32) {
        self.since_gunfire += dt;
        if self.level == 0 {
            self.unseen = 0.0;
            return;
        }
        if seen {
            self.unseen = 0.0;
            return;
        }
        self.unseen += dt;
        if self.unseen >= evade_seconds(self.level) {
            self.set_level(self.level - 1);
            // Start the lower level afresh, not just short of the next star.
            self.points = match self.level {
                0 => 0,
                n => THRESHOLDS[usize::from(n) - 1],
            };
            self.unseen = 0.0;
        }
    }
    /// Set the stars directly (busted, a script, or losing one), keeping the
    /// points in step so the next crime carries on from there.
    pub fn set_level(&mut self, level: u8) {
        self.level = level.min(MAX_LEVEL);
        let floor = match self.level {
            0 => 0,
            n => THRESHOLDS[usize::from(n) - 1],
        };
        let ceiling = THRESHOLDS
            .get(usize::from(self.level))
            .map_or(u32::MAX, |next| next - 1);
        self.points = self.points.clamp(floor, ceiling);
    }
    pub fn clear(&mut self) {
        *self = Self {
            since_gunfire: self.since_gunfire,
            ..Self::default()
        };
    }
}

/// Police cars sent after a player with `level` stars. Kept low so a chase
/// stays cheap on weak PCs.
pub fn pursuit_cars(level: u8) -> usize {
    [0, 1, 2, 3, 4, 4, 4][usize::from(level.min(MAX_LEVEL))]
}

/// Officers riding in each pursuing car.
pub fn crew(level: u8) -> u8 {
    if level >= 3 {
        2
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_killing_in_front_of_a_cop_gives_a_star() {
        let mut wanted = Wanted::default();
        assert!(wanted.commit(Crime::KillPerson, true));
        assert_eq!(wanted.level(), 1);
    }

    #[test]
    fn unseen_small_crimes_go_unnoticed_but_a_rampage_does_not() {
        let mut wanted = Wanted::default();
        for _ in 0..20 {
            wanted.commit(Crime::HitPerson, false);
            wanted.commit(Crime::CarJack, false);
        }
        assert_eq!(wanted.level(), 0);
        for _ in 0..3 {
            wanted.commit(Crime::KillPerson, false);
        }
        assert_eq!(wanted.level(), 0);
        wanted.commit(Crime::KillPerson, false);
        assert_eq!(wanted.level(), 1, "four killings get reported");
    }

    #[test]
    fn touching_the_police_always_earns_a_star() {
        let mut wanted = Wanted::default();
        wanted.commit(Crime::HitPoliceCar, true);
        assert_eq!(wanted.level(), 1);
    }

    #[test]
    fn stars_climb_with_crimes_and_stop_at_six() {
        let mut wanted = Wanted::default();
        wanted.commit(Crime::KillPerson, true);
        for _ in 0..4 {
            wanted.commit(Crime::KillCop, false);
        }
        assert_eq!(wanted.level(), 3, "once wanted, every crime counts");
        for _ in 0..200 {
            wanted.commit(Crime::BlowUpPolice, true);
        }
        assert_eq!(wanted.level(), MAX_LEVEL);
    }

    #[test]
    fn a_burst_of_gunfire_counts_once() {
        let mut wanted = Wanted::default();
        wanted.update(false, 5.0);
        for _ in 0..30 {
            wanted.commit(Crime::Gunfire, true);
        }
        assert_eq!(wanted.points, 15);
        for _ in 0..3 {
            wanted.update(false, 1.0);
            wanted.commit(Crime::Gunfire, true);
        }
        assert_eq!(wanted.level(), 1);
    }

    #[test]
    fn hiding_loses_one_star_at_a_time_and_flashes_meanwhile() {
        let mut wanted = Wanted::default();
        wanted.set_level(2);
        wanted.update(false, 1.0);
        assert!(!wanted.flashing());
        wanted.update(false, 2.0);
        assert!(wanted.flashing());
        wanted.update(true, 0.1);
        assert!(!wanted.flashing(), "spotted again");
        for _ in 0..19 {
            wanted.update(false, 1.0);
        }
        assert_eq!(wanted.level(), 2);
        wanted.update(false, 1.0);
        assert_eq!(wanted.level(), 1);
        for _ in 0..15 {
            wanted.update(false, 1.0);
        }
        assert_eq!(wanted.level(), 0);
        assert!(!wanted.flashing());
        // The points went with the stars: a small crime needs a witness again.
        wanted.commit(Crime::HitPerson, false);
        assert_eq!(wanted.level(), 0);
    }

    #[test]
    fn more_stars_send_more_cars() {
        assert_eq!(pursuit_cars(0), 0);
        assert!(pursuit_cars(1) < pursuit_cars(3));
        assert_eq!(pursuit_cars(MAX_LEVEL), 4);
        assert_eq!(pursuit_cars(200), 4);
        assert!(crew(1) < crew(4));
    }
}
