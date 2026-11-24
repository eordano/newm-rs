# The README video

`docs/video/` builds the video at the top of the README: about 2:35 at 1920×1080 and 60 fps
(`out/newm-rs.mp4`), plus a silent 960×540 loop to play inline (`out/newm-rs-loop.webp`).
Neither is committed; `out/` stays out of git.

The canvas in the video replays the [usage log](session.md#the-usage-log) of a real newm-rs
session (a `telemetry` build), stripped down to window kinds and tiles. No app IDs or titles reach the video, and no key names
or action labels appear on screen.

## Files

| File | What it is |
|---|---|
| `SCRIPT.md` | The script: the principles, the features, the narration as spoken. |
| `sanitize.py` | Strips a `usage.jsonl` down to geometry: op kinds, times, tile rectangles, generic window kinds and the camera, keeping only the stretches `render.py` replays. Layout timings survive only as a median, a count and a histogram. Writes `events.json`. |
| `narration.json` | The narration text, one segment per beat. |
| `narrate.py` | Fetches one text-to-speech clip per segment, checks each with a speech-to-text transcript (up to six tries), and writes `out/voice/*.wav` and `timing.json`. |
| `timeline.py` | Lays the clips on the music's 120 BPM beat grid. Writes `out/timeline.json`. |
| `render.py` | Draws the frames as raw RGB on stdout. |
| `music.py` | Generates the music in numpy (no samples) and ducks it under the voice. |
| `run.sh` | Runs the steps; `env.sh` fetches Python with Pillow and NumPy, the JetBrains Mono Nerd Font and ffmpeg through Nix. |

## Building

```sh
cd docs/video
python3 -I sanitize.py /path/to/usage.jsonl events.json
python3 -I narrate.py
python3 -I timeline.py
./run.sh video     # out/video.mp4
./run.sh loop      # out/newm-rs-loop.webp
./run.sh audio     # out/mix.wav, loudness-normalized
./run.sh mux       # out/newm-rs.mp4
```

`run.sh` also has `sheet OUT.png T...` and `stills DIR T...` for frames at given times, and
`contact` and `review` for overview sheets of the finished video.

## Environment

| Variable | Used by | Meaning |
|---|---|---|
| `WALL` | `run.sh` | Wallpaper image; `wallpaper.png` in `docs/video/` by default. |
| `TTS_URL` | `narrate.py` | Base URL of an OpenAI-compatible `/v1` API for speech and transcription; `http://localhost:8000/v1` by default. |
| `TTS_VOICE` | `narrate.py` | The speech voice. Required. |
| `STT_MODEL` | `narrate.py` | The transcription model; `faster-whisper-large-v3-turbo` by default. |
| `LLM_API_KEY` | `narrate.py` | API key, sent only as a header. Required. |
