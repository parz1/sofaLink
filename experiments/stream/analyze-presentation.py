"""Join independent SurfaceFlinger fences to unchanged client audio targets.

Usage: python analyze-presentation.py session.jsonl surface-latency.txt output.json
SurfaceFlinger present fences describe compositor output, not physical panel light.
"""
import csv
import json
import pathlib
import sys

def analyze(log, surface, output):
    rows = {}
    for line in pathlib.Path(log).read_text(encoding="utf-8-sig").splitlines():
        record = json.loads(line)
        if record.get("type") == "client_frames":
            for row in record.get("trace", {}).get("rows", []):
                if len(row) > 12 and row[11] == 0 and row[12] > 0:
                    rows[row[12]] = row
    joined = []
    valid_fences = 0
    for line in pathlib.Path(surface).read_text(encoding="utf-8-sig").splitlines():
        cols = line.split()
        if len(cols) != 3:
            continue
        desired, present, ready = map(int, cols)
        if min(desired, present, ready) <= 0 or max(desired, present, ready) >= 2**63-1:
            continue
        valid_fences += 1
        row = rows.get(desired)
        if row:
            joined.append({"rtp": row[0], "audio_target_ns": row[5], "surface_request_ns": desired,
                           "compositor_present_ns": present, "codec_report_ns": row[6],
                           "compositor_minus_audio_ms": (present-row[5])/1e6,
                           "codec_minus_compositor_ms": (row[6]-present)/1e6,
                           "compositor_minus_request_ms": (present-desired)/1e6})
    summary = {"valid_fences": valid_fences, "matched_frames": len(joined), "boundary": "SurfaceFlinger present fence; physical panel unmeasured"}
    for key in ("compositor_minus_audio_ms", "codec_minus_compositor_ms", "compositor_minus_request_ms"):
        values = sorted(r[key] for r in joined)
        if values:
            summary[key] = {"p50": values[round((len(values)-1)*.5)], "p95": values[round((len(values)-1)*.95)], "min": values[0], "max": values[-1]}
    absolute = sorted(abs(r["compositor_minus_audio_ms"]) for r in joined)
    if absolute:
        summary["absolute_compositor_audio_error_ms"] = {"p50": absolute[round((len(absolute)-1)*.5)], "p95": absolute[round((len(absolute)-1)*.95)]}
    pathlib.Path(output).write_text(json.dumps(summary, indent=2), encoding="utf-8")
    if joined:
        with pathlib.Path(output).with_suffix(".csv").open("w", newline="", encoding="utf-8") as file:
            writer = csv.DictWriter(file, fieldnames=joined[0].keys())
            writer.writeheader(); writer.writerows(joined)
    print(json.dumps(summary, indent=2))

if __name__ == "__main__":
    analyze(*sys.argv[1:])
