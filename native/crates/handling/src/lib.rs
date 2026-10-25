//! GTA:SA automobile physics.
//!
//! The original game's car model, ported to Rust as a pure kernel: no engine
//! types, no rendering, no world ownership. It takes a handling record, a set
//! of wheel and body contacts, and one tick of controls, and advances the body.
//!
//! Structure, in the order it runs (see [`vehicle::Vehicle::tick`]):
//!
//! 1. `vehicle::Vehicle::tick` - the `CAutomobile::ProcessControl` stack
//! 2. `handling::Handling` - `tHandlingData` from `handling.cfg`
//! 3. `transmission::Transmission` - `cTransmission`, gear table and drive force
//! 4. `suspension::apply_suspension` - `CAutomobile::ProcessSuspension`
//! 5. `wheel::process_wheel` - `CVehicle::ProcessWheel`
//! 6. `physical::Body` - `CPhysical`: forces, effective mass, friction, integration
//! 7. `math` - `CMatrix` basis handling and GTA's exact re-orthonormalisation
//!
//! Units are GTA's, not SI: velocity is metres per CTimer step, where a step is
//! 1/50 s in the original and 1/100 s at this project's rate, and `ctimer_step`
//! (1.0 or 0.5) scales every per-step quantity. [`handling::Handling::default`]
//! is the stock Infernus, and the crate's tests exercise the assembled tick.
//!
//! Deliberately absent: collision generation (the caller supplies contacts),
//! damage, audio, rendering, and the bike/boat/aircraft paths.

pub mod contact;
pub mod handling;
pub mod math;
pub mod physical;
pub mod suspension;
pub mod transmission;
pub mod vehicle;
pub mod wheel;

pub use handling::Handling;
pub use math::{Basis, Vec3};
pub use physical::Body;
pub use transmission::Transmission;
pub use vehicle::{Controls, Vehicle, VehicleTickTrace};
pub use wheel::WheelState;
