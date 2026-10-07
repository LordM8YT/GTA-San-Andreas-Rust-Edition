# Vehicle dynamics

The native car now uses a lightweight tire-slip model inspired by the heavier
driving feel requested for this project. This is an original approximation,
not the GTA IV or GTA V physics engine.

- Horizontal momentum is independent of body heading, allowing sideways slip.
- Steering moves gradually and steering lock decreases at higher speeds.
- Body rotation has inertia and a lateral acceleration limit.
- Tire grip opposes sideways motion; the handbrake reduces grip and slows the car.
- Front/rear axle slip produces lateral forces and yaw torque. Braking and
  cornering share a limited tire-grip budget.
- Four ground probes use each wheel's expected height on the tilted body.
  Damped suspension follows road height and grade; body pitch and roll respond
  to slopes, acceleration and cornering.
- Opposite throttle brakes before engaging reverse. Engine acceleration decreases
  near top speed, with rolling resistance and quadratic aerodynamic drag.
- Airborne cars retain momentum but cannot accelerate or steer through tire grip.
- Physics integrates at intervals no greater than 1/120 second. Existing swept
  body collision, ground contact, safe spawning and exit checks remain active.

W / RT accelerates. S / LT brakes, then reverses. Space / LB is the handbrake.
The existing Vehicle handling setting adjusts tire grip, not engine power.
Exiting the car or reaching the streamed map boundary clears sideways momentum.

Suspension and body pitch/roll are approximations with fixed contact spacing,
not independent simulated wheel bodies. This model does not yet simulate wheel
articulation/animation, gear changes, deformation, different road surfaces or physical collision
impulses. Solid obstacles still stop the car rather than producing a crash response.

Automated checks cover braking and reverse, handbrake slip, frame-rate consistency,
airborne traction, wall clearance, spawning and custom car compatibility, plus
uphill/downhill support, banked roads, crests and falling beyond suspension travel.
The read-only `sa-scene` example `vehicle-slopes` also checks sloping original
map surfaces around Grove Street; it is not a whole-map collision audit.
