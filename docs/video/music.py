"""Synthesize the video's music, lay the narration over it and duck the music under the voice.

Everything is generated here with numpy: an original 120 BPM track in D major (kick, clap,
hats, an offbeat bass, pads, chord stabs and a ping-pong arpeggio), no samples. The arrangement
follows out/timeline.json: an intro that rises into the principles, the full groove for the
principles, a brighter variation for the features with a crash and a fill on every feature
group, and an outro that ends on a held chord. The narration clips (48 kHz mono f32) sit at
their beats' voice_at; the music ducks about 13 dB under speech, sitting about 18 dB below the
voice there.

usage: python3 -I music.py TIMELINE VOICE_DIR OUT_DIR
       writes OUT_DIR/music.f32 and OUT_DIR/mix.f32 (48 kHz stereo float32, not yet loudness-normalized)
"""

import json
import math
import os
import sys

import numpy as np

SR = 48000
BEAT = 0.5
BAR = 2.0
RNG = np.random.default_rng(20261120)

CHORDS = {  # bass note, pad voicing (MIDI)
    "D": (38, [62, 66, 69, 74]),
    "A": (45, [61, 64, 69, 73]),
    "Bm": (47, [62, 66, 71, 74]),
    "G": (43, [62, 67, 71, 74]),
    "F#m": (42, [61, 66, 69, 73]),
}
PROG = {"intro": ["D", "A", "Bm", "G"], "prin": ["D", "A", "Bm", "G"],
        "feat": ["Bm", "G", "D", "A"], "close": ["G", "A", "D", "D"]}


def hz(m):
    return 440.0 * 2 ** ((m - 69) / 12)


def tt(n):
    return np.arange(n) / SR


def filt(x, lo=None, hi=None, order=2):
    X = np.fft.rfft(x)
    f = np.fft.rfftfreq(len(x), 1 / SR)
    g = np.ones_like(f)
    if hi:
        g /= np.sqrt(1 + (f / hi) ** (2 * order))
    if lo:
        with np.errstate(divide="ignore"):
            g /= np.sqrt(1 + (lo / np.maximum(f, 1e-3)) ** (2 * order))
    return np.fft.irfft(X * g, len(x))


def saw(f, n, phase=0.0):
    return 2 * ((tt(n) * f + phase) % 1.0) - 1


def adsr(n, a, d, s, r_start, r):
    t = tt(n)
    env = np.where(t < a, t / max(a, 1e-4), s + (1 - s) * np.exp(-(t - a) / max(d, 1e-4)))
    rel = np.clip((t - r_start) / max(r, 1e-4), 0, 1)
    return env * (1 - rel)


CACHE = {}


def kick():
    if "kick" not in CACHE:
        n = int(0.42 * SR)
        t = tt(n)
        f = 48 + 110 * np.exp(-t / 0.035)
        ph = 2 * np.pi * np.cumsum(f) / SR
        body = np.sin(ph) * np.exp(-t / 0.26)
        click = filt(RNG.standard_normal(n) * np.exp(-t / 0.003), lo=1500) * 0.4
        CACHE["kick"] = np.tanh(1.6 * (body + click)) * 0.9
    return CACHE["kick"]


def clap():
    if "clap" not in CACHE:
        n = int(0.32 * SR)
        t = tt(n)
        env = np.zeros(n)
        for k, o in enumerate((0.0, 0.011, 0.022)):
            env += np.where(t >= o, np.exp(-(t - o) / 0.008), 0) * (0.8 if k < 2 else 1.0)
        env += np.where(t >= 0.022, np.exp(-(t - 0.022) / 0.11), 0) * 0.55
        CACHE["clap"] = filt(RNG.standard_normal(n), lo=900, hi=3200) * env * 0.55
    return CACHE["clap"]


def hat(open_=False):
    key = "hat_o" if open_ else "hat"
    if key not in CACHE:
        n = int((0.22 if open_ else 0.07) * SR)
        t = tt(n)
        CACHE[key] = filt(RNG.standard_normal(n), lo=7000) * np.exp(-t / (0.09 if open_ else 0.022)) * 0.35
    return CACHE[key]


def crash():
    if "crash" not in CACHE:
        n = int(2.2 * SR)
        t = tt(n)
        CACHE["crash"] = filt(RNG.standard_normal(n), lo=4500) * np.exp(-t / 0.75) * 0.32
    return CACHE["crash"]


def bass(m, dur):
    key = ("bass", m, dur)
    if key not in CACHE:
        n = int((dur + 0.05) * SR)
        f = hz(m)
        x = 0.6 * saw(f, n) + 0.5 * np.sign(np.sin(2 * np.pi * f * tt(n))) * 0.5 + 0.5 * np.sin(2 * np.pi * f / 2 * tt(n))
        x = filt(x, hi=750) * adsr(n, 0.004, 0.09, 0.55, dur, 0.04)
        CACHE[key] = np.tanh(1.4 * x) * 0.42
    return CACHE[key]


def pad_note(m, dur, cutoff):
    key = ("pad", m, dur, int(cutoff))
    if key not in CACHE:
        n = int((dur + 0.7) * SR)
        f = hz(m)
        L = sum(saw(f * 2 ** (c / 1200), n, p) for c, p in ((-9, 0.1), (0, 0.5), (7, 0.8)))
        R = sum(saw(f * 2 ** (c / 1200), n, p) for c, p in ((-6, 0.3), (0, 0.9), (10, 0.2)))
        env = adsr(n, 0.25, 0.8, 0.8, dur, 0.6)
        CACHE[key] = (filt(L, hi=cutoff) * env * 0.05, filt(R, hi=cutoff) * env * 0.05)
    return CACHE[key]


def pluck(m, dur=0.16, cutoff=3200, gain=0.12):
    key = ("pluck", m, dur, cutoff)
    if key not in CACHE:
        n = int((dur + 0.08) * SR)
        f = hz(m)
        x = np.sign(np.sin(2 * np.pi * f * tt(n))) * 0.6 + saw(f * 1.003, n) * 0.4
        CACHE[key] = filt(x, hi=cutoff) * np.exp(-tt(n) / (dur * 0.45)) * gain
    return CACHE[key]


def stab(chord, cutoff=2600):
    key = ("stab", chord, cutoff)
    if key not in CACHE:
        n = int(0.32 * SR)
        x = sum(saw(hz(m + 12), n, 0.37 * k) for k, m in enumerate(CHORDS[chord][1]))
        CACHE[key] = filt(x, hi=cutoff) * np.exp(-tt(n) / 0.07) * 0.06
    return CACHE[key]


def riser(dur):
    n = int(dur * SR)
    t = tt(n)
    out = np.zeros(n)
    chunks = 12
    for k in range(chunks):
        a, b = k * n // chunks, (k + 1) * n // chunks
        seg = filt(RNG.standard_normal(b - a + 2400), lo=300, hi=600 + 9000 * (k / chunks) ** 2)
        w = np.ones(b - a + 2400)
        w[:1200] = np.linspace(0, 1, 1200)
        w[-1200:] = np.linspace(1, 0, 1200)
        lo_, hi_ = max(0, a - 1200), min(n, b + 1200)
        out[lo_:hi_] += (seg * w)[1200 - (a - lo_):1200 - (a - lo_) + hi_ - lo_]
    sweep = np.sin(2 * np.pi * np.cumsum(220 + 660 * (t / dur) ** 2) / SR) * 0.15
    return (out * 0.18 + sweep) * (t / dur) ** 2


class Track:
    def __init__(self, seconds):
        self.n = int(seconds * SR) + SR * 3
        self.bus = {k: np.zeros((2, self.n)) for k in ("kick", "drums", "bass", "pad", "lead")}

    def add(self, bus, t, x, pan=0.0, gain=1.0):
        i = int(round(t * SR))
        if i >= self.n or i < 0:
            return
        if isinstance(x, tuple):
            L, R = x
        else:
            L = R = x
        m = min(len(L), self.n - i)
        gl, gr = gain * math.sqrt(0.5 * (1 - pan)), gain * math.sqrt(0.5 * (1 + pan))
        self.bus[bus][0, i:i + m] += L[:m] * gl * 1.414
        self.bus[bus][1, i:i + m] += R[:m] * gr * 1.414


def shift(x, d):
    y = np.zeros_like(x)
    y[..., d:] = x[..., :-d]
    return y


def build(tl):
    end = tl["end"]
    secs = tl["sections"]
    P = next(s["t0"] for s in secs if s["section"] == "principles")
    F = next(s["t0"] for s in secs if s["section"] == "features")
    C = next(s["t0"] for s in secs if s["section"] == "close")
    groups = [s["t0"] for s in secs if s["section"] == "features"]
    tr = Track(end)
    nbars = int(math.ceil(end / BAR))
    final = C + BAR
    for bar in range(nbars):
        t0 = bar * BAR
        if t0 < P:
            sec, rel = "intro", bar
        elif t0 < F:
            sec, rel = "prin", int((t0 - P) / BAR)
        elif t0 < C:
            sec, rel = "feat", int((t0 - F) / BAR)
        else:
            sec, rel = "close", int((t0 - C) / BAR)
        chord = PROG[sec][rel % 4]
        if sec == "close" and t0 >= final:
            chord = "D"
        root, voicing = CHORDS[chord]
        tail = t0 >= final
        # pads
        if sec == "intro":
            cutoff = 500 + 2600 * (t0 / max(P, 1)) ** 1.5
        elif sec == "feat":
            cutoff = 3400
        else:
            cutoff = 2400
        if tail:
            if t0 == final:
                for m in voicing + [voicing[0] - 12]:
                    tr.add("pad", t0, pad_note(m, end - t0 - 0.6, 2800), gain=1.3)
        else:
            for m in voicing:
                tr.add("pad", t0, pad_note(m, BAR, cutoff), gain=0.9 if sec != "intro" else 1.1)
        # arpeggio, 16ths, up-down across the voicing an octave up
        arp_on = (sec != "intro" or bar >= 1) and not tail
        if arp_on:
            notes = [m + 12 for m in voicing]
            seq = notes + notes[-2:0:-1]
            ag = 0.6 + 0.4 * min(1.0, t0 / max(P, 1)) if sec == "intro" else 1.0
            for k in range(16):
                m = seq[k % len(seq)]
                t = t0 + k * BAR / 16
                g = ag * (1.0 if k % 4 == 0 else 0.7)
                tr.add("lead", t, pluck(m, cutoff=2600 if sec == "intro" else 3600), pan=-0.35 if k % 2 else 0.35, gain=g)
        # drums and bass
        groove = sec in ("prin", "feat", "close") and not tail
        if groove:
            for k in range(4):
                tr.add("kick", t0 + k * BEAT, kick())
            for k in (1, 3):
                tr.add("drums", t0 + k * BEAT, clap(), pan=0.05, gain=0.95)
            for k in range(16):
                t = t0 + k * BAR / 16
                if k % 4 == 2:
                    tr.add("drums", t, hat(True), pan=0.2, gain=0.8)
                else:
                    tr.add("drums", t, hat(), pan=0.25, gain=0.55 if k % 2 else 0.35)
            for k in range(8):
                if k % 2 == 1 or (sec == "feat" and k == 6):
                    m = root + (12 if k in (3, 7) else 0)
                    tr.add("bass", t0 + k * BEAT / 2, bass(m, 0.2))
            if sec == "feat":
                for k in (3, 7, 11, 14):
                    tr.add("lead", t0 + k * BAR / 16, stab(chord), pan=0.0, gain=0.9)
        elif sec == "intro" and bar >= 2:
            for k in range(16):
                t = t0 + k * BAR / 16
                tr.add("drums", t, hat(), pan=0.25, gain=(0.25 if k % 2 else 0.15) * min(1.0, (bar - 1) / 3))
    # the rise into the principles and the fills before each section change
    rd = min(4.0, P)
    tr.add("drums", P - rd, riser(rd), gain=0.9)
    for k in range(8):
        t = P - BAR + k * BAR / 8 if k < 4 else P - BAR / 2 + (k - 4) * BAR / 16
        tr.add("drums", t, clap(), gain=0.25 + 0.1 * k)
    for g in groups[1:] + [C]:
        for k in range(4):
            tr.add("drums", g - BEAT + k * BEAT / 4, clap(), gain=0.3 + 0.12 * k)
    for t in [P, F] + groups[1:] + [C, final]:
        tr.add("drums", t, crash(), pan=-0.1, gain=1.0 if t in (P, F, final) else 0.7)
    tr.add("kick", final, kick(), gain=1.1)
    tr.add("lead", final, stab("D", 3200), gain=1.6)
    # sidechain pump on everything but the kick, wherever the kick plays
    pump = np.ones(tr.n)
    t_ = np.arange(tr.n) / SR
    beat_pos = t_ % BEAT
    grooving = (t_ >= P) & (t_ < final)
    pump[grooving] = 1 - 0.4 * np.exp(-beat_pos[grooving] / 0.11)
    # a cheap tapped-delay room on the pads and leads
    room = tr.bus["pad"] + tr.bus["lead"]
    wet = np.zeros_like(room)
    for k, ms in enumerate((23, 37, 53, 71, 97, 131, 173, 211, 263, 331, 397, 461)):
        d = int(ms * SR / 1000)
        g = 0.32 * 0.84 ** k
        src = room[::-1] if k % 2 else room
        wet += shift(src, d) * g
    lead = tr.bus["lead"] + 0.55 * shift(tr.bus["lead"][::-1], int(0.375 * SR))
    mix = (tr.bus["kick"] * 0.8 + tr.bus["drums"] * 0.95 + (tr.bus["bass"] * 0.8 + tr.bus["pad"] * 1.0
           + lead * 0.8 + wet * 0.5) * pump)
    n_end = int(end * SR)
    mix = mix[:, :n_end]
    fade = np.ones(n_end)
    k = int(2.4 * SR)
    fade[-k:] = np.linspace(1, 0, k) ** 1.5
    return mix * fade


def envelope(x, win=0.01, attack=0.03, release=0.3):
    w = int(win * SR)
    n = len(x) // w
    peaks = np.abs(x[:n * w]).reshape(n, w).max(axis=1)
    out = np.zeros(n)
    a, r = math.exp(-win / attack), math.exp(-win / release)
    v = 0.0
    for i, p in enumerate(peaks):
        c = a if p > v else r
        v = c * v + (1 - c) * p
        out[i] = v
    return np.repeat(out, w), w * n


def main(timeline, vdir, odir):
    tl = json.load(open(timeline))
    music = build(tl)
    n = music.shape[1]
    voice = np.zeros(n)
    for b in tl["beats"]:
        path = os.path.join(vdir, b["id"] + ".f32")
        clip = np.fromfile(path, dtype="<f4").astype(np.float64)
        i = int(round(b["voice_at"] * SR))
        m = min(len(clip), n - i)
        voice[i:i + m] += clip[:m]
    speech = np.abs(voice) > 0.02
    vrms = math.sqrt(np.mean(voice[speech] ** 2)) if speech.any() else 0.1
    mrms = math.sqrt(np.mean(music ** 2)) or 1.0
    music *= vrms * 10 ** (-5 / 20) / mrms
    env, m = envelope(voice)
    act = np.clip(env / 0.04, 0, 1)
    gain = np.ones(n)
    gain[:m] = 10 ** (-13 * act / 20)
    ducked = music * gain
    mix = ducked + voice[None, :]
    peak = np.abs(mix).max()
    mix *= 10 ** (-1 / 20) / peak
    music_out = music * 10 ** (-1 / 20) / np.abs(music).max()
    os.makedirs(odir, exist_ok=True)
    music_out.T.astype("<f4").tofile(os.path.join(odir, "music.f32"))
    mix.T.astype("<f4").tofile(os.path.join(odir, "mix.f32"))
    under = ducked[:, speech[:n]]
    if under.size:
        rel = 20 * math.log10(math.sqrt(np.mean(under ** 2)) / vrms)
        print("music under speech: %.1f dB relative to the voice" % rel)
    print("mix: %.1f s, peak normalized to -1 dBFS" % (n / SR))


if __name__ == "__main__":
    main(*sys.argv[1:4])
