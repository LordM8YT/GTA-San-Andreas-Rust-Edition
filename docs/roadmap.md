# Veikart for native freeroam

Neste milepæl: **kjør sammen med venner i egne biler**, med opptil 20 spillere
inkludert verten. Missions er ikke prioritert. Dette dokumentet beskriver
planlagt arbeid; [multiplayer-status](multiplayer.md) og
[freeroam-status](freeroam-status.md) beskriver det som faktisk finnes.

## Prioriteringer

| Prioritet | Arbeid | Kriterium for milepælen |
| --- | --- | --- |
| 1 | Multiplayer på forskjellige PC-er og nettverk | To faktiske PC-er kan koble til, kjøre mellom streamede områder, koble fra og koble til igjen. Test både LAN og internett; samme-PC-testen dekker ikke dette. |
| 2 | Felles biler og passasjerer – implementert første versjon | Personlige biler vises etter utstigning, og verten reserverer tre passasjerplasser. Bilbytte, modellspesifikt seteantall og felles kollisjonsrespons gjenstår. |
| 3 | Bil, ped, antrekk og ressursversjoner – implementert første versjon | Valgte modeller og klesvalg deles fra en kontrollert ressursliste. Test videre på forskjellige PC-er med større modpakker. |
| 4 | Lyd – motorer og lokale fottrinn implementert | Originale motorloops følger fart og plassering, også i multiplayer. Lokale fottrinn følger bevegelse på bakken. Kollisjonslyd, materialvalg og fjernstyrte fottrinn gjenstår. |
| 5 | Trafikk og gående NPC-er | Start i ett nabolag med et begrenset antall aktører; mål ytelse og definer hvem som simulerer dem i multiplayer. |
| 6 | Lagring av posisjon, bilvalg og antrekk | En ny spilløkt gjenoppretter gyldige valg med reservevalg hvis ressurser er fjernet. Innstillinger lagres allerede. |
| 7 | Døgnsyklus, vær og Classic/Enhanced-profiler | Sammenhengende lys/vær og valgbare uttrykk, med samme tid/vær for deltakere i en session. Dagens grafikkprofiler er ikke en ferdig døgn-/værsimulering. |

Streaming, ytelse og bilfysikk må fortsatt tunes. Fjæring, pitch/roll og
akselbaserte dekkrefter finnes; bevegelige hjul, skade og en full fysisk
kollisjonsrespons gjenstår. En kapasitetstest med 20 nettverksklienter er ikke
dokumentasjon på ytelse med 20 fullt renderende spillere.

## Spillerhosting med serverbrowser og joincode

Dagens vert simulerer sin lokale verden og videresender spillerposeringer.
En første relay-versjon med serverbrowser og offentlige/private rom finnes nå;
se [oppsettet](multiplayer.md). `sa-relay` registrerer navn, spillerantall og
runtime-versjon. Private rom utelates fra listen og bruker en tilfeldig joincode
som invitasjon. Begge parter kobler ut, og all trafikk går via relayen.
Ressurslister, versjonssjekk og automatisk nedlasting/cache er nå koblet inn.

Direkte P2P uten manuell portåpning trenger NAT-traversering og en relay-reserve
når direkte forbindelse ikke virker, blant annet bak CGNAT. Direkte P2P er
fortsatt framtidig arbeid. Verten er fortsatt en spillers PC når
trafikken går via relay; et dedikert spillserverprogram er ikke nødvendig.
Katalog og relay trenger derimot en tilgjengelig tjeneste med drift og
konfigurasjon. Ingen slik offentlig tjeneste er satt opp for prosjektet nå.

Mulige tjenestebaserte løsninger er
[Epic Online Services P2P/lobby](https://dev.epicgames.com/docs/epic-online-services/multiplayer/nat-p2p-interface/eosp-2-p-sample)
eller [Steam Networking](https://partner.steamgames.com/doc/features/multiplayer/networking).
Valg og prosjektoppsett må avklares før integrasjon; dokumentasjonen betyr
ikke at prosjektet allerede har tilgang til deres produksjonstjenester.
Direkte IP beholdes som et alternativ for lokal testing.

## Automatisk modnedlasting og cache

Implementert første join-flyt:

1. Hent vertens ordnede ressursliste: ressurs-ID, formatversjon, filnavn,
   størrelse og SHA-256 for hver fil. Kontroller runtime-kompatibilitet.
2. Vis nødvendige ressurser, nedlastingsstørrelse og fremdrift. Brukerens valg
   om automatisk nedlasting styrer om nye filer hentes uten et ekstra spørsmål.
3. Gjenbruk filer i en separat cache når innholdshashen stemmer. Hent bare
   manglende eller endrede filer, med grenser for filstørrelse og total lagring.
4. Last ned til midlertidige filer og kontroller faktisk størrelse og hash før
   de tas i bruk. Avbrutte/ugyldige filer aktiveres aldri som ferdige ressurser.
5. Last et eget ressurssett for sesjonen i avtalt rekkefølge. Lokale mods
   overskrives ikke. Gjenopprett det lokale ressurssettet ved frakobling.

Cachen skal identifisere innhold, ikke bare servernavn. En endret ressurs får
ny hash, mens uendrede filer kan brukes ved neste join. En hash oppdager
endrede/skadede filer; den gjør ikke verten eller filinnholdet pålitelig.
Nedlastingen skal tillate støttede dataressurser, med avvisning av absolutte
stier, `..`, lenker ut av cacheområdet og kjørbare plugins/scripts.
En cacheoversikt med diskbruk og sletting av ubrukte ressurser gjenstår.

Første omfang er native manifest, DFF/TXD/PNG/COL/IFP og støttede biler, peds,
klær og plasseringer. GTA V/FiveM-modeller må konverteres på vertssiden før de
kan deles som native ressurser; Lua-scripts og vilkårlige FiveM-pakker får
ikke kompatibilitet av automatisk nedlasting. Originale San Andreas-filer
skal fortsatt leses fra hver spillers egen installasjon. Verten må bare dele
mods som de har rett til å distribuere.

Loaderen forbereder nå et eget ressurssett på en bakgrunnstråd og laster
GPU-data gradvis før et samlet bytte. Offline-verdenen beholdes og gjenopprettes
ved frakobling. Tester dekker cachetreff, versjonsendring, feil hash, avbrudd,
ugyldige stier og ressursbytte/gjenoppretting i to Vulkan-instanser. Valgt bil,
ped og antrekk synkroniseres nå, med separat klesvisning per spiller.

## Servermodell inspirert av ReSkate

Vennehosting beholdes i spillet, og `sa-server` kan nå kjøre uten spillvindu
eller originalinstallasjon. Se [serveroppsettet](server-hosting.md). Begge kan registreres i
samme browser og bruke joincode og native ressurscache. Første transport
er fortsatt prosjektets egen relay. Steam-lobbyer og Valves relay krever
eget integrasjonsarbeid og avklart Steamworks-oppsett.

Referanse: [ReSkates serveroppsett](https://github.com/Dingo-Shenanigans/ReSkate/blob/main/Server/README.txt),
sjekket 7. oktober 2026. Deres Steam-integrasjon er ikke lagt inn i prosjektet.
