# Fase 1 — beslutninger og formatkart

## Faktisk status

Brukeren oppga `E:\GTA San Andreas\Grand Theft Auto San Andreas`.
Den stien er ikke montert i utviklingsøkten. En lokalt generert rapport er nå
mottatt og analysert; konkrete funn står i `installation-findings.md`.
Oversikten under er generelt arbeidsgrunnlag for klassisk PC-utgave.
Originale asset-bytes og rendering er ikke verifisert ennå.

| Data | Vanlig plassering / format | Rolle og første prioritet |
|---|---|---|
| Lastemanifest | data/gta.dat, data/default.dat | IMG-, IDE-, IPL- og andre referanser; rekkefølge må bevares |
| Asset-arkiver | models/*.img; IMG VER2 | Indeks med sektorer; les entries ved behov, ikke masseekstrahering |
| Modeller | *.dff i arkiver / løse filer | RenderWare chunks, frame-hierarki, atomics, geometri og materialer |
| Textures | *.txd i arkiver / løse filer | RenderWare texture dictionaries; PC-rasterformat og materialnavn må dekodes |
| Objektdefinisjoner | data/maps/**/*.ide | Model-ID til DFF/TXD-navn, draw distance, flags; TXD-arv krever senere støtte |
| Plasseringer | *.ipl, også i IMG | Tekst og binære streams; posisjon, quaternion, interiør og LOD-referanser |
| Kollisjon | *.col | Senere fase etter renderer |
| Anim og scripts | *.ifp, *.scm | Utsettes; ingen missions eller animert spiller i første PoC |

## Minste bevis

1. Kartlegg faktisk installasjon med dette verktøyet.
2. Velg én statisk modell; bil utsettes til frame-hierarki og submeshes er håndtert.
3. Implementer Rust-lesere for IMG og relevante DFF-chunks med grensekontroller.
4. Vis original geometri uten å kjøre eller laste kode fra gta_sa.exe.
5. Dekod modellens TXD, slå opp materialtextures og vis den teksturert.
6. Koble IDE til IPL og velg et lite område ved Grove Street.
7. Implementer debug-kamera, WASD, mouse-look og justerbar hastighet.

Et enkelt mesheksperiment kan starte uten textures, men teksturert originalmodell
er den første fullstendige modellmilepælen. Ingen løfter om full spillkompatibilitet
eller høy FPS før disse er målt i faktisk kjøring.

## Viktige avklaringer i loaderen

- IMG: little-endian VER2-header og directory; 2048-byte sektorer. Behold begge
  16-bit størrelsesfelter, og avvis ukjente varianter ved ekstraksjon.
- DFF: les chunk-grenser, valider indeksene og håndter frame transforms per atomic.
  En chunk-reader alene er ikke en mesh-loader. Native geometri og plugins må
  støttes eksplisitt eller gi en tydelig unsupported-feil.
- TXD: undersøk platform ID, rasterformat, compression, alpha og mipmaps før
  decoder velges. Manglende materialtextures rapporteres med navn.
- Kart: les tekst-IPL og binære IPL-streams; bevar LOD-lenker og filtrer interiører.
  Bruk én dokumentert konvertering fra GTA-koordinater/quaternions til rendereren.
- Cache: utenfor spillinstallasjonen, med innholdshash og converter-versjon som
  nøkkel. Ingen originale eller konverterte bruker-assets i prosjektarkiver.
- Distribusjon: kode og egne syntetiske testdata; brukeren angir lokal installasjon
  ved oppstart. Runtime krever lokale assets, men filtilstedeværelse er ikke en
  juridisk eierskapskontroll.

## Kildereferanser for videre implementering

Disse er primærkilder fra utviklere av formatverktøy. Ingen kode er kopiert inn
fra dem. Lisenser må vurderes før eventuell kodegjenbruk.

- DragonFF: https://github.com/Parik27/DragonFF
- RenderWare DFF-implementasjon: https://github.com/Parik27/DragonFF/blob/master/gtaLib/dff.py
- Map-implementasjon: https://github.com/Parik27/DragonFF/blob/master/gtaLib/map.py
- IMG-verktøy: https://github.com/Divinakra140/SAFT_San_Andreas_File_Tool

## Langsiktig scope

Renderer/runtime i Rust der det er praktisk, moderne aspect ratios, lys/skygger,
draw distance, vann og post-processing. Classic/Enhanced-modus og lokal texture-
oppskalering etterpå. Kollisjon, spiller, biler, fysikk, AI, trafikk, scripting
og missions er egne større milepæler, ikke del av dette inspeksjonsverktøyet.
