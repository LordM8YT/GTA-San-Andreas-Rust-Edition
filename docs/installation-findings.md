# Funn fra brukerens installasjonsrapport

Kilde: vedlagt `sa-installation-report.zip`, lest 30. september 2026.
Dette er metadata fra lokal inspeksjon, ikke direkte tilgang til Windows-disken.

| Funn | Verdi |
|---|---:|
| Relevante løse filer | 322 |
| VER2-arkiver | 8 |
| Indeksfeil / inspeksjonsfeil | 0 / 0 |
| IDE-definisjoner i inventaret | 14 750 |
| Statiske kandidatreferanser med DFF/TXD-navn funnet | 14 205 |
| Oppføringer i models/gta3.img | 16 297 |
| Løse IDE-filer | 59 |
| Løse IPL-filer | 53 |

Tallene gjelder oppføringer i rapporten, ikke dedupliserte modeller eller
bevis på at alle objekter kan rendres. Alle åtte rapporterte arkiver har null
ugyldige klassiske områder og null ikke-null andre størrelsesfelter.

## Valgt første testmodell

- Objekt: `CJ_WASTEBIN`, ID `1347`.
- Referanse: `data/maps/generic/dynamic2.ide`, linje 22 i inventaret.
- DFF: `cj_wastebin.dff`, sektor `159712`, 2 sektorer = 4096 byte med padding.
- TXD: `cj_bins.txd`, sektor `159674`, 23 sektorer = 47104 byte med padding.
- Begge finnes i `models/gta3.img`; samlet 51200 byte før ZIP-komprimering.

Modellen er valgt fordi den er liten og statisk. Chunk-struktur, mesh,
materialteksturer og faktisk rendering er **ikke verifisert ennå**.

## Los Santos-data funnet

Rapporten lister LAe, LAe2, LAhills, LAn, LAn2, LAs, LAs2, LAw, LAw2 og
LaWn IPL-filer under `data/maps/LA`. Før Grove Street velges, må inst-data og
relevante binære IPL-streams leses. Ingen kartregion er lastet eller rendret.

## Neste blokkerende inngang

De to valgte asset-filene må gjøres tilgjengelig privat for å utvikle og teste
loaderen mot faktisk brukerdata. `export-first-model.cmd` gjør en avgrenset,
skrivebeskyttet lesing og legger kopier utenfor installasjonen.
