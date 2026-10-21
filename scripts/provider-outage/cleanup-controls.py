#!/usr/bin/env python3
"""Exercise exact cleanup against disposable Docker objects, without an app."""

import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import re
import secrets
import signal
import sys
import time

sys.dont_write_bytecode = True
SPEC = importlib.util.spec_from_file_location(
    "provider_outage_runner", Path(__file__).with_name("run-packaged.py")
)
runner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runner)
COMMAND = runner.command
CASES = {
    "ordinary", "absent-race", "wrong-labels", "remove-fault", "incoming-nonzero",
    "null-options", "signal-term", "signal-hup",
}


class Control:
    def __init__(self, case):
        self.case = case
        self.gate = runner.Gate()
        self.owner = secrets.token_hex(16)
        self.containers = {}
        self.volumes = {}
        self.interposed = False
        self.removals = []

    def prepare(self):
        image = os.environ["HITO_OUTAGE_NODE_IMAGE"]
        if not re.fullmatch(r"sha256:[0-9a-f]{64}", image):
            raise ValueError("Immutable control image is required")
        [found] = json.loads(COMMAND(["docker", "image", "inspect", image]))
        if found["Id"] != image or found.get("Os") != "linux" or found.get("Architecture") != "arm64":
            raise ValueError("Exact control image changed")
        labels = {
            "org.hito.owned-fixture": "provider-outage",
            "org.hito.fixture-run": self.gate.run_id,
            "org.hito.cleanup-control": self.owner,
        }
        services = {}
        for service in ("outage-keeper", "outage-legacy", "outage-current"):
            own_labels = dict(labels)
            if self.case == "wrong-labels" and service == "outage-current":
                own_labels["org.hito.fixture-run"] = f"foreign-{self.owner}"
            services[service] = {
                "image": image, "user": "10001:10001", "read_only": True,
                "network_mode": "none", "cap_drop": ["ALL"],
                "security_opt": ["no-new-privileges:true"],
                "labels": own_labels, "mem_limit": "64m", "pids_limit": 32,
                "command": ["node", "-e", "setInterval(() => {}, 60000)"],
                "logging": {"driver": "json-file", "options": {"max-size": "64k", "max-file": "1"}},
            }
        services["outage-keeper"]["volumes"] = ["outage-data:/data:ro", "outage-control:/control:ro"]
        volumes = {}
        for suffix in ("data", "control"):
            own_labels = dict(labels)
            if self.case == "wrong-labels" and suffix == "control":
                own_labels["org.hito.fixture-run"] = f"foreign-{self.owner}"
            volumes[f"outage-{suffix}"] = {
                "name": f"{self.gate.project}-{suffix}", "driver": "local",
                "driver_opts": {"type": "tmpfs", "device": "tmpfs", "o": "size=1m,uid=10001,gid=10001,mode=0700"},
                "labels": own_labels,
            }
            if self.case == "null-options" and suffix == "data":
                del volumes[f"outage-{suffix}"]["driver_opts"]
        config = self.gate.directory / "cleanup-compose.json"
        config.write_text(json.dumps({"services": services, "volumes": volumes}))
        self.compose = ["docker", "compose", "-p", self.gate.project, "-f", str(config)]
        COMMAND([*self.compose, "up", "-d", "--no-build", "--pull", "never"], seconds=45)
        for service in services:
            identifier = COMMAND([*self.compose, "ps", "-aq", service]).decode().strip()
            [container] = json.loads(COMMAND(["docker", "inspect", identifier]))
            self.verify_owner(container["Config"]["Labels"])
            if not re.fullmatch(r"[0-9a-f]{64}", container["Id"]):
                raise ValueError("Control immutable identity rejected")
            host = container["HostConfig"]
            if (
                container["Image"] != image or container["Config"].get("User") != "10001:10001"
                or not host.get("ReadonlyRootfs") or host.get("NetworkMode") != "none"
                or host.get("Memory") != 64 * 1024 * 1024 or host.get("PidsLimit") != 32
                or host.get("CapDrop") != ["ALL"] or host.get("PortBindings")
                or "no-new-privileges:true" not in host.get("SecurityOpt", [])
                or any(mount.get("Type") != "volume" or mount.get("Name") not in {
                    f"{self.gate.project}-data", f"{self.gate.project}-control"
                } or mount.get("RW") for mount in container.get("Mounts", []))
            ):
                raise ValueError("Actual lightweight cleanup fixture isolation changed")
            self.containers[service] = container["Id"]
        for suffix in ("data", "control"):
            name = f"{self.gate.project}-{suffix}"
            [volume] = json.loads(COMMAND(["docker", "volume", "inspect", name]))
            self.verify_owner(volume["Labels"])
            if self.case == "null-options" and suffix == "data" and volume.get("Options") is not None:
                raise ValueError("Actual Docker null volume options were not reached")
            self.volumes[suffix] = name

    def verify_owner(self, labels):
        if (
            labels.get("com.docker.compose.project") != self.gate.project
            or labels.get("org.hito.cleanup-control") != self.owner
        ):
            raise ValueError("Independent cleanup control ownership changed")

    def absent_container(self, identifier):
        found = COMMAND(["docker", "ps", "-aq", "--no-trunc", "--filter", f"id={identifier}"])
        return not found.strip()

    def remove_control_container(self, identifier):
        if self.absent_container(identifier):
            return
        [container] = json.loads(COMMAND(["docker", "inspect", identifier]))
        self.verify_owner(container["Config"]["Labels"])
        if container["Id"] != identifier:
            raise ValueError("Independent control immutable identity changed")
        COMMAND(["docker", "rm", "-f", identifier])

    def interpose(self, arguments, *args, **kwargs):
        target = self.containers["outage-legacy"]
        if (
            self.case == "absent-race" and not self.interposed
            and arguments[:2] == ["docker", "inspect"]
            and target.startswith(arguments[2])
        ):
            self.remove_control_container(target)
            self.interposed = True
        if arguments[:3] == ["docker", "rm", "-f"]:
            self.removals.append(arguments[3])
            if (
                self.case in {"remove-fault", "incoming-nonzero"}
                and not self.interposed and arguments[3] == target
            ):
                self.interposed = True
                raise ValueError("Deliberate exact cleanup removal fault")
        return COMMAND(arguments, *args, **kwargs)

    def exercise(self):
        runner.command = self.interpose
        failed = False
        try:
            if self.case == "incoming-nonzero":
                self.check_incoming_status()
                failed = True
            else:
                try:
                    self.gate.cleanup()
                except AttributeError:
                    if self.case == "null-options":
                        raise ValueError("Actual null options aborted independent volume cleanup") from None
                    raise
                except (OSError, ValueError, KeyError, TypeError, runner.subprocess.SubprocessError):
                    failed = True
        finally:
            runner.command = COMMAND
        remaining = {
            service for service, identifier in self.containers.items()
            if not self.absent_container(identifier)
        }
        remaining_volumes = {
            suffix for suffix, name in self.volumes.items()
            if COMMAND(["docker", "volume", "ls", "-q", "--filter", f"name=^{name}$"]).strip()
        }
        cleanup = json.loads((self.gate.evidence / "cleanup.json").read_text())
        if self.case in {"ordinary", "absent-race"}:
            if failed or remaining or remaining_volumes:
                raise ValueError("Confirmed absence or ordinary cleanup did not finish")
            if self.case == "absent-race" and not self.interposed:
                raise ValueError("The actual Docker absence race was not reached")
        elif self.case == "wrong-labels":
            if (
                not failed or remaining != {"outage-current"}
                or remaining_volumes != {"control"}
                or not {"container_identity", "volume_cleanup"}.issubset(cleanup["failures"])
            ):
                raise ValueError("Foreign resources were removed or independent owned cleanup stopped")
        elif self.case in {"remove-fault", "incoming-nonzero"}:
            if (
                not failed or not self.interposed or remaining != {"outage-legacy"}
                or remaining_volumes or "container_remove" not in cleanup["failures"]
            ):
                raise ValueError("Cleanup did not continue after an exact removal fault")
        elif self.case == "null-options":
            if (
                not failed or remaining or remaining_volumes != {"data"}
                or "volume_cleanup" not in cleanup["failures"]
            ):
                raise ValueError("Null options were accepted or stopped independent volume cleanup")
    def check_incoming_status(self):
        old_gate = runner.Gate
        old_execute = self.gate.execute
        old_signals = {
            number: runner.signal.getsignal(number)
            for number in (runner.signal.SIGTERM, runner.signal.SIGHUP, runner.signal.SIGINT)
        }
        def fail_execution():
            raise runner.Interrupted(23)
        self.gate.execute = fail_execution
        runner.Gate = lambda: self.gate
        try:
            with contextlib.redirect_stdout(io.StringIO()) as output:
                status = runner.main()
            receipt = json.loads(output.getvalue())
            if status != 23 or receipt["status"] != 23 or receipt["phase"] != "cleanup":
                raise ValueError("Cleanup replaced the incoming nonzero status")
        finally:
            runner.Gate = old_gate
            self.gate.execute = old_execute
            for number, handler in old_signals.items():
                runner.signal.signal(number, handler)

    def recover(self):
        failures = []
        found = COMMAND([
            "docker", "ps", "-aq", "--no-trunc", "--filter",
            f"label=org.hito.cleanup-control={self.owner}",
        ]).decode().split()
        for identifier in found:
            try:
                self.remove_control_container(identifier)
            except (OSError, ValueError, KeyError, runner.subprocess.SubprocessError):
                failures.append("container")
        names = COMMAND([
            "docker", "volume", "ls", "-q", "--filter",
            f"label=org.hito.cleanup-control={self.owner}",
        ]).decode().split()
        for name in names:
            try:
                [volume] = json.loads(COMMAND(["docker", "volume", "inspect", name]))
                self.verify_owner(volume["Labels"])
                if volume["Name"] != name or name not in {
                    f"{self.gate.project}-data", f"{self.gate.project}-control"
                }:
                    raise ValueError("Independent control volume identity changed")
                COMMAND(["docker", "volume", "rm", name])
            except (OSError, ValueError, KeyError, runner.subprocess.SubprocessError):
                failures.append("volume")
        for category, arguments in (
            ("container", ["docker", "ps", "-aq", "--filter"]),
            ("volume", ["docker", "volume", "ls", "-q", "--filter"]),
        ):
            if COMMAND([*arguments, f"label=org.hito.cleanup-control={self.owner}"]).strip():
                failures.append(category)
        if failures:
            raise ValueError("Independent control recovery failed")


def signal_probe(case):
    number = signal.SIGTERM if case == "signal-term" else signal.SIGHUP
    directory = Path(runner.tempfile.mkdtemp(prefix="hito-outage-signal-"))
    marker = directory / "ownership.json"
    process = runner.subprocess.Popen(
        [sys.executable, str(Path(__file__).resolve()), "--signal-child", str(marker)],
        stdout=runner.subprocess.PIPE, stderr=runner.subprocess.STDOUT,
        start_new_session=True,
    )
    recovery = None
    status = 1
    child_status = None
    before_recovery = None
    output = b""
    try:
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            if marker.exists():
                value = runner.bounded_json(marker, 4096)
                if (
                    not re.fullmatch(r"[0-9a-f]{32}", value.get("run_id", ""))
                    or value.get("project") != f"hito-provider-outage-{value['run_id']}"
                    or not re.fullmatch(r"[0-9a-f]{32}", value.get("owner", ""))
                ):
                    raise ValueError("Signal control ownership marker changed")
                if recovery is None:
                    recovery = Control("signal-child")
                    recovery.gate.run_id = value["run_id"]
                    recovery.gate.project = value["project"]
                    recovery.owner = value["owner"]
                if value.get("phase") == "prepared":
                    break
            if process.poll() is not None:
                raise ValueError("Signal control child ended before preparation")
            time.sleep(0.05)
        else:
            raise ValueError("Signal control preparation timed out")
        os.killpg(process.pid, number)
        output, _ = process.communicate(timeout=30)
        child_status = process.returncode
        if len(output) > 65536:
            raise ValueError("Signal control output exceeded its bound")
        filters = ["--filter", f"label=org.hito.cleanup-control={recovery.owner}"]
        before_recovery = {
            "containers": len(COMMAND(["docker", "ps", "-aq", *filters]).split()),
            "volumes": len(COMMAND(["docker", "volume", "ls", "-q", *filters]).split()),
        }
        if child_status != 128 + number or before_recovery != {"containers": 0, "volumes": 0}:
            raise ValueError("External signal skipped recovery or lost its incoming status")
        receipt = json.loads(output)
        if receipt.get("status") != 128 + number or not receipt.get("recovered"):
            raise ValueError("Signal completion receipt did not preserve recovery")
        status = 0
    except (OSError, ValueError, KeyError, TypeError, runner.subprocess.SubprocessError):
        status = 1
    finally:
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait(timeout=5)
        process.stdout.close()
        if recovery is not None:
            recovery.recover()
    receipt = {
        "event": "provider_outage_signal_control_finished", "case": case,
        "status": status, "signal": number, "child_status": child_status,
        "before_independent_recovery": before_recovery,
        "recovered": recovery is not None,
        "output_sha256": hashlib.sha256(output).hexdigest(),
    }
    (directory / "signal-result.json").write_text(json.dumps(receipt))
    print(json.dumps({**receipt, "evidence": str(directory)}), flush=True)
    return status


def main():
    for number in (signal.SIGTERM, signal.SIGHUP, signal.SIGINT):
        signal.signal(number, runner.interrupted)
    child_marker = Path(sys.argv[2]) if len(sys.argv) == 3 and sys.argv[1] == "--signal-child" else None
    cases = ["signal-child"] if child_marker else sys.argv[1:] or sorted(CASES)
    if child_marker and not child_marker.is_absolute():
        raise ValueError("An exact signal marker path is required")
    if not child_marker and (
        len(cases) > len(CASES) or len(set(cases)) != len(cases) or any(case not in CASES for case in cases)
    ):
        raise ValueError("Choose an exact owned cleanup control")
    for case in cases:
        if case in {"signal-term", "signal-hup"}:
            if signal_probe(case):
                return 1
            continue
        control = Control(case)
        status = 0
        recovered = False
        def publish(phase):
            if child_marker:
                temporary = child_marker.with_suffix(".tmp")
                temporary.write_text(json.dumps({
                    "phase": phase, "run_id": control.gate.run_id,
                    "project": control.gate.project, "owner": control.owner,
                }))
                temporary.replace(child_marker)
        try:
            publish("created")
            control.prepare()
            if child_marker:
                publish("prepared")
                signal.pause()
                raise ValueError("Signal control resumed without termination")
            control.exercise()
        except runner.Interrupted as interruption:
            status = interruption.status
        except KeyboardInterrupt:
            status = 130
        except (OSError, ValueError, KeyError, TypeError, runner.subprocess.SubprocessError):
            status = 1
        finally:
            for number in (signal.SIGTERM, signal.SIGHUP, signal.SIGINT):
                signal.signal(number, signal.SIG_IGN)
            runner.command = COMMAND
            try:
                control.recover()
                recovered = True
            except (OSError, ValueError, KeyError, TypeError, runner.subprocess.SubprocessError):
                if status == 0:
                    status = 1
            for number in (signal.SIGTERM, signal.SIGHUP, signal.SIGINT):
                signal.signal(number, runner.interrupted)
        print(json.dumps({
            "event": "provider_outage_cleanup_control_finished", "case": case,
            "status": status, "interposed": control.interposed,
            "recovered": recovered, "evidence": str(control.gate.evidence),
            "runner_sha256": hashlib.sha256(Path(runner.__file__).read_bytes()).hexdigest(),
        }), flush=True)
        if status:
            return status
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except runner.Interrupted as interruption:
        print(json.dumps({
            "event": "provider_outage_cleanup_control_interrupted",
            "status": interruption.status,
        }), flush=True)
        raise SystemExit(interruption.status) from None
