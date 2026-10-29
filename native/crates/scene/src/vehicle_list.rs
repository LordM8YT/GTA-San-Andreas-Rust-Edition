//! The four-wheeled cars listed in the player's own `data/vehicles.ide`, with
//! their first body colours from `data/carcols.dat`. Only names are read up
//! front; meshes load when a car is first chosen.
use std::{collections::HashMap, path::Path};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OriginalCar {
    /// Model ID, the first field of the IDE row.
    pub id: i32,
    /// File name in gta3.img, e.g. `infernus`.
    pub model: String,
    /// GXT key of the in-game name, e.g. `INFERNU`.
    pub name_key: String,
    /// The game's own grouping, e.g. `poorfamily` or `executive`.
    pub class: String,
}

impl OriginalCar {
    /// Ordinary cars the game itself sends through city traffic.
    pub fn is_civilian(&self) -> bool {
        matches!(
            self.class.as_str(),
            "poorfamily" | "richfamily" | "executive" | "worker" | "taxi" | "big"
        )
    }
    /// The four-wheeled patrol cars the game sends after a wanted player.
    pub fn is_police(&self) -> bool {
        POLICE_CARS.contains(&self.model.as_str())
    }
}

/// Patrol cars of Los Santos, San Fierro and Las Venturas, and the
/// countryside sheriff's Ranger.
pub const POLICE_CARS: [&str; 4] = ["copcarla", "copcarsf", "copcarvg", "copcarru"];

/// Cars a player can spawn and drive with the four-wheel model. Bikes, boats,
/// aircraft, trains and trailers need their own physics and are left out, as
/// are entries the game itself marks `ignore` (RC toys and the like), except
/// the police cars, which only the police system spawns.
pub fn original_cars(game: &Path) -> Vec<OriginalCar> {
    sa_assets::game_path::resolve(game, "data/vehicles.ide")
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|text| parse_cars(&text))
        .unwrap_or_default()
}

fn parse_cars(ide: &str) -> Vec<OriginalCar> {
    let mut cars = Vec::new();
    let mut in_cars = false;
    for line in ide.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        match line {
            "cars" => {
                in_cars = true;
                continue;
            }
            "end" => {
                in_cars = false;
                continue;
            }
            _ => {}
        }
        if !in_cars {
            continue;
        }
        let fields: Vec<_> = line.split(',').map(str::trim).collect();
        if fields.len() < 8 || fields[3] != "car" {
            continue;
        }
        let model = fields[1].to_ascii_lowercase();
        if fields[7].eq_ignore_ascii_case("ignore") && !POLICE_CARS.contains(&model.as_str()) {
            continue;
        }
        if model.is_empty()
            || !model.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            || cars.iter().any(|c: &OriginalCar| c.model == model)
        {
            continue;
        }
        let Ok(id) = fields[0].parse() else { continue };
        cars.push(OriginalCar {
            id,
            model,
            name_key: fields[5].to_ascii_uppercase(),
            class: fields[7].to_ascii_lowercase(),
        });
    }
    cars
}

/// First primary and secondary body colour per model, as RGB.
pub fn body_colours(game: &Path) -> HashMap<String, [[u8; 3]; 2]> {
    sa_assets::game_path::resolve(game, "data/carcols.dat")
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|text| parse_carcols(&text))
        .unwrap_or_default()
}

fn parse_carcols(text: &str) -> HashMap<String, [[u8; 3]; 2]> {
    let mut palette: Vec<[u8; 3]> = Vec::new();
    let mut colours = HashMap::new();
    let mut section = "";
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if matches!(line, "col" | "car" | "car4") {
            section = if line == "col" { "col" } else { "car" };
            continue;
        }
        if line == "end" {
            section = "";
            continue;
        }
        // Palette rows are "r,g,b" and sometimes "r g b" or "r.g.b".
        match section {
            "col" => {
                let parts: Vec<&str> = line
                    .split([',', ' ', '\t', '.'])
                    .filter(|v| !v.is_empty())
                    .collect();
                if let [r, g, b, ..] = parts[..] {
                    if let (Ok(r), Ok(g), Ok(b)) = (r.parse(), g.parse(), b.parse()) {
                        palette.push([r, g, b]);
                    }
                }
            }
            "car" => {
                let fields: Vec<_> = line.split(',').map(str::trim).collect();
                let (Some(model), Some(primary), Some(secondary)) =
                    (fields.first(), fields.get(1), fields.get(2))
                else {
                    continue;
                };
                let lookup = |s: &str| {
                    s.parse::<usize>()
                        .ok()
                        .and_then(|i| palette.get(i))
                        .copied()
                };
                if let (Some(primary), Some(secondary)) = (lookup(primary), lookup(secondary)) {
                    colours
                        .entry(model.to_ascii_lowercase())
                        .or_insert([primary, secondary]);
                }
            }
            _ => {}
        }
    }
    colours
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_drivable_cars_only() {
        let ide = "# vehicles\ncars\n\
            400, landstal, landstal, car, LANDSTAL, LANDSTK, null, richfamily, 10, 0, 0, -1, 0.768, 0.768, -1\n\
            411, infernus, infernus, car, INFERNUS, INFERNU, null, executive, 10, 0, 0, -1, 0.7, 0.7, -1\n\
            441, rcbandit, rcbandit, car, RCBANDIT, RCBANDT, null, ignore, 1, 0, 0, -1, 0.5, 0.5, -1\n\
            448, pizzaboy, pizzaboy, bike, PIZZABOY, PIZZABO, bikev, motorbike, 1, 0, 0, 16, 0.46, 0.46\n\
            435, artict1, artict1, trailer, ARTICT1, ARTICT1, null, ignore, 1, 0, 0, -1, 1.1, 1.1, -1\n\
            411, Infernus, infernus, car, INFERNUS, INFERNU, null, executive, 10, 0, 0, -1, 0.7, 0.7, -1\n\
            596, copcarla, copcarla, car, POLICAR, POLICAR, null, ignore, 1, 0, 0, -1, 0.7, 0.7, -1\n\
            end\n";
        let cars = parse_cars(ide);
        assert_eq!(
            cars.iter().map(|c| c.model.as_str()).collect::<Vec<_>>(),
            ["landstal", "infernus", "copcarla"]
        );
        assert_eq!(cars[1].name_key, "INFERNU");
        assert!(cars[0].is_civilian() && cars[1].is_civilian());
        assert!(cars[2].is_police() && !cars[2].is_civilian());
    }

    #[test]
    fn reads_first_colour_pair_through_the_palette() {
        let text = "col\n0,0,0 # black\n245,245,245\n42,119,161 # blue\nend\n\
            car\nlandstal, 2,1, 1,0\ninfernus, 1,9\nend\ncar4\ntaxi, 1,0,1,0\nend\n";
        let colours = parse_carcols(text);
        assert_eq!(colours["landstal"], [[42, 119, 161], [245, 245, 245]]);
        assert!(
            !colours.contains_key("infernus"),
            "index out of range is skipped"
        );
        assert_eq!(colours["taxi"], [[245, 245, 245], [0, 0, 0]]);
    }

    #[test]
    fn missing_files_give_empty_lists() {
        let empty = Path::new("/definitely/not/a/game");
        assert!(original_cars(empty).is_empty());
        assert!(body_colours(empty).is_empty());
    }
}
