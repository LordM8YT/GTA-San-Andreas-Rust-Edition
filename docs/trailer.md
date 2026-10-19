# Trailer camera

SARE can fly a scripted camera through ten shots across the map for a trailer.
Each shot sets its own time of day and weather from your `timecyc.dat`, hides
the HUD and turns on temporal anti-aliasing and enhanced lighting. Your saved
settings are not changed.

## Record clips

Install [ffmpeg](https://ffmpeg.org/) once (Windows: `winget install ffmpeg`),
then run this from the extracted client folder:

```
sa-runtime.exe --trailer-record trailer
```

Add `--game-dir <folder>` if SARE does not find your San Andreas installation.
SARE renders every shot at exactly 60 FPS with the Ultra preset into
`trailer/01-grove_street_morning.mp4`, `trailer/02-downtown_sunset.mp4` and so
on, then quits. Recording is slower than real time and the window shows no
picture while it records; progress is printed in the console. The clips have the
window's size, so use a 1920×1080 window or fullscreen and do not resize it.

`--trailer` (without `-record`) plays the same shots in real time instead, so
you can record with OBS or Xbox Game Bar.

## Change the shots

`--trailer-shots shots.txt` replaces the built-in list. Each line is one shot:

```
name  center_x center_y  hour weather seconds fov  camera_from  camera_to  look_from  look_to
```

The four points are map `x y` plus a height above the ground (or above sea level
over water). The camera is lifted over roofs and hills between its two ends.
`center_x center_y` is the neighbourhood that is streamed in; keep every point
within 400 m of it. Weather is the index in Graphics → Weather, counted from 0.
The built-in list is `SHOTS` in `native/crates/runtime/src/trailer.rs`.

The built-in shots were placed from map coordinates without in-game review. Some
may need adjusting after a first recording.
