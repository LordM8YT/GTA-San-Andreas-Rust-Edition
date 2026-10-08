# Native audio

The `sa-audio` crate provides Kira-backed effect mixing, spatial emitters,
listener updates and file-backed music streaming. Runtime audio is optional:
an unavailable output device leaves the game running silently.

The original frontend menu sounds now load directly from the selected San
Andreas installation. Keyboard, mouse and Xbox menu navigation use the
original highlight/select cues; keyboard and Xbox back navigation use the
original back cue. F10 still plays the separate synthetic diagnostic tone.

`archive::SfxArchive` reads `audio/CONFIG/PakFiles.dat`, `BankLkup.dat` and
the corresponding `audio/SFX` packages. Package names and bank ranges are
validated. PCM is signed little-endian 16-bit mono, with the original sample
rate preserved. Loop offsets count samples. Frontend bank 60 stores paired
mono channels; these are combined into stereo without duplicating one side.
Original assets are read in memory and are neither edited nor bundled.

Freeroam now plays the original Taxi/Admiral (GENRL bank 86) and Infernus
(bank 37) engine loops. Slot 0 supplies revs and slot 1 supplies idle; loop
starts preserve their sample indices. Native speed and local throttle blend
the two samples and adjust pitch with short smoothing. This is a basic native
speed/load approximation, not a recreation of the original gearbox/RPM logic.
Other/custom cars use the sedan sound for now.

Occupied nearby multiplayer cars also emit engine sound from their interpolated
positions, including the car a passenger rides in. At most eight nearby cars
have two looping voices each; parked cars are silent. Leaving a car, losing its
peer, opening a menu or installing a session world releases its voices. Remote
throttle is not transmitted, so their pitch uses speed. There is no Doppler,
starter/gearshift/skid/crash sound, remote footsteps or NPC audio yet.

The local walking player uses five original generic/concrete footsteps from
bank 0, slots 1–5. Actual grounded movement accumulates step distance; standing
still, airborne movement, swimming and teleports do not trigger steps. Run and
walk use different stride distances. Surface materials and exact animation
contact events are not connected yet. Both footsteps and engines use spatial
children of the effects bus, so master/effects settings apply to them.

Metadata is bounded to 64 packages and 4,096 banks. A requested bank is
limited to 16 MiB of PCM; indexing a larger, unused speech bank does not
prevent other banks loading. Sample loops and headroom are decoded metadata,
but automatic headroom gain, surface-dependent footsteps and original radio
stream decoding are not connected yet. The generic music API accepts ordinary
supported audio files; it does not make original radio archives playable.

From `native`:

```powershell
./target/release/sa-runtime.exe --probe-audio
```

The local probe indexed 710 banks and decoded 690 sounds / 12,416,408 PCM
samples from banks 0–143. It also assembled the three frontend stereo cues.
This checks decoding, not whether a speaker is audible. Runtime startup
successfully initialized the local audio output during the GPU menu smoke.

`--smoke-car --smoke-audio` checks engine creation and restart after re-entry;
`--smoke-ped --smoke-audio` checks footstep playback during walking/running.
`tools/test-multiplayer.ps1 -Relay -Dedicated -Appearance -Passenger -Audio`
also checks gameplay emitter/step creation through session loading, model
changes, riding and offline restoration. A device-free mixer test captures
actual samples and checks that loops survive beyond their source length,
effects mute produces silence, unmute restores output and dropping the
emitter stops output. These checks do not replace listening on different
speakers/headphones or testing real network latency.

Format and sound identifiers were checked against primary reverse-engineered
structures, rather than inferred from file extensions:

- [Bank/package structures](https://github.com/gta-reversed/gta-reversed/blob/master/source/game_sa/Audio/Loaders/AEBankLoader.h)
- [PCM bounds, rates and loop units](https://github.com/gta-reversed/gta-reversed/blob/master/source/game_sa/Audio/Loaders/AEMP3BankLoader.cpp)
- [Frontend sound pairs](https://github.com/gta-reversed/gta-reversed/blob/master/source/game_sa/Audio/Entities/AEFrontendAudioEntity.cpp)
- [Sound-bank identifiers](https://github.com/gta-reversed/gta-reversed/blob/master/source/game_sa/Audio/Enums/eSoundBank.h)
- [Vehicle-to-bank identifiers](https://github.com/gta-reversed/gta-reversed/blob/master/source/game_sa/Audio/Entities/AEVehicleAudioEntity.VehicleAudioSettings.h)
- [Idle/rev sample identifiers](https://github.com/gta-reversed/gta-reversed/blob/master/source/game_sa/Audio/Entities/AEVehicleAudioEntity.cpp)
- [Generic footstep identifiers](https://github.com/gta-reversed/gta-reversed/blob/master/source/game_sa/Audio/Entities/AEPedAudioEntity.cpp)
