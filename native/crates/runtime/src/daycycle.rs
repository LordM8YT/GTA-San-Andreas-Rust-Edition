//! Day/night cycle driven by the player's own `data/timecyc.dat`.
//!
//! San Andreas keys its colours at eight times per weather. Every weather
//! block in the file is read; the weather changes every few game hours from
//! the set that fits the part of the map the player is in, and blends over a
//! game hour. One game minute passes per real second, as in the original game.
use glam::Vec3;
use std::path::Path;

/// Hours of the eight timecyc rows.
const HOURS: [f32; 8] = [0.0, 5.0, 6.0, 7.0, 12.0, 19.0, 20.0, 22.0];
const SUNRISE: f32 = 6.0;
const SUNSET: f32 = 20.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub ambient: Vec3,
    pub ambient_objects: Vec3,
    pub sky_top: Vec3,
    pub sky_bottom: Vec3,
    pub sun_core: Vec3,
}

impl Palette {
    fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            ambient: self.ambient.lerp(other.ambient, t),
            ambient_objects: self.ambient_objects.lerp(other.ambient_objects, t),
            sky_top: self.sky_top.lerp(other.sky_top, t),
            sky_bottom: self.sky_bottom.lerp(other.sky_bottom, t),
            sun_core: self.sun_core.lerp(other.sun_core, t),
        }
    }
}

/// How a weather looks beyond its timecyc colours, by its index in the file.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Kind {
    /// Haze density relative to clear weather.
    fog: f32,
    /// Strength of direct sunlight and shadows.
    sun: f32,
    rain: f32,
}

fn kind(weather: usize) -> Kind {
    match weather {
        // Cloudy Los Santos, San Fierro, Las Venturas and countryside.
        4 | 7 | 12 | 15 => Kind {
            fog: 1.6,
            sun: 0.45,
            rain: 0.0,
        },
        // Rainy San Fierro and countryside.
        8 | 16 => Kind {
            fog: 3.0,
            sun: 0.15,
            rain: 1.0,
        },
        // Foggy San Fierro.
        9 => Kind {
            fog: 8.0,
            sun: 0.3,
            rain: 0.0,
        },
        // Desert sandstorm.
        19 => Kind {
            fog: 10.0,
            sun: 0.25,
            rain: 0.0,
        },
        _ => Kind {
            fog: 1.0,
            sun: 1.0,
            rain: 0.0,
        },
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    LosSantos,
    SanFierro,
    LasVenturas,
    Countryside,
    Desert,
}

/// Rough map areas in San Andreas coordinates (X east, Y north).
pub fn region_at(x: f32, y: f32) -> Region {
    if x < -1300.0 && y > -1000.0 {
        Region::SanFierro
    } else if x > 800.0 && y > 600.0 {
        Region::LasVenturas
    } else if y > 600.0 {
        Region::Desert
    } else if x > 0.0 {
        Region::LosSantos
    } else {
        Region::Countryside
    }
}

/// Weathers each area cycles through; repeats make an entry more likely.
fn choices(region: Region) -> &'static [usize] {
    match region {
        Region::LosSantos => &[0, 0, 1, 1, 2, 3, 4],
        Region::SanFierro => &[5, 5, 6, 7, 8, 9],
        Region::LasVenturas => &[10, 10, 11, 12],
        Region::Countryside => &[13, 14, 14, 15, 16],
        Region::Desert => &[17, 17, 18, 18, 19],
    }
}

/// Game minutes between weather changes, and how long a change blends.
const WEATHER_PERIOD: f32 = 6.0 * 60.0;
const WEATHER_BLEND: f32 = 60.0;

/// Values sampled for one frame, ready for the scene uniform.
#[derive(Clone, Copy, Debug)]
pub struct Lighting {
    /// Direction towards the sun (or the moon at night), world space, Y up.
    pub sun: Vec3,
    /// 0 at night, 1 in full daylight.
    pub daylight: f32,
    pub sun_color: Vec3,
    /// Multiplier for the original prelit colours, which were baked for day.
    pub ambient: Vec3,
    pub sky: Vec3,
    /// Haze density multiplier; 1 for clear weather.
    pub fog: f32,
    /// Shadow strength multiplier; 1 for clear weather.
    pub shadow: f32,
    /// 0 for dry weather, 1 for full rain.
    pub rain: f32,
}

pub struct DayCycle {
    /// One eight-row palette per weather in the file.
    blocks: Vec<[Palette; 8]>,
    /// Minutes since midnight.
    pub minutes: f32,
    /// Weather fading out and weather fading in, as timecyc block indices.
    weather: (usize, usize),
    /// Game minutes since the last weather change.
    since_change: f32,
    changes: u32,
    region: Region,
    /// A weather chosen in the settings, which stops the cycle.
    pub fixed_weather: Option<usize>,
}

impl Default for DayCycle {
    fn default() -> Self {
        Self {
            blocks: vec![fallback()],
            minutes: 12.0 * 60.0,
            weather: (0, 0),
            since_change: WEATHER_BLEND,
            changes: 0,
            region: Region::LosSantos,
            fixed_weather: None,
        }
    }
}

impl DayCycle {
    pub fn load(game: &Path) -> Self {
        let blocks = std::fs::read_to_string(game.join("data/timecyc.dat"))
            .map_err(anyhow::Error::from)
            .and_then(|text| parse(&text))
            .unwrap_or_else(|error| {
                eprintln!("timecyc.dat unavailable, using built-in colours: {error:#}");
                vec![fallback()]
            });
        Self {
            blocks,
            ..Default::default()
        }
    }
    pub fn advance(&mut self, seconds: f32) {
        self.minutes = (self.minutes + seconds).rem_euclid(24.0 * 60.0);
        self.advance_weather(seconds);
    }
    /// Weather keeps changing while the clock is held at a fixed hour.
    pub fn advance_weather(&mut self, seconds: f32) {
        self.since_change += seconds;
        if self.since_change >= WEATHER_PERIOD {
            self.changes = self.changes.wrapping_add(1);
            let list = choices(self.region);
            // A fixed scramble of the change count, so every run differs
            // little from the last but still varies.
            let pick = (self.changes.wrapping_mul(2_654_435_761) >> 16) as usize % list.len();
            self.change_to(list[pick]);
        }
    }
    /// Tell the cycle where the player is, in San Andreas coordinates.
    pub fn set_position(&mut self, x: f32, y: f32) {
        self.region = region_at(x, y);
    }
    fn change_to(&mut self, weather: usize) {
        self.weather = (self.current_weather(), weather);
        self.since_change = 0.0;
    }
    /// The weather the player mostly sees right now.
    pub fn current_weather(&self) -> usize {
        if let Some(fixed) = self.fixed_weather {
            return fixed;
        }
        if self.since_change < WEATHER_BLEND * 0.5 {
            self.weather.0
        } else {
            self.weather.1
        }
    }
    fn blend(&self) -> (usize, usize, f32) {
        match self.fixed_weather {
            Some(fixed) => (fixed, fixed, 0.0),
            None => (
                self.weather.0,
                self.weather.1,
                (self.since_change / WEATHER_BLEND).clamp(0.0, 1.0),
            ),
        }
    }
    fn block(&self, weather: usize) -> &[Palette; 8] {
        self.blocks.get(weather).unwrap_or(&self.blocks[0])
    }
    pub fn clock(&self) -> (u32, u32) {
        let minutes = self.minutes as u32;
        (minutes / 60 % 24, minutes % 60)
    }
    pub fn palette(&self, hour: f32) -> Palette {
        let (from, to, t) = self.blend();
        let a = sample(self.block(from), hour);
        if t <= 0.0 || from == to {
            return a;
        }
        a.lerp(sample(self.block(to), hour), t)
    }
    fn noon(&self) -> Palette {
        sample(&self.blocks[0], 12.0)
    }
    pub fn lighting(&self, hour: f32) -> Lighting {
        let palette = self.palette(hour);
        // Brightness is measured against clear noon, so a dull day stays dull.
        let noon = self.noon();
        let (from, to, t) = self.blend();
        let (a, b) = (kind(from), kind(to));
        let weather = Kind {
            fog: a.fog + (b.fog - a.fog) * t,
            sun: a.sun + (b.sun - a.sun) * t,
            rain: a.rain + (b.rain - a.rain) * t,
        };
        let hour = hour.rem_euclid(24.0);
        let day = (SUNRISE..SUNSET).contains(&hour);
        // Fraction of the sun's (or moon's) arc, rising in the east (+X),
        // passing to the south (+Z) and setting in the west.
        let arc = if day {
            (hour - SUNRISE) / (SUNSET - SUNRISE)
        } else {
            (hour - SUNSET).rem_euclid(24.0) / (24.0 - SUNSET + SUNRISE)
        };
        let angle = arc * std::f32::consts::PI;
        let sun = Vec3::new(angle.cos(), angle.sin() * 0.9 + 0.08, 0.45).normalize();
        // Soft fade around sunrise and sunset so shadows do not pop.
        let fade = |edge: f32| (1.0 - (hour - edge).abs() / 0.75).clamp(0.0, 1.0);
        let daylight = if day {
            1.0 - fade(SUNRISE).max(fade(SUNSET)) * 0.5
        } else {
            fade(SUNRISE).max(fade(SUNSET)) * 0.5
        };
        let ambient = (palette.ambient_objects + 8.0) / (noon.ambient_objects + 8.0);
        Lighting {
            sun,
            daylight,
            sun_color: if day {
                (palette.sun_core / 255.0).max(Vec3::splat(0.3)) * (0.55 + 0.45 * weather.sun)
            } else {
                Vec3::new(0.45, 0.55, 0.8)
            },
            ambient: ambient.clamp(Vec3::splat(0.22), Vec3::splat(1.25)),
            sky: (palette.sky_top * 0.35 + palette.sky_bottom * 0.65) / 255.0,
            fog: weather.fog,
            shadow: weather.sun,
            rain: weather.rain,
        }
    }
}

/// Reads every complete weather block. Supports rows with and without the
/// directional colour column found in earlier games' files.
fn parse(text: &str) -> anyhow::Result<Vec<[Palette; 8]>> {
    let mut rows = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        let values: Vec<f32> = line
            .split_whitespace()
            .map_while(|v| v.parse().ok())
            .collect();
        // Headers and stray text lines are skipped; colour rows have 40+ values.
        if values.len() < 40 {
            continue;
        }
        let rgb = |group: usize| {
            Vec3::new(
                values[group * 3],
                values[group * 3 + 1],
                values[group * 3 + 2],
            )
        };
        // San Andreas rows hold 52 values; a directional colour adds three.
        let skip = usize::from(values.len() >= 55);
        rows.push(Palette {
            ambient: rgb(0),
            ambient_objects: rgb(1),
            sky_top: rgb(2 + skip),
            sky_bottom: rgb(3 + skip),
            sun_core: rgb(4 + skip),
        });
    }
    anyhow::ensure!(rows.len() >= 8, "only {} timecyc rows", rows.len());
    for row in &rows {
        for colour in [
            row.ambient,
            row.ambient_objects,
            row.sky_top,
            row.sky_bottom,
            row.sun_core,
        ] {
            anyhow::ensure!(
                colour.min_element() >= 0.0 && colour.max_element() <= 255.0,
                "timecyc colour out of range"
            );
        }
    }
    Ok(rows.as_chunks::<8>().0.to_vec())
}

/// Interpolate a weather's eight rows at an hour, wrapping past midnight.
fn sample(slots: &[Palette; 8], hour: f32) -> Palette {
    let hour = hour.rem_euclid(24.0);
    let next = HOURS.iter().position(|h| *h > hour).unwrap_or(0);
    let previous = (next + HOURS.len() - 1) % HOURS.len();
    let (start, mut end) = (HOURS[previous], HOURS[next]);
    if end <= start {
        end += 24.0;
    }
    let hour = if hour < start { hour + 24.0 } else { hour };
    slots[previous].lerp(slots[next], (hour - start) / (end - start))
}

/// Approximate clear-weather colours used without a readable timecyc.dat.
fn fallback() -> [Palette; 8] {
    let p = |ambient: [f32; 3], sky_top: [f32; 3], sky_bottom: [f32; 3], sun: [f32; 3]| Palette {
        ambient: Vec3::from(ambient),
        ambient_objects: Vec3::from(ambient),
        sky_top: Vec3::from(sky_top),
        sky_bottom: Vec3::from(sky_bottom),
        sun_core: Vec3::from(sun),
    };
    [
        p(
            [25.0, 28.0, 40.0],
            [8.0, 12.0, 30.0],
            [20.0, 28.0, 50.0],
            [120.0, 140.0, 200.0],
        ),
        p(
            [30.0, 30.0, 40.0],
            [20.0, 25.0, 45.0],
            [45.0, 45.0, 60.0],
            [150.0, 150.0, 180.0],
        ),
        p(
            [60.0, 50.0, 45.0],
            [70.0, 90.0, 130.0],
            [200.0, 140.0, 90.0],
            [255.0, 170.0, 100.0],
        ),
        p(
            [80.0, 75.0, 70.0],
            [90.0, 130.0, 180.0],
            [190.0, 170.0, 150.0],
            [255.0, 210.0, 160.0],
        ),
        p(
            [100.0, 100.0, 100.0],
            [92.0, 140.0, 184.0],
            [150.0, 180.0, 205.0],
            [255.0, 245.0, 225.0],
        ),
        p(
            [95.0, 85.0, 75.0],
            [90.0, 120.0, 170.0],
            [220.0, 160.0, 110.0],
            [255.0, 190.0, 120.0],
        ),
        p(
            [70.0, 55.0, 55.0],
            [60.0, 60.0, 110.0],
            [200.0, 110.0, 80.0],
            [255.0, 130.0, 80.0],
        ),
        p(
            [35.0, 35.0, 50.0],
            [15.0, 20.0, 45.0],
            [40.0, 40.0, 70.0],
            [130.0, 140.0, 200.0],
        ),
    ]
}

/// A world's own weather (Build Mode world settings or a gameplay action),
/// over whatever the cycle chose. Strengths follow the cycle's own kinds.
pub fn apply_weather(lighting: &mut Lighting, weather: sa_scene::world_logic::Weather, hour: f32) {
    use sa_scene::world_logic::Weather;
    let (fog, sun, rain) = match weather {
        Weather::Clear => (1.0, 1.0, 0.0),
        Weather::Cloudy => (1.6, 0.45, 0.0),
        Weather::Rain => (3.0, 0.15, 1.0),
        Weather::Fog => (8.0, 0.3, 0.0),
    };
    // By day the sun's colour carries the weather's dimming: undo the
    // cycle's and apply this one's. The moon is not dimmed.
    if (SUNRISE..SUNSET).contains(&hour.rem_euclid(24.0)) {
        let base = 0.55 + 0.45 * lighting.shadow.clamp(0.0, 1.0);
        lighting.sun_color *= (0.55 + 0.45 * sun) / base;
    }
    lighting.fog = fog;
    lighting.shadow = sun;
    lighting.rain = rain;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn noon_is_bright_and_midnight_is_dark() {
        let cycle = DayCycle::default();
        let noon = cycle.lighting(12.0);
        let night = cycle.lighting(0.0);
        assert_eq!(noon.daylight, 1.0);
        assert_eq!(night.daylight, 0.0);
        assert!((noon.ambient - Vec3::ONE).abs().max_element() < 1e-4);
        assert!(night.ambient.max_element() < 0.5);
        assert!(noon.sun.y > 0.8, "noon sun should be high: {:?}", noon.sun);
        assert!(
            cycle.lighting(7.0).sun.x > 0.5,
            "morning sun rises in the east"
        );
        assert!(
            cycle.lighting(19.0).sun.x < -0.5,
            "evening sun sets in the west"
        );
    }
    #[test]
    fn palette_wraps_past_midnight() {
        let cycle = DayCycle::default();
        let late = cycle.palette(23.0);
        let slots = fallback();
        assert_eq!(late, slots[7].lerp(slots[0], 0.5));
        assert_eq!(cycle.palette(12.0), slots[4]);
    }
    #[test]
    fn parses_san_andreas_rows() {
        let mut text = String::from("// EXTRASUNNY_LA\n");
        for hour in 0..8 {
            let mut row = vec![hour.to_string(); 6];
            row.extend((0..46).map(|_| "100".to_string()));
            text.push_str(&row.join(" "));
            text.push('\n');
        }
        let blocks = parse(&text).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0][3].ambient, Vec3::splat(3.0));
        assert_eq!(blocks[0][3].sky_top, Vec3::splat(100.0));
        assert!(parse("1 2 3").is_err());
    }
    fn grey_block() -> [Palette; 8] {
        fallback().map(|p| Palette {
            sky_top: Vec3::splat(128.0),
            sky_bottom: Vec3::splat(128.0),
            ..p
        })
    }
    #[test]
    fn weather_blends_colours_fog_and_rain() {
        let mut blocks = vec![fallback(); 9];
        blocks[8] = grey_block();
        let mut cycle = DayCycle {
            blocks,
            ..Default::default()
        };
        let clear = cycle.lighting(12.0);
        assert_eq!((clear.fog, clear.shadow, clear.rain), (1.0, 1.0, 0.0));
        cycle.change_to(8);
        let start = cycle.lighting(12.0);
        assert_eq!(start.sky, clear.sky, "a change starts from the old weather");
        cycle.advance(WEATHER_BLEND * 0.5);
        let half = cycle.lighting(12.0);
        assert!(half.rain > 0.4 && half.rain < 0.6);
        cycle.advance(WEATHER_BLEND);
        let rain = cycle.lighting(12.0);
        assert_eq!(rain.rain, 1.0);
        assert!(rain.fog > 2.0 && rain.shadow < 0.5);
        assert!((rain.sky - Vec3::splat(128.0 / 255.0)).abs().max_element() < 1e-4);
        assert_eq!(cycle.current_weather(), 8);
    }
    #[test]
    fn fixed_weather_overrides_the_cycle() {
        let cycle = DayCycle {
            fixed_weather: Some(9),
            ..Default::default()
        };
        let fog = cycle.lighting(12.0);
        assert!(fog.fog > 5.0);
        assert_eq!(cycle.current_weather(), 9);
        // Missing blocks fall back to the first weather's colours.
        assert_eq!(fog.sky, DayCycle::default().lighting(12.0).sky);
    }
    #[test]
    fn weather_changes_within_the_area_list() {
        let mut cycle = DayCycle::default();
        cycle.set_position(-2000.0, 500.0);
        assert_eq!(cycle.region, Region::SanFierro);
        for _ in 0..20 {
            cycle.advance(WEATHER_PERIOD);
            assert!(choices(Region::SanFierro).contains(&cycle.weather.1));
        }
        assert_eq!(region_at(2000.0, -1500.0), Region::LosSantos);
        assert_eq!(region_at(2000.0, 1500.0), Region::LasVenturas);
        assert_eq!(region_at(0.0, 1500.0), Region::Desert);
        assert_eq!(region_at(-1000.0, -2000.0), Region::Countryside);
    }
    #[test]
    fn clock_advances_and_wraps() {
        let mut cycle = DayCycle {
            minutes: 23.0 * 60.0 + 59.0,
            ..Default::default()
        };
        cycle.advance(2.0);
        assert_eq!(cycle.clock(), (0, 1));
    }

    #[test]
    fn a_world_weather_overrides_the_cycle() {
        use sa_scene::world_logic::Weather;
        let cycle = DayCycle::default();
        let clear = cycle.lighting(12.0);
        let mut rain = clear;
        apply_weather(&mut rain, Weather::Rain, 12.0);
        assert_eq!((rain.rain, rain.fog), (1.0, 3.0));
        assert!(rain.sun_color.x < clear.sun_color.x, "rain dims the sun");
        let mut back = rain;
        apply_weather(&mut back, Weather::Clear, 12.0);
        assert!((back.sun_color - clear.sun_color).length() < 1e-4);
        assert_eq!((back.rain, back.shadow), (0.0, 1.0));
        // The moon keeps its colour, also at the exact moment of sunset.
        let moon = cycle.lighting(SUNSET);
        let mut wet = moon;
        apply_weather(&mut wet, Weather::Rain, SUNSET);
        assert_eq!(wet.sun_color, moon.sun_color);
    }
}
