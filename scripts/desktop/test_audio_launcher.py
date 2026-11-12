import ctypes
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest

ENTRY = Path(__file__).with_name("with-audio.sh").resolve()


def identity(pid):
    try:
        directory = Path(f"/proc/{pid}")
        fields = (directory / "stat").read_text().rsplit(") ", 1)[1].split()
        return {
            "pid": pid, "parent": int(fields[1]), "start": int(fields[19]), "state": fields[0],
            "name": (directory / "comm").read_text().strip(),
            "arguments": (directory / "cmdline").read_bytes().split(b"\0"),
        }
    except (FileNotFoundError, ProcessLookupError):
        return None


def family(parent):
    records = {}
    for path in Path("/proc").iterdir():
        if path.name.isdigit():
            record = identity(int(path.name))
            if record is not None:
                records[record["pid"]] = record
    members = {parent}
    while True:
        found = {pid for pid, record in records.items() if record["parent"] in members}
        if found <= members:
            return [record for pid, record in records.items() if pid in members and pid != parent]
        members |= found


def signal_owned(record, number):
    before = identity(record["pid"])
    if before is None or before["start"] != record["start"]:
        return
    try:
        descriptor = os.pidfd_open(record["pid"])
    except ProcessLookupError:
        return
    try:
        after = identity(record["pid"])
        if after is not None and after["start"] == record["start"]:
            try:
                signal.pidfd_send_signal(descriptor, number)
            except ProcessLookupError:
                pass
    finally:
        os.close(descriptor)


class AudioLauncherControls(unittest.TestCase):
    def setUp(self):
        libc = ctypes.CDLL(None, use_errno=True)
        self.assertEqual(libc.prctl(36, 1, 0, 0, 0), 0)
        self.temporary = tempfile.TemporaryDirectory(prefix="hito-launcher-control-")
        self.directory = Path(self.temporary.name)
        os.chown(self.directory, 10001, 10001)
        self.processes = []
        self.streams = []
        self.observed = {}

    def tearDown(self):
        deadline = time.monotonic() + 3
        while True:
            remaining = family(os.getpid())
            for record in remaining:
                signal_owned(record, signal.SIGCONT)
                signal_owned(record, signal.SIGKILL)
            try:
                while True:
                    pid, status = os.waitpid(-1, os.WNOHANG)
                    if pid == 0:
                        break
                    for process in self.processes:
                        if process.pid == pid:
                            process.returncode = os.waitstatus_to_exitcode(status)
            except ChildProcessError:
                break
            if time.monotonic() >= deadline:
                self.fail("control teardown retained owned child processes")
            time.sleep(0.01)
        for stream in self.streams:
            stream.close()
        self.temporary.cleanup()

    def launch(self, command, fault=False, environment=None):
        stream = (self.directory / f"child-{len(self.processes)}.log").open("w")
        self.streams.append(stream)
        arguments = ["sh", str(ENTRY)]
        if fault:
            arguments.append("--backend-fault-control")
        process = subprocess.Popen(
            [*arguments, *command], stdout=stream, stderr=subprocess.STDOUT,
            start_new_session=True, env=environment,
        )
        self.processes.append(process)
        return process

    def wait_for(self, process, condition):
        deadline = time.monotonic() + 5
        while True:
            records = family(process.pid)
            self.observed.update((record["pid"], record) for record in records)
            if condition(records):
                return records
            self.assertIsNone(process.poll(), "launcher exited before fixture readiness")
            self.assertLess(time.monotonic(), deadline, "fixture readiness deadline expired")
            time.sleep(0.005)

    def ready(self, process):
        return self.wait_for(process, lambda records: any(
            record["name"] == "sleep" and record["arguments"][1:2] == [b"30"]
            for record in records
        ) and any(record["name"] == "pulseaudio" for record in records))

    def assert_joined(self, process, status):
        self.assertEqual(process.wait(timeout=2), status)
        survivors = []
        for record in self.observed.values():
            current = identity(record["pid"])
            if current is not None and current["start"] == record["start"]:
                survivors.append((current["pid"], current["state"], current["name"]))
        self.assertEqual(survivors, [], "launcher retained captured children")
        self.assertEqual(family(os.getpid()), [], "launcher left adopted descendants")

    def cancel(self, process, number=signal.SIGTERM):
        started = time.monotonic()
        process.send_signal(number)
        self.assert_joined(process, 128 + number)
        self.assertLess(time.monotonic() - started, 2)

    def test_default_success_and_environment(self):
        environment = os.environ.copy()
        environment["HITO_PRIVATE_PULSE_PID"] = "1"
        marker = self.directory / "command-ran"
        command = 'test -z "${HITO_PRIVATE_PULSE_PID:-}" && test -S "${PULSE_SERVER#unix:}" && printf ready > "$1"'
        process = self.launch(["sh", "-c", command, "owned-command", str(marker)], environment=environment)
        self.assert_joined(process, 0)
        self.assertTrue(marker.exists(), "the child must verify the actual private socket")

    def test_nonzero_command_status(self):
        process = self.launch(["sh", "-c", "exit 17"], fault=True)
        self.assert_joined(process, 17)

    def test_failed_command_reaps_background_descendants(self):
        process = self.launch(["sh", "-c", "sleep 30 & exit 101"], fault=True)
        self.assert_joined(process, 101)

    def test_default_long_command_term(self):
        process = self.launch(["sleep", "30"])
        self.ready(process)
        self.cancel(process)

    def test_default_long_command_int(self):
        process = self.launch(["sleep", "30"])
        self.ready(process)
        self.cancel(process, signal.SIGINT)

    def test_nested_command_cancellation(self):
        process = self.launch(["sh", "-c", "sleep 30 & wait"], fault=True)
        self.ready(process)
        self.cancel(process)

    def test_descendant_in_another_session(self):
        process = self.launch(["sh", "-c", 'setsid sh -c "sleep 30 & wait" & wait'], fault=True)
        self.ready(process)
        self.cancel(process)

    def test_stopped_backend_and_wrapper_cancellation(self):
        process = self.launch(["sleep", "30"], fault=True)
        records = self.ready(process)
        pulse = next(record for record in records if record["name"] == "pulseaudio")
        wrapper = next(record for record in records if record["pid"] == pulse["parent"])
        self.assertEqual(wrapper["name"], "runuser")
        signal_owned(pulse, signal.SIGSTOP)
        signal_owned(wrapper, signal.SIGSTOP)
        self.wait_for(process, lambda records: all(any(
            record["pid"] == stopped["pid"] and record["state"] == "T" for record in records
        ) for stopped in [pulse, wrapper]))
        self.cancel(process)

    def test_term_resistant_descendants_are_killed_and_reaped(self):
        marker = self.directory / "term-resistant-ready"
        command = 'trap "" TERM HUP INT; printf ready > "$1"; sleep 30 & wait'
        process = self.launch(["sh", "-c", command, "owned-command", str(marker)], fault=True)
        self.ready(process)
        self.wait_for(process, lambda _records: marker.exists())
        self.cancel(process)

    def interrupt_cleanup(self, number):
        ready = self.directory / "cleanup-helper-ready"
        marker = self.directory / "cleanup-started"
        helper = (
            "import signal,time,sys\nfrom pathlib import Path\n"
            "marker=Path(sys.argv[2])\n"
            "signal.signal(signal.SIGTERM, lambda *_: marker.write_text('cleanup'))\n"
            "Path(sys.argv[1]).write_text('ready')\n"
            "while True: time.sleep(.01)\n"
        )
        command = (
            "import subprocess,sys,time\nfrom pathlib import Path\n"
            "subprocess.Popen([sys.executable,'-c',sys.argv[1],sys.argv[2],sys.argv[3]])\n"
            "ready=Path(sys.argv[2]);deadline=time.monotonic()+4\n"
            "while not ready.exists():\n"
            "    if time.monotonic()>deadline: raise RuntimeError('helper readiness')\n"
            "    time.sleep(.005)\n"
        )
        process = self.launch([sys.executable, "-c", command, helper, str(ready), str(marker)])
        self.wait_for(process, lambda _records: marker.exists())
        self.cancel(process, number)

    def test_term_during_cleanup_preserves_interruption_status(self):
        self.interrupt_cleanup(signal.SIGTERM)

    def test_int_during_cleanup_preserves_interruption_status(self):
        self.interrupt_cleanup(signal.SIGINT)

    def test_reaped_descendants_release_observation_descriptors(self):
        ready = self.directory / "descriptor-ready"
        proceed = self.directory / "descriptor-proceed"
        done = self.directory / "descriptor-done"
        command = (
            "from pathlib import Path; import subprocess,sys,time; "
            "base=Path(sys.argv[1]); (base/'descriptor-ready').touch(); "
            "exec(\"while not (base/'descriptor-proceed').exists(): time.sleep(.005)\"); "
            "[subprocess.run(['sleep','.02'],check=True) for _ in range(80)]; "
            "(base/'descriptor-done').touch(); time.sleep(30)"
        )
        process = self.launch([sys.executable, "-c", command, str(self.directory)])
        self.wait_for(process, lambda _records: ready.exists())
        descriptors = Path(f"/proc/{process.pid}/fd")
        before = len(list(descriptors.iterdir()))
        proceed.touch()
        self.wait_for(process, lambda _records: done.exists())
        deadline = time.monotonic() + 0.5
        while len(list(descriptors.iterdir())) > before + 8 and time.monotonic() < deadline:
            time.sleep(0.01)
        self.assertLessEqual(len(list(descriptors.iterdir())), before + 8)
        self.cancel(process)

    def test_cancel_before_backend_readiness(self):
        marker = self.directory / "startup-ready"
        binaries = self.directory / "bin"
        binaries.mkdir()
        pulse = binaries / "pulseaudio"
        pulse.write_text(
            f"#!{sys.executable}\nimport os,time\nfrom pathlib import Path\n"
            'Path(os.environ["HITO_LAUNCHER_START_READY"]).write_text("owned startup child")\n'
            "while True: time.sleep(1)\n"
        )
        pulse.chmod(0o755)
        environment = os.environ.copy()
        environment["PATH"] = str(binaries) + os.pathsep + environment["PATH"]
        environment["HITO_LAUNCHER_START_READY"] = str(marker)
        process = self.launch(["sleep", "30"], environment=environment)
        records = self.wait_for(process, lambda _records: marker.exists())
        self.assertFalse(any(record["name"] == "sleep" for record in records))
        self.cancel(process)


if __name__ == "__main__":
    unittest.main()
