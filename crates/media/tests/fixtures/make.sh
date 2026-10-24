#!/bin/sh
# Regenerates the media test fixtures. Every input is synthesised here (a 440 Hz sine and
# a moving colour gradient), so the files are contributor-original (CC0-1.0); see
# ATTRIBUTION.md. The encoders only transcode our own signal. WAV and AIFF fixtures are written
# by the tests themselves.
set -e
cd "$(dirname "$0")"
FF="ffmpeg -hide_banner -loglevel error -y"
TONE="-f lavfi -i sine=frequency=440:sample_rate=48000:duration=0.5"
TONE44="-f lavfi -i sine=frequency=440:sample_rate=44100:duration=0.5"
META="-map_metadata -1 -fflags +bitexact -flags:v +bitexact -flags:a +bitexact"
$FF $TONE44 -c:a libmp3lame -b:a 64k $META tone.mp3
$FF $TONE44 -c:a aac -b:a 64k $META tone.m4a
$FF $TONE44 -c:a aac -b:a 64k $META -f adts tone.aac
$FF $TONE44 -c:a alac $META tone-alac.m4a
$FF $TONE44 -c:a flac $META tone.flac
$FF $TONE44 -c:a vorbis -strict -2 -ac 2 $META tone.ogg
$FF $TONE -c:a libopus -b:a 32k $META tone.opus
$FF $TONE44 -c:a wmav2 -b:a 64k $META tone.wma
# Video: 64x48 (AV1: 64x64), 10 fps, 6 frames of our own moving colour gradient (raw RGB from
# python3 below), so every pixel is contributor-original.
frames() {
  width="$1" height="$2" out="$3"
  python3 -c "
import sys
w, h = $width, $height
for f in range(6):
    sys.stdout.buffer.write(bytes(v for y in range(h) for x in range(w) for v in ((x * 4 + f * 20) % 256, (y * 5) % 256, (f * 40) % 256)))
" > "$out"
}
frames 64 48 /tmp/deckcraft-fixture-64x48.rgb
frames 64 64 /tmp/deckcraft-fixture-64x64.rgb
VID="-f rawvideo -pix_fmt rgb24 -s 64x48 -r 10 -i /tmp/deckcraft-fixture-64x48.rgb"
$FF $VID $TONE44 -c:v libx264 -pix_fmt yuv420p -g 5 -bf 2 -c:a aac -b:a 48k -shortest $META clip-h264.mp4
$FF $VID -c:v libx264 -pix_fmt yuv420p -g 5 -bf 0 $META clip-h264.mov
$FF $VID -c:v libx265 -pix_fmt yuv420p -x265-params log-level=error:keyint=5 -tag:v hvc1 $META clip-hevc.mp4
$FF $VID $TONE -c:v libvpx-vp9 -pix_fmt yuv420p -g 5 -b:v 100k -c:a libopus -b:a 32k -shortest $META clip-vp9.webm
$FF -f rawvideo -pix_fmt rgb24 -s 64x64 -r 10 -i /tmp/deckcraft-fixture-64x64.rgb -c:v libsvtav1 -pix_fmt yuv420p -g 5 -svtav1-params loglevel=0 $META clip-av1.mkv
