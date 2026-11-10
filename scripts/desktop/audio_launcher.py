#!/usr/bin/env python3
import ctypes
import os
from pathlib import Path
import select
import signal
import subprocess
import sys
import tempfile
import time

PR_SET_CHILD_SUBREAPER = 36


class OwnedChild:
    def __init__(self, pid: int, started: int, descriptor: int):
        self.pid = pid
        self.started = started
        self.descriptor = descriptor

    def signal(self, number: int) -> None:
        try:
            signal.pidfd_send_signal(self.descriptor, number)
        except ProcessLookupError:
            pass

    def exited(self) -> bool:
        readiness = select.poll()
        readiness.register(self.descriptor, select.POLLIN)
        return bool(readiness.poll(0))

    def close(self) -> None:
        os.close(self.descriptor)


def identity(pid: int) -> tuple[int, int] | None:
    try:
        fields = Path(f"/proc/{pid}/stat").read_text().rsplit(") ", 1)[1].split()
        return int(fields[1]), int(fields[19])
    except (FileNotFoundError, ProcessLookupError):
        return None


def children(pid: int) -> list[int]:
    try:
        return [int(value) for value in Path(f"/proc/{pid}/task/{pid}/children").read_text().split()]
    except (FileNotFoundError, ProcessLookupError):
        return []


class AudioLauncher:
    def __init__(self):
        libc = ctypes.CDLL(None, use_errno=True)
        libc.prctl.argtypes = [ctypes.c_int, *([ctypes.c_ulong] * 4)]
        libc.prctl.restype = ctypes.c_int
        if libc.prctl(PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) != 0:
            raise OSError(ctypes.get_errno(), "Audio child supervision unavailable")
        if not hasattr(os, "pidfd_open") or not hasattr(signal, "pidfd_send_signal"):
            raise RuntimeError("Audio child ownership unavailable")
        self.owned: dict[int, OwnedChild] = {}
        self.processes: dict[int, subprocess.Popen] = {}
        self.interrupted = 0
        signal.signal(signal.SIGINT, self.interrupt)
        signal.signal(signal.SIGTERM, self.interrupt)

    def interrupt(self, number: int, _frame: object) -> None:
        self.interrupted = number

    def discover(self) -> None:
        for pid, child in list(self.owned.items()):
            if child.exited():
                self.owned.pop(pid).close()
        pending = [os.getpid()]
        visited = set()
        while pending:
            parent = pending.pop()
            if parent in visited:
                continue
            visited.add(parent)
            parent_identity = identity(parent)
            if parent != os.getpid() and (
                parent not in self.owned or parent_identity is None
                or parent_identity[1] != self.owned[parent].started
            ):
                continue
            for pid in children(parent):
                before = identity(pid)
                if before is None or before[0] != parent:
                    continue
                if pid in self.owned and self.owned[pid].started != before[1]:
                    self.owned.pop(pid).close()
                if pid not in self.owned:
                    try:
                        descriptor = os.pidfd_open(pid)
                    except ProcessLookupError:
                        continue
                    after = identity(pid)
                    if after != before or identity(parent) != parent_identity:
                        os.close(descriptor)
                        continue
                    self.owned[pid] = OwnedChild(pid, before[1], descriptor)
                pending.append(pid)

    def reap(self) -> bool:
        while True:
            try:
                pid, status = os.waitpid(-1, os.WNOHANG)
            except ChildProcessError:
                return False
            if pid == 0:
                return True
            if pid in self.processes:
                self.processes[pid].returncode = os.waitstatus_to_exitcode(status)
            child = self.owned.pop(pid, None)
            if child is not None:
                child.close()

    def launch(self, command: list[str], **options) -> subprocess.Popen:
        process = subprocess.Popen(command, process_group=0, **options)
        self.processes[process.pid] = process
        self.discover()
        return process

    def cleanup(self) -> None:
        started = time.monotonic()
        while True:
            self.discover()
            if not self.reap():
                for child in self.owned.values():
                    child.close()
                self.owned.clear()
                return
            terminating = time.monotonic() - started < 0.4
            for child in list(self.owned.values()):
                child.signal(signal.SIGCONT)
                child.signal(signal.SIGTERM if terminating else signal.SIGKILL)
            if time.monotonic() - started >= 2:
                raise RuntimeError("Owned audio cleanup deadline exceeded")
            time.sleep(0.01)

    def pulse_child(self, wrapper: subprocess.Popen, socket: Path) -> int | None:
        expected = f"--load=module-native-protocol-unix socket={socket} auth-anonymous=1".encode()
        self.discover()
        for pid in children(wrapper.pid):
            if pid not in self.owned:
                continue
            try:
                directory = Path(f"/proc/{pid}")
                if directory.stat().st_uid != 10001 or (directory / "comm").read_text().strip() != "pulseaudio":
                    continue
                if expected in (directory / "cmdline").read_bytes().split(b"\0"):
                    return pid
            except (FileNotFoundError, ProcessLookupError):
                continue
        return None

    def run(self, command: list[str], fault_control: bool, directory: Path) -> int:
        if self.interrupted:
            return 128 + self.interrupted
        environment = os.environ.copy()
        environment.pop("HITO_PRIVATE_PULSE_PID", None)
        runtime = directory / "runtime"
        runtime.mkdir(mode=0o700)
        for path in [directory, runtime]:
            os.chown(path, 10001, 10001)
        socket = directory / "pulse.sock"
        with (directory / "pulse.log").open("wb") as log:
            pulse = self.launch([
                "runuser", "-u", "hito-desktop", "--", "env", f"XDG_RUNTIME_DIR={runtime}",
                "pulseaudio", "-n", "--daemonize=no", "--exit-idle-time=-1", "--use-pid-file=no",
                "--log-target=stderr", "--load=module-null-sink sink_name=hito-test rate=48000 channels=2",
                f"--load=module-native-protocol-unix socket={socket} auth-anonymous=1",
            ], env=environment, stdout=log, stderr=subprocess.STDOUT)
        deadline = time.monotonic() + 5
        pulse_pid = None
        while not self.interrupted:
            self.discover()
            self.reap()
            if pulse.returncode is not None or time.monotonic() >= deadline:
                raise RuntimeError("Private audio backend could not start")
            if socket.is_socket():
                pulse_pid = self.pulse_child(pulse, socket)
                if pulse_pid is not None:
                    break
            time.sleep(0.01)
        if self.interrupted:
            return 128 + self.interrupted
        config = directory / "asound.conf"
        config.write_text("pcm.!default { type pulse }\nctl.!default { type pulse }\n")
        os.chown(config, 10001, 10001)
        settings = [f"PULSE_SERVER=unix:{socket}", f"ALSA_CONFIG_PATH={config}"]
        if fault_control:
            settings.append(f"HITO_PRIVATE_PULSE_PID={pulse_pid}")
        test = self.launch(["runuser", "-u", "hito-desktop", "--", "env", *settings, *command], env=environment)
        while not self.interrupted:
            self.discover()
            self.reap()
            if test.returncode is not None:
                return test.returncode if test.returncode >= 0 else 128 - test.returncode
            if pulse.returncode is not None:
                raise RuntimeError("Private audio backend stopped")
            time.sleep(0.01)
        return 128 + self.interrupted


def main(arguments: list[str]) -> int:
    fault_control = bool(arguments and arguments[0] == "--backend-fault-control")
    command = arguments[1:] if fault_control else arguments
    if not command:
        print("Audio fixture command is required", file=sys.stderr)
        return 2
    try:
        launcher = AudioLauncher()
    except (OSError, RuntimeError):
        print('{"event":"audio_fixture_ownership_unavailable"}', file=sys.stderr)
        return 1
    status = 1
    cleanup_failed = False
    with tempfile.TemporaryDirectory(prefix="hito-audio-") as temporary:
        try:
            status = launcher.run(command, fault_control, Path(temporary))
        except (OSError, RuntimeError) as error:
            print(f'{{"event":"audio_fixture_failed","error_kind":"{type(error).__name__}","errno":{getattr(error, "errno", 0) or 0}}}', file=sys.stderr)
        finally:
            try:
                launcher.cleanup()
            except (OSError, RuntimeError):
                print('{"event":"audio_fixture_cleanup_failed"}', file=sys.stderr)
                cleanup_failed = True
    if cleanup_failed:
        return 1
    return 128 + launcher.interrupted if launcher.interrupted else status


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
