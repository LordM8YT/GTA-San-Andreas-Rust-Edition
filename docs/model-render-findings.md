# Første modell: faktiske asset-funn og validering

Kilde: brukerens private `sa-first-model-private.zip`.

- DFF-root: RenderWare clump, library ID `0x1803FFFF`.
- Én frame med rotparent, én atomic og én non-native geometri.
- 66 vertices, 60 trekanter, ett UV-sett og én morph target.
- Ett materiale, hvit materialfarge, texture-referanse `Miamibin`.
- TXD: sju D3D9 DXT1-textures. `Miamibin` er 128 × 128 med ett mipnivå.
- Både DFF og TXD ble dekodet fra de opplastede originale filene.

## Implementasjonen

`tools/load_assets.py` har bounded chunk-leser, en målrettet static DFF-loader,
D3D9 DXT1-decoder og PNG-koding. `tools/run_viewer.py` leser den valgte modellen
direkte fra brukerens lokale IMG, dekoder i minnet og serverer til WebGL-visningen.
Ingen original exe startes, ingen Rockstar-kode brukes, og ingen cache skrives
til spillmappen. Dette er et modellbevis, ikke en full GTA-runtime.

PoC-en bruker Python-standardbiblioteket og WebGL. Rust er fortsatt ønsket
for den videre native runtime; det finnes foreløpig ingen kompilert Rust-del.

## Validering og grenser

- Faktisk originalmodell og alle sju textures dekodet, materialnavnet løst.
- Lokalt HTTP-program startet; modell, texture-PNG, HTML og JS returnerte HTTP 200.
- JavaScript-syntaks kontrollert med Node.
- Bounded parsing og DXT1 testes med egne syntetiske testdata.
- En separat software-rasterisering brukes til visuelt modellbevis.
- Nettleseren i utviklingsøkten blokkerte loopback-URL-en. WebGL-visningen,
  input og FPS må derfor bekreftes på brukerens lokale PC; det er ikke hevdet
  at disse er visuelt verifisert i utviklingsnettleseren.

Kun den dokumenterte første modelltypen støttes: én frame/atomic/geometri/
materiale, ett UV-sett og D3D9 DXT1. Ukjente formater gir feil. Ingen biler,
kartstreaming, collision, scripts, missions, alpha-sortering, skygger eller
Grove Street-region er implementert ennå. Prelight-data bevares i modelldata,
men første renderer bruker enkel egen belysning og dekodet originaltexture.
