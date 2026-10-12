"""Offline PNG diagnostics only; requires Pillow and NumPy, never runs in the stream."""
import argparse
import json
from pathlib import Path

import numpy as np
from PIL import Image


def temporal(files):
    images = [np.asarray(Image.open(path).convert("RGB"), dtype=np.int16) for path in files]
    if len(images) < 2:
        raise ValueError("At least two screenshots / decoded frames are required")
    if any(image.shape != (1080, 1920, 3) for image in images):
        raise ValueError("Expected 1920x1080 RGB images")
    frames = np.stack(images)
    span = frames.max(axis=0) - frames.min(axis=0)
    return {
        "frames": len(images),
        "maximum_channel_span": int(span.max()),
        "mean_channel_span": float(span.mean()),
        "changed_pixels": int(np.any(span > 0, axis=2).sum()),
        "fine_lines_mean_span": float(span[740:940, 240:1680].mean()),
        "solid_patch_maximum_span": int(span[220:420, 20:1900].max()),
        "red_center": frames[:, 300, 1320].tolist(),
        "green_center": frames[:, 300, 840].tolist(),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    result = {"scope": "digital pixel stability on a static chart; excludes physical HDMI/panel"}
    for name, pattern in (("client", "client*.png"), ("pc_decode", "decoded-*.png")):
        files = sorted(args.directory.glob(pattern))
        if files:
            result[name] = temporal(files)
    output = json.dumps(result, indent=2)
    (args.directory / "temporal.json").write_text(output, encoding="utf-8")
    print(output)


if __name__ == "__main__":
    main()
