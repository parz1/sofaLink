"""Offline encoder burst diagnostics. Frame size does not measure QP or visual quality."""
import argparse
import csv
import json
from collections import defaultdict
from pathlib import Path


def percentile(values, fraction):
    values = sorted(values)
    return values[min(len(values) - 1, int((len(values) - 1) * fraction))] if values else None


def analyze(path):
    sessions = defaultdict(lambda: {"configuration": {}, "formats": [], "frames": {}, "trace_drops": 0})
    invalid = 0
    duplicates = 0
    with path.open(encoding="utf-8") as source:
        for line in source:
            try:
                event = json.loads(line)
                session = event["session_id"]
                if not isinstance(session, str) or not session:
                    raise ValueError("invalid session")
                data = sessions[session]
                kind = event.get("type")
                if kind == "configuration":
                    data["configuration"] = {key: event.get(key) for key in (
                        "fps", "encoder_profile", "gop_size", "vbv_frames", "pipeline")}
                elif kind == "encoder_format":
                    data["formats"].append(event.get("caps"))
                elif kind == "host_diagnostics":
                    data["trace_drops"] = max(data["trace_drops"], event.get("trace_drops", 0))
                elif kind == "encoder_frames":
                    for row in event["rows"]:
                        if (not isinstance(row, list) or len(row) != 4
                                or any(type(value) is not int for value in row)
                                or row[0] <= 0 or row[1] < 0 or row[2] <= 0 or row[3] not in (0, 1)):
                            invalid += 1
                            continue
                        if row[0] in data["frames"]:
                            duplicates += 1
                        data["frames"][row[0]] = row
            except (ValueError, KeyError, TypeError):
                invalid += 1
    result = {"scope": "compressed frame sizes and cadence; cannot prove QP changes or physical flicker",
              "invalid_records": invalid, "duplicate_frames": duplicates, "sessions": {}}
    all_rows = []
    for session, data in sessions.items():
        frames = sorted(data["frames"].values())
        if not frames:
            continue
        keyframes = [row for row in frames if row[3]]
        deltas = [row[2] for row in frames if not row[3]]
        key_sizes = [row[2] for row in keyframes]
        intervals = [b[0] - a[0] for a, b in zip(keyframes, keyframes[1:])]
        median_delta = percentile(deltas, .5)
        result["sessions"][session] = {
            "configuration": data["configuration"], "negotiated_formats": data["formats"],
            "frames": len(frames), "keyframes": len(keyframes),
            "missing_frame_records": frames[-1][0] - frames[0][0] + 1 - len(frames),
            "trace_drops": data["trace_drops"], "keyframe_intervals_frames": sorted(set(intervals)),
            "keyframe_bytes_p50": percentile(key_sizes, .5), "keyframe_bytes_p95": percentile(key_sizes, .95),
            "delta_bytes_p50": median_delta, "delta_bytes_p95": percentile(deltas, .95),
            "keyframe_p95_to_delta_p50_ratio": percentile(key_sizes, .95) / median_delta if key_sizes and median_delta else None,
        }
        all_rows.extend([[session, *row] for row in frames])
    return result, all_rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("log", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    result, rows = analyze(args.log)
    if not rows:
        parser.error("No encoder_frames records; sample with --diagnostics using the updated host")
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "encoder.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
    with (args.out / "encoder.csv").open("w", encoding="utf-8", newline="") as target:
        writer = csv.writer(target)
        writer.writerow(["session_id", "frame_index", "encoded_pts_ns", "bytes", "keyframe"])
        writer.writerows(rows)
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
