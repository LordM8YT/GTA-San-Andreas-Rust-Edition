# sare_addon_vehicles

An example of addon vehicles: your own glTF cars, trucks and vans on a SARE
server, FiveM style. Copy this folder, put your `.glb` files in `vehicles/`
and list them in `resource.json`. Each entry uses the same fields as a
content pack's `vehicles` (see `docs/content-packs.md`, "Vehicles").

Add `ensure sare_addon_vehicles` to `server.cfg` (and `ensure sare_vehicles`
for the `/car` command). Players download the models when they join, then:

```
/car example_truck
```

Scripts spawn it the same way: `CreateVehicle("example_truck", x, y, z, heading, true, true)`.

The example model is the CC0 tow truck from the SARE RP Town pack.
