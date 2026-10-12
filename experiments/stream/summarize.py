"""Summarize every second, including stalls; only trim startup/shutdown boundaries."""
import json
import statistics
import sys
from pathlib import Path

rows = [json.loads(line) for line in Path(sys.argv[1]).read_text(encoding="utf-8").splitlines() if line.strip()]
start = next(x["host_monotonic_ns"] for x in rows if x["type"] == "configuration")
end = max(x["host_monotonic_ns"] for x in rows)
steady = [x for x in rows if start + 3e9 <= x["host_monotonic_ns"] <= end - 1e9]
clients = [x["metrics"] for x in steady if x["type"] == "client"]
hosts = [x for x in steady if x["type"] == "host"]
def summary(values):
    values = sorted(values)
    return {"median": round(statistics.median(values),2), "min": round(min(values),2), "max": round(max(values),2)} if values else None
last = next((x["metrics"] for x in reversed(rows) if x["type"] == "client"), {})
first = next((x["metrics"] for x in rows if x["type"] == "client"), {})
result = {
    "measured_seconds": round((end-start)/1e9,2), "steady_client_samples": len(clients),
    "client_fps": summary([x["fps"] for x in clients]),
    "client_seconds_below_50fps": sum(x["fps"]<50 for x in clients),
    "receive_to_decode_ms_per_second_p50": summary([x["receiveToDecodeP50Ms"] for x in clients if x.get("receiveToDecodeP50Ms",0)>0]),
    "receive_to_render_ms_per_second_p50": summary([x["receiveToRenderP50Ms"] for x in clients if x["receiveToRenderP50Ms"]>0]),
    "receive_to_render_ms_per_second_p95": summary([x["receiveToRenderP95Ms"] for x in clients if x["receiveToRenderP95Ms"]>0]),
    "encoder_ms_per_second_p50": summary([x["encoder_ms_p50"] for x in hosts if x["encoder_ms_p50"] is not None]),
    "counter_deltas": {key: last.get(key,0)-first.get(key,0) for key in ("rendered","receivedFrames","lostPackets","damagedFrames","queueDrops")},
    "scope": "Per-second medians are not pooled frame percentiles. Client timing excludes PC capture/encode, preceding network, HDMI/TV. Glass-to-glass remains unmeasured."
}
print(json.dumps(result,indent=2))
