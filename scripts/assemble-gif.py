#!/usr/bin/env python3
"""Assemble PNG frames into a GIF for the README demos.

Usage:
    python3 scripts/assemble-gif.py assets/frames/yang assets/demos/yang.gif
        [--fps 1.5] [--width 960] [--hold 2]

Frames are sorted by file name, so `01-file.png`, `02-hover.png`, ... gives the
intended order. `--hold` repeats the last frame, which is what lets a reader see
the end state before the loop restarts.

Requires Pillow, which is only needed to produce assets and is not a build
dependency.
"""

import argparse
import pathlib
import sys

try:
    from PIL import Image
except ImportError:  # pragma: no cover - the message is the point
    sys.exit("assemble-gif: Pillow is required (pip install pillow)")


def frame_paths(directory: pathlib.Path) -> list[pathlib.Path]:
    frames = sorted(p for p in directory.iterdir() if p.suffix.lower() == ".png")
    if not frames:
        sys.exit(f"assemble-gif: no PNG frames in {directory}")
    return frames


def load(path: pathlib.Path, width: int) -> Image.Image:
    image = Image.open(path).convert("RGB")
    if image.width > width:
        height = round(image.height * width / image.width)
        image = image.resize((width, height), Image.LANCZOS)
    return image


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("frames", type=pathlib.Path)
    parser.add_argument("output", type=pathlib.Path)
    parser.add_argument("--fps", type=float, default=1.5, help="frames per second")
    parser.add_argument("--width", type=int, default=960, help="downscale width")
    parser.add_argument("--hold", type=int, default=2, help="extra copies of the last frame")
    parser.add_argument("--loop", type=int, default=0, help="0 loops forever")
    args = parser.parse_args()

    images = [load(path, args.width) for path in frame_paths(args.frames)]
    images += [images[-1]] * max(args.hold, 0)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    # One adaptive palette across all frames keeps the colours stable and stops
    # the GIF from shimmering when it loops.
    palette = images[0].quantize(colors=128, method=Image.Quantize.MEDIANCUT)
    frames = [image.quantize(palette=palette, dither=Image.Dither.NONE) for image in images]
    frames[0].save(
        args.output,
        save_all=True,
        append_images=frames[1:],
        duration=round(1000 / args.fps),
        loop=args.loop,
        optimize=True,
    )
    size = args.output.stat().st_size
    print(f"assemble-gif: {args.output} ({len(images)} frames, {size // 1024} KiB)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
