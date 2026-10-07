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

Metadata is bounded to 64 packages and 4,096 banks. A requested bank is
limited to 16 MiB of PCM; indexing a larger, unused speech bank does not
prevent other banks loading. Sample loops and headroom are decoded metadata,
but automatic engine loops, surface-dependent footsteps and original radio
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

Format and sound identifiers were checked against primary reverse-engineered
structures, rather than inferred from file extensions:

- [Bank/package structures](https://github.com/gta-reversed/gta-reversed/blob/master/source/game_sa/Audio/Loaders/AEBankLoader.h)
- [PCM bounds, rates and loop units](https://github.com/gta-reversed/gta-reversed/blob/master/source/game_sa/Audio/Loaders/AEMP3BankLoader.cpp)
- [Frontend sound pairs](https://github.com/gta-reversed/gta-reversed/blob/master/source/game_sa/Audio/Entities/AEFrontendAudioEntity.cpp)
- [Sound-bank identifiers](https://github.com/gta-reversed/gta-reversed/blob/master/source/game_sa/Audio/Enums/eSoundBank.h)
