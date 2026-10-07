# Native mods

For native freeroam, se [native-mods](../docs/native-mods.md).
Den støtter nå også custom DFF/TXD/COL. Dokumentasjonen under gjelder den
eldre Python/WebGL-testen.

# Mods i testversjonen

Lag en mappe under `mods/` med `mod.json`. Dette er data-mods: ingen scripts
eller DLL-er kjøres. Den medfølgende Camera demo viser formatet.
Mods lastes i alfabetisk mapperekkefølge når runtime starter. Senere mods
vinner ved samme innstilling eller teksturnøkkel. Start runtime på nytt etter
endringer; `--no-mods` deaktiverer alle mods.

Støttet nå:

- `settings`: `camera_speed`, `sky_color` (tre tall 0–1), `fog_distance`.
- `texture_overrides`: `"dictionary:texture": "textures/my.png"`.
  Dictionary-navnet skrives uten `.txd`. Se teksturlisten i Mods-menyen for
  tilgjengelige nøkler. PNG må ligge inne i denne mod-mappen, maks 4096×4096
  og 16 MiB. Potens-av-to-størrelser gir korrekt repeat på WebGL 1.
- `exclude_model_ids`: skjul valgte modeller i denne scenen.
- `placements`: legg til originalmodeller fra installasjonen på egne posisjoner
  i testområdet. Maks 100 per mod, 200 samlet. Bruk `model_id`,
  `position: [GTA_X, GTA_Y, GTA_Z]`, valgfri `rotation: [X,Y,Z,W]` i IPL-konvensjon.
  Dette er visuelle objekter; kollisjon er ikke implementert.

Eksempel på plassering, som kan legges i `placements`:

```json
{ "model_id": 1347, "position": [2490, -1665, 12.5], "rotation": [0, 0, 0, 1] }
```

Tilgjengelige originalmodeller avhenger av IDE-filer i din installasjon.
Teksturoverstyringer for nøkler som ikke brukes av scenen vises ikke.
Del egne manifest-filer og egne originale teksturer. Rockstar-filer eller
konverterte/upskalerte versjoner av dem må ikke legges i prosjektleveranser;
generer slike filer lokalt fra brukerens egen installasjon.
Custom mesh-import, scripting, mod-API for gamecode og multiplayer er ikke
implementert i denne testversjonen.
