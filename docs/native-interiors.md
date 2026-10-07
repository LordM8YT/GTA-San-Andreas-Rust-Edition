# Originalinteriører

Kartlasteren leser nå både `models/gta3.img` og `models/gta_int.img` fra
den lokale installasjonen. Ingen originalfiler endres. Det første arkivet
har prioritet hvis et navn finnes i begge. IPL-strømmer, modeller, TXD-er
og COL-er fra interiørarkivet bruker de samme begrensede dekoderne som kartet.

Utendørs lasting velger bare interiør-ID 0. `WorldLoader::load_interior`
velger ett bestemt, ikke-null interiør-ID og bruker samme geometrilaster,
teksturoppslag og kollisjon. Utendørsvann inkluderes ikke i rommene.
Modellenes faktiske grenser avgjør hvilke plasseringer som tas med.

Interiørmenyen (I) tilbyr CJ sitt hus, Sweet sitt hus og Madd Dogg sin villa.
R eller et valg i kartmenyen laster et utendørsområde før spilleren flyttes.
Rommet byttes først etter kontroll av inngangens ståplass. Gange holdes på
rommets underlag ved åpne portalkanter, slik at manglende utendørsgulv ikke
gir et fall. Vanlige dørinnganger og flere reisemål gjenstår.
GTA V MLO-filer lastes ikke direkte.

Fra `native` kan originaldata og bevegelse kontrolleres med:

```powershell
./target/release/sa-runtime.exe --probe-interiors
```

Kontrollen laster CJ sitt hus (ID 3), Sweet sitt hus (ID 1) og Madd Dogg
sin villa (ID 5). Den krever en trygg ståplass ved inngangen, utfører 600
steg med gange og et hopp i hvert rom, og avviser fall gjennom gulvet eller
manglende landing. En vellykket kontroll beviser denne ruten; alle rom og
alle dører er ikke kontrollert.

`--smoke-interiors` kontrollerer også GPU-opplasting, 120 rendersteg med
gange/hopp i hvert av de tre rommene, korrekt dimensjon/vann og retur til
Grove Street. Denne ruten er bestått. Når et trangt rom presser kameraet
helt inntil figuren, skjules figuren midlertidig for å beholde utsikten.

Lokal installasjon: 44 804 utendørsplasseringer, 5 965 interiørplasseringer
og 10 155 kollisjonsmodeller. De tre kontrollrommene hadde henholdsvis
36/14/288 plasseringer og 12 366/9 819/67 050 rendertriangler. Sweet sitt
hus har ett materiale med reservefarge fordi teksturen mangler i oppgitt TXD.
