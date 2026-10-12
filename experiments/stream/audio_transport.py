"""Bounded PCM delivery. Wire: LHPA, byte count, media PTS ns, RTP->media offset ns, S16LE."""
import queue
import socket
import struct
import threading
import time
from collections import deque


class PcmServer:
    def __init__(self, device, port=5006, diagnostics=False):
        self.device = str(device)
        self.queue = queue.Queue(maxsize=4)
        self.stopping = threading.Event()
        self.frames = self.bytes = self.drops = 0
        self.connected = False
        self.error = ""
        self.diagnostics = diagnostics
        self.timings_lock = threading.Lock()
        self.queue_ms = deque(maxlen=512)
        self.send_ms = deque(maxlen=512)
        self.listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.listener.bind(("0.0.0.0", port))
        self.listener.listen(1)
        self.listener.settimeout(.25)
        self.client = None
        self.thread = threading.Thread(target=self.run, daemon=True)
        self.thread.start()

    def offer(self, pcm, pts, video_offset):
        # Never block the native capture pipeline behind a slow receiver.
        packet = struct.pack(">4sIQq", b"LHPA", len(pcm), pts, video_offset) + pcm
        packet = (time.perf_counter_ns() if self.diagnostics else 0, packet)
        try:
            self.queue.put_nowait(packet)
        except queue.Full:
            try:
                self.queue.get_nowait()
            except queue.Empty:
                pass
            self.drops += 1
            try:
                self.queue.put_nowait(packet)
            except queue.Full:
                self.drops += 1

    def run(self):
        while not self.stopping.is_set():
            try:
                client, remote = self.listener.accept()
                if remote[0] != self.device:
                    client.close()
                    continue
                self.client = client
                # A new subscriber starts at live media, never at a pre-connection backlog.
                while True:
                    try: self.queue.get_nowait()
                    except queue.Empty: break
                client.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
                client.setsockopt(socket.SOL_SOCKET, socket.SO_SNDBUF, 32768)
                client.settimeout(2)
                self.connected = True
                while not self.stopping.is_set():
                    try:
                        queued_ns, packet = self.queue.get(timeout=.25)
                    except queue.Empty:
                        continue
                    before = time.perf_counter_ns() if self.diagnostics else 0
                    client.sendall(packet)
                    if self.diagnostics:
                        after = time.perf_counter_ns()
                        with self.timings_lock:
                            self.queue_ms.append((before-queued_ns)/1e6)
                            self.send_ms.append((after-before)/1e6)
                    self.frames += 1
                    self.bytes += len(packet)-24
            except (OSError, TimeoutError) as error:
                if self.connected and not self.stopping.is_set(): self.error = str(error)
            finally:
                self.connected = False
                if self.client:
                    self.client.close()
                    self.client = None

    def timing_stats(self):
        with self.timings_lock:
            result = {}
            for name, values in (("audio_queue_ms",self.queue_ms),("audio_send_ms",self.send_ms)):
                samples = sorted(values)
                result[name+"_p95"] = samples[min(len(samples)-1,int(len(samples)*.95))] if samples else None
                result[name+"_max"] = samples[-1] if samples else None
                values.clear()
            return result

    def close(self):
        self.stopping.set()
        self.listener.close()
        if self.client:
            try:
                self.client.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
            self.client.close()
        self.thread.join(timeout=1)
