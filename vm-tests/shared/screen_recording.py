import os
import socket
import subprocess
import threading
import time

RECORDING_FRAME_SECONDS = 0.5
RECORDING_CONNECT_SECONDS = 30
QEMU_PROMPT = b"(qemu) "


class ScreenRecording:
    def __init__(self, recorded_machine):
        self.recorded_machine = recorded_machine
        self.frames_directory = os.path.join(recorded_machine.out_dir, "recording")
        self.raw_frame = os.path.join(recorded_machine.state_dir, "recording-frame.ppm")
        self.stopping = threading.Event()
        self.thread = threading.Thread(target=self.record_frames, daemon=True)

    def start(self):
        os.makedirs(self.frames_directory, exist_ok=True)
        self.thread.start()

    def stop(self):
        self.stopping.set()
        self.thread.join()

    def connect_monitor(self):
        monitor_path = os.path.join(self.recorded_machine.state_dir, "recording-monitor")
        deadline = time.monotonic() + RECORDING_CONNECT_SECONDS
        while time.monotonic() < deadline and not self.stopping.is_set():
            monitor = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            try:
                monitor.connect(monitor_path)
                return monitor
            except OSError:
                monitor.close()
                time.sleep(0.1)
        return None

    def read_until_prompt(self, monitor):
        received = b""
        while not received.endswith(QEMU_PROMPT):
            chunk = monitor.recv(4096)
            if not chunk:
                raise ConnectionError("the recording monitor closed")
            received += chunk

    def save_frame(self, monitor, frame_number):
        monitor.sendall(f"screendump {self.raw_frame}\n".encode())
        self.read_until_prompt(monitor)
        frame_path = os.path.join(self.frames_directory, f"frame-{frame_number:05d}.png")
        unfinished_path = f"{frame_path}.unfinished"
        subprocess.run(f"pnmtopng '{self.raw_frame}' > '{unfinished_path}'", shell=True, check=True)
        os.replace(unfinished_path, frame_path)

    def record_frames(self):
        monitor = self.connect_monitor()
        if monitor is None:
            return
        try:
            self.read_until_prompt(monitor)
            frame_number = 0
            while not self.stopping.is_set():
                self.save_frame(monitor, frame_number)
                frame_number += 1
                self.stopping.wait(RECORDING_FRAME_SECONDS)
        except (OSError, ConnectionError, subprocess.CalledProcessError):
            pass
        finally:
            monitor.close()
            for leftover in os.listdir(self.frames_directory):
                if leftover.endswith(".unfinished"):
                    os.remove(os.path.join(self.frames_directory, leftover))
