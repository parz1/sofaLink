"""Bounded native encoder recovery check; localhost, no TV/audio/global settings."""
import json
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def main():
    output = ROOT / "local/encoder-ab/idr-check.jsonl"
    output.parent.mkdir(parents=True, exist_ok=True)
    ready = threading.Event()
    session = "idr-check"
    events = []
    with output.open("w", encoding="utf-8") as log:
        process = subprocess.Popen([sys.executable, str(Path(__file__).with_name("host.py")),
            "--device", "127.0.0.1", "--transport", "udp", "--test-pattern", "--no-audio",
            "--encoder-profile", "high", "--gop-size", "-1", "--vbv-frames", "1",
            "--diagnostics", "--session-id", session, "--seconds", "6"],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding="utf-8")
        def consume():
            for line in process.stdout:
                log.write(line)
                event = json.loads(line)
                events.append(event)
                if event.get("type") == "host_ready": ready.set()
        reader = threading.Thread(target=consume)
        reader.start()
        try:
            if not ready.wait(20): raise RuntimeError("Host readiness timeout")
            with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
                def send(sequence, token=session):
                    sock.sendto(json.dumps({"type":"keyframe_request", "sessionId":token,
                        "sequence":sequence}).encode(), ("127.0.0.1", 5005))
                time.sleep(1)
                send(1, "stale-session")
                send(True)
                send(0)
                send(2)
                time.sleep(.1)
                send(3)  # Inside rate limit: cannot trigger another IDR.
                time.sleep(.7)
                send(2)  # Replay: cannot trigger another IDR.
                send(4)
            process.wait(timeout=20)
        finally:
            if process.poll() is None:
                process.terminate()
                process.wait(timeout=5)
            reader.join(timeout=5)
        errors = process.stderr.read()
    if process.returncode: raise RuntimeError(errors[-2000:])
    requests = [e for e in events if e.get("type") == "keyframe_requested"]
    if [e["sequence"] for e in requests] != [2, 4] or not all(e["accepted"] for e in requests):
        raise RuntimeError(f"Request validation failed: {requests}")
    keys = [row[0] for e in events if e.get("type") == "encoder_frames" for row in e["rows"] if row[3]]
    if len(keys) != 3 or keys[0] != 1:
        raise RuntimeError(f"Expected startup + two requested keyframes, got {keys}")
    print(json.dumps({"result":"PASS", "accepted_sequences":[2,4], "keyframe_indices":keys,
        "scope":"native localhost control and encode; not physical TV loss recovery", "log":str(output)}))


if __name__ == "__main__": main()
