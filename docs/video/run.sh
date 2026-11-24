#!/usr/bin/env bash
# Build the README video on a build machine, from this directory's files plus
# wallpaper.png (any wide wallpaper; the video used a dark 5120x1440 one), out/timeline.json
# and out/voice/*.wav.
# usage: run.sh sheet OUT.png T... | stills DIR T... | video [FROM TO] | loop | audio | mux | contact | review
set -euo pipefail
cd "$(dirname "$0")"
source ./env.sh
REG="$FD/JetBrainsMonoNerdFont-Regular.ttf"
BOLD="$FD/JetBrainsMonoNerdFont-Bold.ttf"
ARGS=(out/timeline.json events.json "${WALL:-wallpaper.png}" "$REG" "$BOLD")
FFQ=("$FF/ffmpeg" -hide_banner -loglevel error -y)
step="$1"
shift
case "$step" in
sheet)
  out="$1"
  shift
  "$PY" -I render.py sheet "${ARGS[@]}" "$out" "$@"
  ;;
stills)
  out="$1"
  shift
  mkdir -p "$out"
  "$PY" -I render.py stills "${ARGS[@]}" "$out" "$@"
  ;;
video)
  "$PY" -I render.py video "${ARGS[@]}" "$@" |
    "${FFQ[@]}" -f rawvideo -pix_fmt rgb24 -s 1920x1080 -r 60 -i - \
      -c:v libx264 -preset slow -crf 20 -tune animation -pix_fmt yuv420p -movflags +faststart out/video.mp4
  ;;
loop)
  "$PY" -I render.py loop "${ARGS[@]}" |
    "${FFQ[@]}" -f rawvideo -pix_fmt rgb24 -s 960x540 -r 30 -i - \
      -c:v libwebp_anim -loop 0 -q:v 72 -compression_level 6 -preset picture out/newm-rs-loop.webp
  ;;
audio)
  mkdir -p out/voice48
  for w in out/voice/*.wav; do
    id=$(basename "$w" .wav)
    "${FFQ[@]}" -i "$w" -ac 1 -ar 48000 -f f32le "out/voice48/$id.f32"
  done
  "$PY" -I music.py out/timeline.json out/voice48 out
  chain='alimiter=limit=0.7:attack=4:release=80:level=disabled'
  "$FF/ffmpeg" -hide_banner -nostats -f f32le -ar 48000 -ac 2 -i out/mix.f32 \
    -af "$chain,loudnorm=I=-14:TP=-1.5:LRA=11:print_format=json" -f null - 2>out/loudnorm.log
  m=$("$PY" -I -c 'import json,sys; t=open(sys.argv[1]).read(); d=json.loads(t[t.rindex("{"):t.rindex("}")+1]); print("measured_I=%s:measured_TP=%s:measured_LRA=%s:measured_thresh=%s:offset=%s" % (d["input_i"], d["input_tp"], d["input_lra"], d["input_thresh"], d["target_offset"]))' out/loudnorm.log)
  "${FFQ[@]}" -f f32le -ar 48000 -ac 2 -i out/mix.f32 \
    -af "$chain,loudnorm=I=-14:TP=-1.5:LRA=11:linear=true:$m,aresample=48000" -c:a pcm_s16le out/mix.wav
  "$FF/ffmpeg" -hide_banner -nostats -i out/mix.wav -af ebur128=peak=true -f null - 2>&1 | tail -12
  ;;
mux)
  "${FFQ[@]}" -i out/video.mp4 -i out/mix.wav -map 0:v -map 1:a -c:v copy -c:a aac -b:a 192k -ac 2 \
    -movflags +faststart out/newm-rs.mp4
  "$FF/ffprobe" -v error -show_entries format=duration,size -of default=nw=1 out/newm-rs.mp4
  ;;
contact)
  "${FFQ[@]}" -i out/newm-rs.mp4 -vf 'fps=1/10,scale=480:-1,tile=4x4' -frames:v 1 out/newm-rs-contact.png
  ;;
review)
  mkdir -p out/review
  "${FFQ[@]}" -i out/newm-rs.mp4 -vf "fps=1/2,scale=640:-1,drawtext=fontfile=$REG:text='%{pts}':x=8:y=8:fontsize=20:fontcolor=white:box=1:boxcolor=black@0.6,tile=4x5" out/review/r-%02d.png
  ;;
*)
  echo "unknown step: $step" >&2
  exit 2
  ;;
esac
