//! Players driving server vehicles (`CreateVehicle`). A player asks to get
//! in with [`ENTER`]; the server checks the vehicle is near, free and in
//! their routing bucket, lets resources refuse through the cancellable
//! `sare:enteringVehicle` event, and answers with [`GRANTED`]. While driven,
//! the vehicle is the driver's own synced car: other players see it through
//! the driver's pose, and the server entity follows that pose. On exit
//! ([`EXIT`]), disconnect or when the driver stops driving, the entity is
//! left where the car stopped and everyone gets it again.
use crate::natives::{send_client, world_position};
use crate::script::{self, Shared};
use sa_net::entities::{EntityKind, ModelRef};
use serde_json::Value as Json;
use std::time::{Duration, Instant};

/// Client -> server: `[handle]`, the player wants to drive this vehicle.
pub const ENTER: &str = "__sare:enterVehicle";
/// Client -> server: `[handle]`, the player got out.
pub const EXIT: &str = "__sare:exitVehicle";
/// Server -> client: `[handle, model, x, y, z, heading, colour or null]`.
pub const GRANTED: &str = "__sare:vehicleGranted";
/// Server -> client: `[handle, reason]`.
pub const REFUSED: &str = "__sare:vehicleRefused";
/// Server -> client: `[handle]`, get out now (deleted or `TaskLeaveVehicle`).
pub const REVOKED: &str = "__sare:vehicleRevoked";
/// How close a player must be to get in, in metres.
pub const ENTER_RANGE: f32 = 7.0;
/// How long a driver may be out of the car before the server takes it back.
const GRACE: Duration = Duration::from_secs(3);

/// Who drives what: player id -> (handle, last time seen driving).
pub type Drivers = std::collections::BTreeMap<u32, (u32, Instant)>;

fn handle_arg(args: &[Json]) -> Option<u32> {
    args.first()
        .and_then(Json::as_f64)
        .filter(|v| v.is_finite() && *v >= 0.0 && *v <= u32::MAX as f64)
        .map(|v| v as u32)
}

/// Handle a vehicle request from a client; false for other events.
pub fn handle_event(shared: &Shared, source: u32, name: &str, args: &[Json]) -> bool {
    match name {
        ENTER => {
            if let Some(handle) = handle_arg(args) {
                if let Err(reason) = enter(shared, source, handle, true) {
                    send_client(
                        shared,
                        Some(source),
                        REFUSED,
                        &[handle.into(), reason.into()],
                    );
                }
            }
            true
        }
        EXIT => {
            let handle = handle_arg(args);
            let current = shared.borrow().drivers.get(&source).map(|d| d.0);
            if current.is_some() && current == handle {
                release(shared, source);
            }
            true
        }
        _ => false,
    }
}

fn model_json(model: &ModelRef) -> Json {
    match model {
        ModelRef::Name(name) => Json::from(name.as_str()),
        ModelRef::Hash(hash) => Json::from(*hash),
    }
}

/// Seat `player` in vehicle `handle`. `checked` applies the distance check
/// and the `sare:enteringVehicle` event (scripts warping a player skip them).
pub fn enter(shared: &Shared, player: u32, handle: u32, checked: bool) -> Result<(), &'static str> {
    let session = shared.borrow().session.clone();
    let bucket = session.as_ref().map_or(0, |s| s.routing_bucket(player));
    let (entity, pose) = {
        let state = shared.borrow();
        let entity = state
            .entities
            .get(handle)
            .cloned()
            .ok_or("That vehicle is gone.")?;
        let pose = state
            .peers
            .iter()
            .find(|p| p.id == player)
            .map(|p| p.pose)
            .ok_or("Not connected.")?;
        (entity, pose)
    };
    if entity.state.kind != EntityKind::Vehicle || entity.bucket != bucket {
        return Err("That vehicle is not here.");
    }
    if entity.driver.is_some_and(|d| d != player) {
        return Err("Someone is driving it.");
    }
    if checked {
        let at = world_position(&pose);
        let gap: f32 = at
            .iter()
            .zip(entity.state.position)
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f32>()
            .sqrt();
        if gap > ENTER_RANGE {
            return Err("Too far away.");
        }
        if script::dispatch(
            shared,
            "sare:enteringVehicle",
            player,
            &[handle.into()],
            false,
        ) {
            return Err("You cannot use this vehicle.");
        }
        // A handler may have deleted or taken it meanwhile.
        if shared
            .borrow()
            .entities
            .get(handle)
            .is_none_or(|e| e.driver.is_some_and(|d| d != player))
        {
            return Err("That vehicle is gone.");
        }
    }
    if shared
        .borrow()
        .drivers
        .get(&player)
        .is_some_and(|d| d.0 != handle)
    {
        release(shared, player);
    }
    {
        let mut state = shared.borrow_mut();
        state.entities.set_driver(handle, Some(player));
        state.drivers.insert(player, (handle, Instant::now()));
    }
    let [x, y, z] = entity.state.position;
    let color = entity
        .state
        .color
        .map_or(Json::Null, |c| Json::from(c.to_vec()));
    send_client(
        shared,
        Some(player),
        GRANTED,
        &[
            handle.into(),
            model_json(&entity.state.model),
            x.into(),
            y.into(),
            z.into(),
            entity.state.heading.into(),
            color,
        ],
    );
    script::dispatch(
        shared,
        "sare:vehicleEntered",
        player,
        &[handle.into()],
        false,
    );
    Ok(())
}

/// Take `player` out of their vehicle and give it back to everyone where
/// it stopped.
pub fn release(shared: &Shared, player: u32) {
    let Some((handle, _)) = shared.borrow_mut().drivers.remove(&player) else {
        return;
    };
    let released = shared.borrow_mut().entities.set_driver(handle, None);
    if released {
        script::dispatch(
            shared,
            "sare:vehicleExited",
            player,
            &[handle.into()],
            false,
        );
    }
}

/// Tell the driver of `handle`, if any, to get out (before deleting it).
pub fn revoke(shared: &Shared, handle: u32) {
    let driver = shared
        .borrow()
        .drivers
        .iter()
        .find(|(_, d)| d.0 == handle)
        .map(|(p, _)| *p);
    if let Some(player) = driver {
        send_client(shared, Some(player), REVOKED, &[handle.into()]);
        release(shared, player);
    }
}

/// Every frame: driven vehicles follow their driver's car; drivers who left
/// or stopped driving give their vehicle back.
pub fn track(shared: &Shared) {
    let drivers: Vec<(u32, (u32, Instant))> = shared
        .borrow()
        .drivers
        .iter()
        .map(|(p, d)| (*p, *d))
        .collect();
    for (player, (handle, seen)) in drivers {
        let pose = shared
            .borrow()
            .peers
            .iter()
            .find(|p| p.id == player)
            .map(|p| p.pose);
        let gone = shared.borrow().entities.get(handle).is_none();
        match pose {
            _ if gone => {
                shared.borrow_mut().drivers.remove(&player);
            }
            Some(pose) if pose.driving => {
                let car = pose.vehicle.map_or(
                    (world_position(&pose), crate::natives::heading(&pose)),
                    |v| {
                        let as_pose = sa_net::Pose {
                            position: v.position,
                            yaw: v.yaw,
                            ..Default::default()
                        };
                        (world_position(&as_pose), crate::natives::heading(&as_pose))
                    },
                );
                let mut state = shared.borrow_mut();
                state.entities.follow(handle, car.0, car.1);
                state.drivers.insert(player, (handle, Instant::now()));
            }
            Some(_) if seen.elapsed() < GRACE => {}
            _ => release(shared, player),
        }
    }
}
