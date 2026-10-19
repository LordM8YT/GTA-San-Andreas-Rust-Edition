"""Original SARE trailer theme: a West Coast G-funk groove that builds into a
cinematic drop. Everything is synthesized here, so there is no license to clear.
96 BPM, C minor, 30 bars plus a short tail, 78 s. Usage: python tools/trailer-music.py theme.wav (needs numpy)"""
import sys, wave
import numpy as np

SR = 44100
BPM = 96
BEAT = 60 / BPM
BAR = 4 * BEAT
BARS = 30
N = int(BARS * BAR * SR) + SR * 3
rng = np.random.default_rng(1992)
L = np.zeros(N)
R = np.zeros(N)

def t_of(n):
    return np.arange(n) / SR

def add(sig, start, gain=1.0, pan=0.0):
    i = int(start * SR)
    if i >= N:
        return
    sig = sig[: N - i]
    l = np.cos((pan + 1) * np.pi / 4) * np.sqrt(2)
    r = np.sin((pan + 1) * np.pi / 4) * np.sqrt(2)
    L[i:i + len(sig)] += sig * gain * l
    R[i:i + len(sig)] += sig * gain * r

def fft_filter(sig, low=None, high=None, order=2.0):
    """Smooth high/low-pass in the frequency domain."""
    spec = np.fft.rfft(sig)
    f = np.fft.rfftfreq(len(sig), 1 / SR)
    gain = np.ones_like(f)
    if high is not None:  # low-pass cutoff
        gain *= 1 / np.sqrt(1 + (f / high) ** (2 * order))
    if low is not None:  # high-pass cutoff
        gain *= 1 / np.sqrt(1 + (low / np.maximum(f, 1e-3)) ** (2 * order))
    return np.fft.irfft(spec * gain, len(sig))

def note(name):
    names = {'C': 0, 'Db': 1, 'D': 2, 'Eb': 3, 'E': 4, 'F': 5, 'Gb': 6, 'G': 7, 'Ab': 8, 'A': 9, 'Bb': 10, 'B': 11}
    pitch, octave = name[:-1], int(name[-1])
    return 440 * 2 ** ((names[pitch] + 12 * (octave + 1) - 69) / 12)

def env(n, attack, release, sustain=1.0):
    t = t_of(n)
    a = np.clip(t / max(attack, 1e-4), 0, 1)
    r = np.clip((n / SR - t) / max(release, 1e-4), 0, 1)
    return a * r * sustain

def saw(freq, n, detune=0.0):
    t = t_of(n)
    phase = (freq * (1 + detune) * t + rng.random()) % 1.0
    return 2 * phase - 1

# ---------- drums ----------
def kick():
    n = int(0.5 * SR)
    t = t_of(n)
    freq = 45 + 85 * np.exp(-t * 28)
    phase = 2 * np.pi * np.cumsum(freq) / SR
    body = np.sin(phase) * np.exp(-t * 6.5)
    click = rng.standard_normal(n) * np.exp(-t * 300) * 0.3
    return np.tanh((body + click) * 1.6)

def snare():
    n = int(0.35 * SR)
    t = t_of(n)
    noise = fft_filter(rng.standard_normal(n), low=900, high=9000) * np.exp(-t * 18)
    tone = np.sin(2 * np.pi * 190 * t) * np.exp(-t * 30)
    return np.tanh((noise * 0.9 + tone * 0.6) * 1.4)

def clap():
    n = int(0.3 * SR)
    t = t_of(n)
    noise = fft_filter(rng.standard_normal(n), low=1100, high=6000)
    e = np.zeros(n)
    for k, d in enumerate([0, 0.011, 0.022]):
        i = int(d * SR)
        e[i:] += np.exp(-(t[: n - i]) * (90 if k < 2 else 16))
    return noise * e * 0.6

def hat(open_=False):
    n = int((0.25 if open_ else 0.06) * SR)
    t = t_of(n)
    return fft_filter(rng.standard_normal(n), low=7000) * np.exp(-t * (14 if open_ else 70))

KICK, SNARE, CLAP, HAT, OHAT = kick(), snare(), clap(), hat(), hat(True)

def impact():
    n = int(4 * SR)
    t = t_of(n)
    boom = np.sin(2 * np.pi * (32 + 40 * np.exp(-t * 4)) * t) * np.exp(-t * 1.1)
    crash = fft_filter(rng.standard_normal(n), low=3000) * np.exp(-t * 2.2) * 0.35
    return np.tanh((boom + crash) * 1.5)

def riser(seconds):
    n = int(seconds * SR)
    t = t_of(n)
    shape = (t / seconds) ** 2
    noise = rng.standard_normal(n)
    # Two bands crossfaded upwards approximate a rising filter.
    low = fft_filter(noise, low=300, high=1500)
    high = fft_filter(noise, low=3000, high=12000)
    tone = np.sin(2 * np.pi * np.cumsum(200 + 900 * shape) / SR) * 0.15
    return (low * (1 - shape) + high * shape) * shape * 0.5 + tone * shape

# ---------- harmony ----------
# i - VI - iv - V in C minor, one chord per bar.
PROGRESSION = [
    ['C3', 'Eb3', 'G3', 'Bb3', 'D4'],
    ['Ab2', 'C3', 'Eb3', 'G3', 'C4'],
    ['F2', 'Ab2', 'C3', 'Eb3', 'G3'],
    ['G2', 'B2', 'D3', 'F3', 'Ab3'],
]
ROOTS = ['C2', 'Ab1', 'F1', 'G1']

def pad(chord, seconds, bright=1800):
    n = int(seconds * SR)
    sig = np.zeros(n)
    for name in chord:
        for d in (-0.006, 0.0, 0.006):
            sig += saw(note(name), n, d)
    sig = fft_filter(sig, high=bright) / (len(chord) * 3)
    return sig * env(n, seconds * 0.35, seconds * 0.3)

def rhodes(chord, seconds):
    n = int(seconds * SR)
    t = t_of(n)
    sig = np.zeros(n)
    for name in chord[1:]:
        f = note(name)
        sig += np.sin(2 * np.pi * f * t + 0.8 * np.sin(2 * np.pi * f * t) * np.exp(-t * 3))
    return sig / 4 * np.exp(-t * 1.2) * env(n, 0.005, 0.2)

def bass(name, seconds, slide_to=None):
    n = int(seconds * SR)
    t = t_of(n)
    f0 = note(name)
    f = np.full(n, f0)
    if slide_to:
        f1 = note(slide_to)
        glide = np.clip((t - seconds * 0.75) / (seconds * 0.25), 0, 1)
        f = f0 * (f1 / f0) ** glide
    phase = 2 * np.pi * np.cumsum(f) / SR
    sig = np.sin(phase) + 0.25 * np.sin(2 * phase) + 0.1 * np.sign(np.sin(phase))
    return np.tanh(sig * 1.2) * env(n, 0.01, 0.08)

def lead(notes, start):
    """G-funk whistle lead: sine with portamento and delayed vibrato."""
    total = sum(d for _, d in notes)
    n = int(total * BEAT * SR)
    t = t_of(n)
    f = np.zeros(n)
    pos = 0
    prev = note(notes[0][0])
    for name, beats in notes:
        k = int(beats * BEAT * SR)
        target = note(name)
        glide = np.clip(t_of(k) / 0.06, 0, 1)
        f[pos:pos + k] = prev * (target / prev) ** glide
        prev = target
        pos += k
    f = f[:n] * (1 + 0.012 * np.sin(2 * np.pi * 5.5 * t) * np.clip(t / 1.5, 0, 1))
    phase = 2 * np.pi * np.cumsum(f) / SR
    sig = np.sin(phase) + 0.12 * np.sin(3 * phase)
    add(sig * env(n, 0.05, 0.4) * 0.3, start, pan=0.25)

def brass(chord, seconds):
    n = int(seconds * SR)
    t = t_of(n)
    sig = np.zeros(n)
    for name in chord[:4]:
        for d in (-0.004, 0.004):
            sig += saw(note(name) * 2, n, d)
    # Filter opens fast then settles, like a brass section swell.
    bright = fft_filter(sig, high=3500)
    dark = fft_filter(sig, high=900)
    open_ = np.exp(-t * 3)
    return (bright * open_ + dark * (1 - open_)) / 8 * env(n, 0.02, 0.25)

def choir(chord, seconds):
    n = int(seconds * SR)
    t = t_of(n)
    sig = np.zeros(n)
    for name in chord[:4]:
        f = note(name) * 2
        for d in (-0.01, -0.003, 0.004, 0.011):
            vib = 1 + 0.004 * np.sin(2 * np.pi * (4.5 + d * 50) * t)
            sig += np.sin(2 * np.pi * np.cumsum(f * (1 + d) * vib) / SR)
    sig = fft_filter(sig * (1 + 0.3 * np.sin(2 * np.pi * 900 * t)), high=2600)
    return sig / 16 * env(n, 0.6, 0.8)

# ---------- arrangement ----------
def bar_time(b):
    return b * BAR

def drums(bar, fill=False, big=False):
    t0 = bar_time(bar)
    for beat in range(4):
        for sixteenth in range(4):
            pos = beat + sixteenth / 4
            swing = 0.035 if sixteenth % 2 else 0.0
            ts = t0 + (pos + swing) * BEAT
            if sixteenth == 0 or (sixteenth == 2 and beat in (1, 3) and big):
                pass
            gain = 0.18 if sixteenth % 2 == 0 else 0.11
            if not (fill and beat == 3):
                add(OHAT if (beat == 3 and sixteenth == 2) else HAT, ts, gain, pan=-0.3)
    for pos in ([0, 1.75, 2.5] if not big else [0, 0.75, 1.5, 2, 2.75]):
        add(KICK, t0 + pos * BEAT, 0.7)
    for pos in (1, 3):
        add(SNARE, t0 + pos * BEAT, 0.55)
        add(CLAP, t0 + pos * BEAT, 0.45, pan=0.1)
    if fill:
        for k in range(8):
            add(SNARE, t0 + (3 + k / 8) * BEAT, 0.15 + k * 0.04)

# Intro (bars 0-3): pads and sub drones, an impact at the start of the groove.
for bar in range(4):
    chord = PROGRESSION[bar % 4]
    add(pad(chord, BAR * 1.2, bright=900 + bar * 300), bar_time(bar), 0.5)
    add(bass(ROOTS[bar % 4], BAR, None) * 0.5, bar_time(bar), 0.5)
add(riser(BAR * 2), bar_time(2), 0.8)

# Groove A (bars 4-15): drums, bass, rhodes, G-funk lead from bar 8.
for bar in range(4, 16):
    chord = PROGRESSION[bar % 4]
    drums(bar, fill=(bar % 4 == 3))
    add(pad(chord, BAR * 1.1), bar_time(bar), 0.42)
    for hit in (0, 1.5, 2.5):
        add(rhodes(chord, BEAT * 1.4), bar_time(bar) + hit * BEAT, 0.42, pan=-0.2)
    root = ROOTS[bar % 4]
    nxt = ROOTS[(bar + 1) % 4]
    add(bass(root, BEAT * 1.5), bar_time(bar), 0.4)
    add(bass(root, BEAT * 0.5), bar_time(bar) + 1.75 * BEAT, 0.32)
    add(bass(root, BEAT * 1.5, slide_to=nxt), bar_time(bar) + 2.5 * BEAT, 0.36)
add(impact(), bar_time(4), 0.8)
melody = [('G5', 1.5), ('Bb5', 0.5), ('C6', 2), ('Bb5', 1), ('G5', 1), ('Eb5', 2),
          ('F5', 1.5), ('G5', 0.5), ('Ab5', 2), ('G5', 1), ('F5', 1), ('D5', 2)]
for phrase_bar in (8, 12):
    lead(melody, bar_time(phrase_bar))

# Build (bars 16-19): drums thin out, filtered pads, snare roll and riser.
for bar in range(16, 20):
    chord = PROGRESSION[bar % 4]
    add(pad(chord, BAR * 1.1, bright=1200 + (bar - 16) * 900), bar_time(bar), 0.4)
    add(KICK, bar_time(bar), 0.8)
    add(KICK, bar_time(bar) + 2 * BEAT, 0.6)
    rolls = 4 * (2 ** (bar - 16)) if bar < 19 else 32
    for k in range(rolls):
        add(SNARE, bar_time(bar) + k * BAR / rolls, 0.12 + 0.3 * (bar - 16 + k / rolls) / 4)
add(riser(BAR * 4), bar_time(16), 0.9)

# Drop (bars 20-27): full drums, brass stabs, choir, bass and lead together.
add(impact(), bar_time(20), 1.0)
for bar in range(20, 28):
    chord = PROGRESSION[bar % 4]
    drums(bar, fill=(bar == 27), big=True)
    add(choir(chord, BAR * 1.05), bar_time(bar), 1.0)
    for hit in (0, 0.75, 2, 2.75):
        add(brass(chord, BEAT * 0.7), bar_time(bar) + hit * BEAT, 1.0, pan=0.15)
    root = ROOTS[bar % 4]
    add(bass(root, BEAT * 0.75), bar_time(bar), 0.6)
    add(bass(root, BEAT * 0.75), bar_time(bar) + 0.75 * BEAT, 0.5)
    add(bass(root, BEAT * 1.25, slide_to=ROOTS[(bar + 1) % 4]), bar_time(bar) + 2 * BEAT, 0.55)
lead(melody, bar_time(20))
lead(melody, bar_time(24))

# Outro (bars 28-29): final hit on C minor and a long tail for the logo.
add(impact(), bar_time(28), 1.1)
add(choir(PROGRESSION[0], BAR * 2), bar_time(28), 0.6)
add(pad(PROGRESSION[0], BAR * 2.2, bright=1400), bar_time(28), 0.5)

# ---------- mix ----------
def reverb(sig, seconds=2.2, mix=0.18):
    n = int(seconds * SR)
    t = t_of(n)
    ir = rng.standard_normal(n) * np.exp(-t * 6.9 / seconds)
    ir = fft_filter(ir, low=200, high=7000)
    ir /= np.sqrt(np.sum(ir ** 2))
    size = 1 << int(np.ceil(np.log2(len(sig) + n)))
    wet = np.fft.irfft(np.fft.rfft(sig, size) * np.fft.rfft(ir, size), size)[: len(sig)]
    return sig + wet * mix

L, R = reverb(L), reverb(R)
mix = np.stack([L, R], axis=1)
mix = np.tanh(mix / (np.max(np.abs(mix)) * 0.7)) * 0.9
fade = int(1.5 * SR)
mix[-fade:] *= np.linspace(1, 0, fade)[:, None]
pcm = (mix * 32767).astype('<i2')
with wave.open(sys.argv[1], 'wb') as out:
    out.setnchannels(2)
    out.setsampwidth(2)
    out.setframerate(SR)
    out.writeframes(pcm.tobytes())
print(f"wrote {sys.argv[1]}: {len(pcm) / SR:.1f} s")
