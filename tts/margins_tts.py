"""Neural narration for Margins: renders a paper to speech with Kokoro-82M on Apple silicon (MLX).

Usage:  margins_tts.py --voice af_heart --out DIR < segments.json

segments.json: [{"id": "...", "text": "...", "kind": "heading|text|caption"}, ...]

Writes DIR/narration.wav (24 kHz mono) and DIR/narration.json:
    {"voice": "...", "segments": [{"id", "kind", "start", "end"}, ...]}   (seconds)
Prints "PROGRESS i n" lines so the app can show progress. Runs entirely on this Mac.
"""
import argparse, json, os, sys, wave

# misaki's bundled espeak-ng has a broken data path on macOS; use Homebrew's instead
import espeakng_loader
for prefix in ("/opt/homebrew", "/usr/local"):
    if os.path.exists(f"{prefix}/lib/libespeak-ng.dylib"):
        espeakng_loader.get_library_path = lambda p=prefix: f"{p}/lib/libespeak-ng.dylib"
        espeakng_loader.get_data_path = lambda p=prefix: f"{p}/share/espeak-ng-data"
        break

import numpy as np
from mlx_audio.tts.utils import load_model

MODEL = "prince-canuma/Kokoro-82M"
RATE = 24000
PAUSE = {"heading": (0.55, 0.45), "text": (0.0, 0.35), "caption": (0.2, 0.4)}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--voice", default="af_heart")
    ap.add_argument("--speed", type=float, default=1.0)
    ap.add_argument("--out", required=True)
    args = ap.parse_args()
    segments = json.load(sys.stdin)
    os.makedirs(args.out, exist_ok=True)
    model = load_model(MODEL)
    lang = args.voice[0]  # a = American English, b = British English

    timings, t = [], 0.0
    with wave.open(os.path.join(args.out, "narration.wav"), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)

        def silence(sec):
            w.writeframes(b"\x00\x00" * int(sec * RATE))

        for i, seg in enumerate(segments):
            before, after = PAUSE.get(seg.get("kind", "text"), PAUSE["text"])
            silence(before)
            t += before
            chunks = [np.array(r.audio).reshape(-1) for r in
                      model.generate(text=seg["text"], voice=args.voice, speed=args.speed, lang_code=lang)]
            audio = np.concatenate(chunks) if chunks else np.zeros(1, dtype=np.float32)
            w.writeframes((np.clip(audio, -1, 1) * 32767).astype("<i2").tobytes())
            dur = len(audio) / RATE
            timings.append({"id": seg["id"], "kind": seg.get("kind", "text"), "start": round(t, 3), "end": round(t + dur, 3)})
            t += dur
            silence(after)
            t += after
            print(f"PROGRESS {i + 1} {len(segments)}", flush=True)

    with open(os.path.join(args.out, "narration.json"), "w") as f:
        json.dump({"voice": args.voice, "model": MODEL, "segments": timings}, f)


if __name__ == "__main__":
    main()
