"""Fetch one Qwen3-TTS clip per narration segment, check each with Whisper, and measure it.

Reads narration.json, writes out/voice/<id>.wav, out/voice/transcripts.json and timing.json
(each clip's length and the span that holds speech). A clip whose transcript does not match its
text closely, or garbles a name, is fetched again, up to six tries, keeping the best. Endpoints and voice come
from the environment: TTS_URL (an OpenAI-compatible /v1 base, also used for transcriptions),
TTS_VOICE (the speech voice), STT_MODEL, and LLM_API_KEY, which is only ever sent as a header.

usage: python3 -I narrate.py [--force ID ...]
       python3 -I narrate.py --check FILE.wav TEXT
       python3 -I narrate.py --mix MIX.wav out/timeline.json   (each beat of the final mix)
"""

import difflib
import json
import os
import re
import struct
import sys
import time
import urllib.request
import uuid

BASE = os.environ.get("TTS_URL", "http://localhost:8000/v1")
STT = os.environ.get("STT_MODEL", "faster-whisper-large-v3-turbo")
HERE = os.path.dirname(os.path.abspath(__file__))
VOICE = os.path.join(HERE, "out", "voice")
NUM = {"1": "one", "2": "two", "3": "three", "4": "four", "5": "five", "6": "six", "7": "seven",
       "8": "eight", "9": "nine", "10": "ten", "46": "fortysix", "11": "eleven", "12": "twelve"}


def auth():
    return {"Authorization": "Bearer " + os.environ["LLM_API_KEY"]}


def post(url, body, ctype, timeout=300):
    req = urllib.request.Request(url, data=body, method="POST",
                                 headers=dict(auth(), **{"Content-Type": ctype}))
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return r.read()


def tts(voice, text):
    body = json.dumps(dict(voice, input=text, response_format="wav")).encode()
    return post(BASE + "/audio/speech", body, "application/json")


def transcribe(path):
    b = uuid.uuid4().hex
    parts = []
    for k, v in (("model", STT), ("language", "en"), ("response_format", "json")):
        parts.append(f'--{b}\r\nContent-Disposition: form-data; name="{k}"\r\n\r\n{v}\r\n'.encode())
    parts.append(f'--{b}\r\nContent-Disposition: form-data; name="file"; filename="clip.wav"\r\n'
                 'Content-Type: audio/wav\r\n\r\n'.encode() + open(path, "rb").read() + b"\r\n")
    parts.append(f"--{b}--\r\n".encode())
    out = post(BASE + "/audio/transcriptions", b"".join(parts), "multipart/form-data; boundary=" + b)
    return json.loads(out)["text"].strip()


def norm(s):
    s = s.lower().replace("new m r s", "newmrs").replace("new m", "newm")
    s = s.replace("newm-rs", "newmrs").replace("forty-six", "fortysix")
    words = re.findall(r"[a-z]+|\d+", s)
    return "".join(NUM.get(w, w) for w in words)


def heard_names(text, heard):
    """The spoken names came through: newm-rs and newm as "new M R S"/"new M", not "GNU M"."""
    h = heard.lower().replace("-", " ")
    if "new m r s" in text.lower() and not re.search(r"\bnew ?m ?r ?s\b", h):
        return False
    if re.search(r"new[- ]m\b(?! r)", text, re.I) and not re.search(r"\bnew ?m\b(?! ?r ?s)", h):
        return False
    return "gnu" not in h


def score(text, heard):
    return difflib.SequenceMatcher(None, norm(text), norm(heard)).ratio()


def pcm(path):
    """(rate, mono samples in -1..1) from a 16-bit PCM or 32-bit float WAV."""
    data = open(path, "rb").read()
    pos, fmt, rate, ch, bits, raw = 12, None, None, None, None, None
    while pos + 8 <= len(data):
        cid, size = data[pos:pos + 4], struct.unpack("<I", data[pos + 4:pos + 8])[0]
        body = data[pos + 8:pos + 8 + size]
        if cid == b"fmt ":
            fmt, ch, rate = struct.unpack("<HHI", body[:8])
            bits = struct.unpack("<H", body[14:16])[0]
        elif cid == b"data":
            raw = body
        pos += 8 + size + (size & 1)
    if fmt == 3 and bits == 32:
        vals = struct.unpack("<%df" % (len(raw) // 4), raw[:len(raw) // 4 * 4])
    elif bits == 16:
        vals = [v / 32768 for v in struct.unpack("<%dh" % (len(raw) // 2), raw[:len(raw) // 2 * 2])]
    else:
        raise ValueError("unsupported wav: fmt %s bits %s" % (fmt, bits))
    return rate, vals[::ch]


def measure(path):
    rate, x = pcm(path)
    win = max(1, rate // 100)
    env = [max(abs(v) for v in x[i:i + win]) for i in range(0, len(x), win)]
    on = [i for i, e in enumerate(env) if e > 0.02]
    lead = (on[0] * win / rate) if on else 0.0
    tail = ((on[-1] + 1) * win / rate) if on else len(x) / rate
    return {"dur": round(len(x) / rate, 3), "speech": [round(lead, 3), round(tail, 3)], "rate": rate}


def check_mix(mix, timeline):
    """Transcribe the final mix beat by beat, to hear each line through the music."""
    import wave
    tl = json.load(open(timeline))
    src = wave.open(mix, "rb")
    rate, width, ch = src.getframerate(), src.getsampwidth(), src.getnchannels()
    frames = src.readframes(src.getnframes())
    worst = 1.0
    for b in tl["beats"]:
        a = max(0, int((b["voice_at"] - 0.15) * rate))
        z = int((b["voice_at"] + b["clip"] + 0.2) * rate)
        tmp = os.path.join(VOICE, "mixcut.wav")
        with wave.open(tmp, "wb") as out:
            out.setnchannels(ch)
            out.setsampwidth(width)
            out.setframerate(rate)
            out.writeframes(frames[a * width * ch:z * width * ch])
        h = transcribe(tmp)
        r = score(b["text"], h)
        worst = min(worst, r)
        print("%-9s %.3f %s  %s" % (b["id"], r, "names-ok" if heard_names(b["text"], h) else "names-WRONG", h), flush=True)
    os.remove(os.path.join(VOICE, "mixcut.wav"))
    print("worst %.3f" % worst)


def main(argv):
    if argv[1:2] == ["--mix"]:
        check_mix(argv[2], argv[3])
        return
    if argv[1:2] == ["--check"]:
        heard = transcribe(argv[2])
        print(round(score(argv[3], heard), 3), heard)
        return
    force = set(argv[2:]) if argv[1:2] == ["--force"] else set()
    spec = json.load(open(os.path.join(HERE, "narration.json")))
    voice = dict(spec["voice"], voice=os.environ["TTS_VOICE"])
    os.makedirs(VOICE, exist_ok=True)
    tpath = os.path.join(VOICE, "transcripts.json")
    heard = json.load(open(tpath)) if os.path.exists(tpath) else {}
    timing = {}
    ids = {seg["id"] for seg in spec["segments"]}
    heard = {k: v for k, v in heard.items() if k in ids}
    for seg in spec["segments"]:
        sid, text = seg["id"], seg["text"]
        path = os.path.join(VOICE, sid + ".wav")
        old = heard.get(sid, {})
        if (sid in force or not os.path.exists(path) or old.get("text") != text
                or old.get("voice") != voice["voice"]):
            best = None
            for attempt in range(6):
                clip = tts(voice, text)
                tmp = path + ".try"
                open(tmp, "wb").write(clip)
                h = transcribe(tmp)
                r = (heard_names(text, h), score(text, h))
                print("%-9s try %d  %.3f %s  %s" % (sid, attempt + 1, r[1], "names-ok" if r[0] else "names-WRONG", h), flush=True)
                if best is None or r > best[0]:
                    best = (r, h)
                    os.replace(tmp, path)
                if r[0] and r[1] >= 0.995:
                    break
                time.sleep(1)
            best = (best[0][1], best[1])
            heard[sid] = {"text": text, "voice": voice["voice"], "heard": best[1],
                          "score": round(best[0], 3)}
            json.dump(heard, open(tpath, "w"), indent=1)
        timing[sid] = measure(path)
        print("%-9s %.3f %5.2fs  %s" % (sid, heard[sid]["score"], timing[sid]["dur"], heard[sid]["heard"]))
    json.dump(timing, open(os.path.join(HERE, "timing.json"), "w"), indent=1)
    print("speech total %.1fs" % sum(t["speech"][1] - t["speech"][0] for t in timing.values()))


if __name__ == "__main__":
    main(sys.argv)
