//! Static world destinations shared by streaming and interface previews.
pub const ORIGIN: [f32; 2] = [2500.0, -1670.0];
pub const RADIUS: f32 = 400.0;
pub const DESTINATIONS: [(&str, [f32; 2]); 9] = [
    ("Grove Street", ORIGIN),
    ("Downtown Los Santos", [1480.0, -1730.0]),
    ("Santa Maria Beach", [350.0, -1800.0]),
    ("Los Santos Airport", [1700.0, -2450.0]),
    ("Countryside", [200.0, -500.0]),
    ("San Fierro", [-2000.0, 300.0]),
    ("Las Venturas", [2000.0, 1500.0]),
    ("Desert", [-500.0, 1900.0]),
    ("Mount Chiliad", [-2300.0, -1600.0]),
];
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
    pub center: [f32; 2],
    pub interior: u8,
}
#[derive(Clone, Copy)]
pub struct Interior {
    pub name: &'static str,
    pub id: u8,
    pub position: [f32; 3],
}
pub const INTERIORS: [Interior; 3] = [
    Interior {
        name: "CJ's House",
        id: 3,
        position: [2496.05, -1692.93, 1013.75],
    },
    Interior {
        name: "Sweet's House",
        id: 1,
        position: [2526.46, -1679.09, 1014.5],
    },
    Interior {
        name: "Madd Dogg's Mansion",
        id: 5,
        position: [1263.08, -785.309, 1090.96],
    },
];
