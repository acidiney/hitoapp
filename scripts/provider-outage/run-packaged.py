#!/usr/bin/env python3
"""Run one owned upgrade/playback gate without builds or persistent user data."""

from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import selectors
import signal
import subprocess
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[2]
SERVICES = {
    "outage-keeper", "outage-legacy", "outage-current",
    "outage-migrate", "outage-browser", "outage-prepare",
}
CURRENT_SOURCE_PIN_SHA256 = "ed566004349b6a336a0cef2bc3c88d3615084af61b1198a1841b5b94568b1a45"
LEGACY_SOURCE = "e0b8c72e804879141fc938e4eeec14204386e179"
LEGACY_INPUTS = "0ea456d33fbc6d793c52bb63673104ce67f63fbb64266c21e6a90d06d589bfe3"
LEGACY_SOURCE_PIN_SHA256 = "7708087196070dab0d807e862a8a949b747d981eb783477d5cf00236f8bc758f"
LEGACY_ROOT_DOCKERFILE_SHA256 = "ce9d2272bf24eef66684785caae44aac88e1c17f55729c445bad96eb6d32f13f"
CURRENT_DOCKERFILE = "docker/server.Dockerfile"
LEGACY_DOCKERFILE = "Dockerfile"
EXPECTED_CURRENT_MIGRATION_VERSIONS = tuple(range(1, 99))
LEGACY_DATABASE_VERSION = 54
SOURCE_RECEIPT_MAX_BYTES = 1024 * 1024


@dataclass(frozen=True)
class SourcePinContract:
    schema: str
    helper: Path
    helper_sha256: str
    dockerfile: str
    dockerfile_sha256: str


@dataclass(frozen=True)
class MigrationInventory:
    entries: tuple[tuple[int, str, str], ...]

    @property
    def versions(self):
        return tuple(version for version, _, _ in self.entries)

    @property
    def latest(self):
        return max(self.versions, default=0)

    def after(self, version):
        return sum(item > version for item in self.versions)


def file_sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def source_pin_contract(prefix, tree):
    tree = Path(tree).resolve()
    if prefix == "HITO_PACKAGED":
        schema = "current-server-dockerfile-v1"
        helper = tree / "scripts/artwork/source-pin.py"
        helper_sha = CURRENT_SOURCE_PIN_SHA256
        dockerfile = CURRENT_DOCKERFILE
    elif prefix == "HITO_OUTAGE_LEGACY":
        schema = "legacy-root-dockerfile-v1"
        helper = tree / "scripts/artwork/source-pin.py"
        helper_sha = LEGACY_SOURCE_PIN_SHA256
        dockerfile = LEGACY_DOCKERFILE
    else:
        raise ValueError("Unknown packaged source-pin contract")
    dockerfile_path = tree / dockerfile
    if not helper.is_file() or helper.is_symlink() or file_sha256(helper) != helper_sha:
        raise ValueError("The pinned source-pin helper changed")
    if not dockerfile_path.is_file() or dockerfile_path.is_symlink():
        raise ValueError("The source tree does not contain its pinned Dockerfile input")
    dockerfile_sha = file_sha256(dockerfile_path)
    if prefix == "HITO_OUTAGE_LEGACY" and dockerfile_sha != LEGACY_ROOT_DOCKERFILE_SHA256:
        raise ValueError("The legacy root Dockerfile changed")
    return SourcePinContract(schema, helper, helper_sha, dockerfile, dockerfile_sha)


def migration_inventory(directory):
    directory = Path(directory)
    if not directory.is_dir() or directory.is_symlink():
        raise ValueError("The embedded migration directory is unavailable")
    entries = []
    for path in sorted(directory.iterdir()):
        if path.suffix != ".sql":
            continue
        match = re.fullmatch(r"([0-9]{4})_[a-z0-9_]+\.sql", path.name)
        if not match or not path.is_file() or path.is_symlink():
            raise ValueError("The embedded migration inventory contains an invalid file")
        entries.append((int(match.group(1)), path.name, file_sha256(path)))
    versions = tuple(version for version, _, _ in entries)
    if not entries or len(set(versions)) != len(versions):
        raise ValueError("The embedded migration inventory is empty or duplicated")
    return MigrationInventory(tuple(entries))


def current_migration_inventory(inventory):
    return inventory.versions == EXPECTED_CURRENT_MIGRATION_VERSIONS


def migration_inventory_from_receipt(value):
    entries = value.get("entries") if isinstance(value, dict) else None
    if not isinstance(entries, list) or len(entries) != 98:
        raise ValueError("The captured candidate migration inventory is incomplete")
    parsed = []
    for entry in entries:
        if (
            not isinstance(entry, dict)
            or set(entry) != {"version", "name", "sha256"}
            or type(entry.get("version")) is not int
            or not isinstance(entry.get("name"), str)
            or not re.fullmatch(r"[0-9]{4}_[a-z0-9_]+\.sql", entry["name"])
            or not re.fullmatch(r"[0-9a-f]{64}", entry.get("sha256", ""))
        ):
            raise ValueError("The captured candidate migration inventory is invalid")
        match = re.match(r"([0-9]{4})_", entry["name"])
        if int(match.group(1)) != entry["version"]:
            raise ValueError("The captured candidate migration version does not match its name")
        parsed.append((entry["version"], entry["name"], entry["sha256"]))
    inventory = MigrationInventory(tuple(parsed))
    if (
        tuple(sorted(parsed)) != inventory.entries
        or not current_migration_inventory(inventory)
        or value.get("current_version") != inventory.latest
        or value.get("total_migrations") != len(inventory.entries)
        or value.get("migrations_after_legacy") != inventory.after(LEGACY_DATABASE_VERSION)
    ):
        raise ValueError("The captured candidate migration inventory does not match its summary")
    return inventory


def migration_receipt_matches(receipt, inventory):
    if not current_migration_inventory(inventory) or not isinstance(receipt, dict):
        return False
    previous = receipt.get("previous_version")
    current = receipt.get("current_version")
    applied = receipt.get("applied_count")
    return (
        receipt.get("event") == "database_migration_finished"
        and receipt.get("outcome") == "complete"
        and type(previous) is int and previous == LEGACY_DATABASE_VERSION
        and type(current) is int and current == inventory.latest
        and type(applied) is int and applied == inventory.after(previous)
    )


class Interrupted(Exception):
    def __init__(self, status):
        self.status = status


def interrupted(number, _frame):
    raise Interrupted(128 + number)


def command(arguments, seconds=30, maximum=2 * 1024 * 1024):
    process = subprocess.Popen(
        arguments, cwd=ROOT, stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT, start_new_session=True,
    )
    output = bytearray()
    deadline = time.monotonic() + seconds
    selector = selectors.DefaultSelector()
    selector.register(process.stdout, selectors.EVENT_READ)
    try:
        while selector.get_map():
            if time.monotonic() >= deadline:
                raise ValueError("Owned command deadline exceeded")
            for key, _ in selector.select(min(0.25, deadline - time.monotonic())):
                chunk = os.read(key.fd, 65536)
                if not chunk:
                    selector.unregister(key.fileobj)
                    continue
                output.extend(chunk)
                if len(output) > maximum:
                    raise ValueError("Owned command output exceeded its bound")
        status = process.wait(timeout=max(0.1, deadline - time.monotonic()))
        if status:
            raise ValueError("Owned command failed")
        return bytes(output)
    finally:
        selector.close()
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait(timeout=5)
        process.stdout.close()


def bounded_json(path, maximum=524288):
    if path.stat().st_size > maximum:
        raise ValueError("Owned evidence exceeded its bound")
    return json.loads(path.read_text())


def source_snapshot():
    paths = command(["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"])
    files = {}
    for raw in paths.split(b"\0"):
        if not raw:
            continue
        path = ROOT / raw.decode()
        data = os.readlink(path).encode() if path.is_symlink() else path.read_bytes()
        files[raw.decode()] = hashlib.sha256(data).hexdigest()
    return files


class Gate:
    def __init__(self):
        self.run_id = secrets.token_hex(16)
        self.project = f"hito-provider-outage-{self.run_id}"
        self.directory = Path(tempfile.mkdtemp(prefix="hito-provider-outage-"))
        self.directory.chmod(0o755)
        self.evidence = self.directory / "evidence"
        self.music = self.directory / "music"
        for path in (self.evidence, self.music):
            path.mkdir(mode=0o777)
            path.chmod(0o777)
        self.compose = [
            "docker", "compose", "-p", self.project,
            "-f", "scripts/provider-outage/compose.e2e.yaml",
            "--profile", "e2e", "--profile", "tools",
        ]
        self.browser = None
        self.browser_output = bytearray()
        self.browser_overflow = False
        self.reader = None
        self.container_services = {}
        self.modules_before = None
        os.environ.update({
            "HITO_OUTAGE_RUN_ID": self.run_id,
            "HITO_OUTAGE_PROJECT": self.project,
            "HITO_OUTAGE_WORKTREE": str(ROOT),
            "HITO_OUTAGE_MUSIC": str(self.music),
            "HITO_OUTAGE_EVIDENCE": str(self.evidence),
        })

    def compose_run(self, *arguments, seconds=60):
        return command([*self.compose, *arguments], seconds)

    def identity(self, identifier, service=None):
        [container] = json.loads(command(["docker", "inspect", identifier]))
        labels = container.get("Config", {}).get("Labels") or {}
        if (
            not re.fullmatch(r"[0-9a-f]{64}", container.get("Id", ""))
            or labels.get("com.docker.compose.project") != self.project
            or labels.get("org.hito.fixture-run") != self.run_id
            or labels.get("org.hito.owned-fixture") != "provider-outage"
            or labels.get("com.docker.compose.service") not in SERVICES
            or (service and labels.get("com.docker.compose.service") != service)
        ):
            raise ValueError("Exact owned container identity changed")
        self.container_services[container["Id"]] = labels["com.docker.compose.service"]
        return container["Id"]

    def service(self, service):
        identifier = self.compose_run("ps", "-aq", service).decode().strip()
        return self.identity(identifier, service)

    def marker(self, name, phase):
        path = self.evidence / f"{name}.json"
        if not path.exists():
            return None
        value = bounded_json(path, 4096)
        if value.get("run_id") != self.run_id or value.get("phase") != phase:
            raise ValueError("Owned browser control identity changed")
        return value

    def wait_marker(self, name, phase, seconds):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            if self.browser_overflow or self.browser.poll() is not None:
                raise ValueError("Owned browser ended before its control point")
            value = self.marker(name, phase)
            if value is not None:
                return value
            time.sleep(0.05)
        raise ValueError("Owned browser control timed out")

    def publish(self, name, phase, **fields):
        path = self.evidence / f"{name}.json"
        temporary = self.evidence / f".{name}.tmp"
        temporary.write_text(json.dumps({"run_id": self.run_id, "phase": phase, **fields}))
        temporary.replace(path)

    def preflight(self):
        for prefix in ("HITO_PACKAGED", "HITO_OUTAGE_LEGACY"):
            image, source, inputs, tree = [
                os.environ[f"{prefix}_{key}"]
                for key in ("IMAGE", "SOURCE", "INPUTS", "WORKTREE")
            ]
            if (
                not re.fullmatch(r"sha256:[0-9a-f]{64}", image)
                or not re.fullmatch(r"[0-9a-f]{40}", source)
                or not re.fullmatch(r"[0-9a-f]{64}", inputs)
            ):
                raise ValueError("Immutable runtime pins are required")
            name = "current" if prefix == "HITO_PACKAGED" else "legacy"
            if prefix == "HITO_OUTAGE_LEGACY" and (source != LEGACY_SOURCE or inputs != LEGACY_INPUTS):
                raise ValueError("The legacy runtime requires its pinned root-Dockerfile fingerprint")
            contract = source_pin_contract(prefix, tree)
            source_receipt = self.evidence / f"{name}-source.json"
            command([
                "python3", str(contract.helper), "--root", tree,
                "--expect-head", source, "--expect-digest", inputs,
                "--output", str(source_receipt),
            ])
            pinned = bounded_json(source_receipt, SOURCE_RECEIPT_MAX_BYTES)
            if (
                pinned.get("schema_version") != 1
                or not isinstance(pinned.get("files"), list)
                or not pinned["files"]
                or pinned.get("source_head") != source
                or pinned.get("product_inputs_sha256") != inputs
            ):
                raise ValueError("The selected product-input helper returned a mismatched pin")
            pin_contract = {
                "source_role": name,
                "fingerprint_schema": contract.schema,
                "helper_sha256": contract.helper_sha256,
                "dockerfile_input": contract.dockerfile,
                "dockerfile_sha256": contract.dockerfile_sha256,
                "source_head": source,
                "product_inputs_sha256": inputs,
                "pinned_file_count": len(pinned.get("files", [])),
            }
            (self.evidence / f"{name}-fingerprint-contract.json").write_text(json.dumps(pin_contract, sort_keys=True))
            if prefix == "HITO_PACKAGED":
                inventory = migration_inventory(Path(tree) / "crates/core/migrations")
                if not current_migration_inventory(inventory) or inventory.after(LEGACY_DATABASE_VERSION) != 44:
                    raise ValueError("The candidate embedded migration inventory is not exactly versions 1 through 98")
                inventory_receipt = {
                    "source_head": source,
                    "fingerprint_schema": contract.schema,
                    "current_version": inventory.latest,
                    "total_migrations": len(inventory.entries),
                    "migrations_after_legacy": inventory.after(LEGACY_DATABASE_VERSION),
                    "entries": [
                        {"version": version, "name": migration, "sha256": digest}
                        for version, migration, digest in inventory.entries
                    ],
                }
                (self.evidence / "current-migration-inventory.json").write_text(json.dumps(inventory_receipt, sort_keys=True))
            inspection = command(["docker", "image", "inspect", image])
            [runtime] = json.loads(inspection)
            if runtime.get("Os") != "linux" or runtime.get("Architecture") != "arm64":
                raise ValueError("The owned runtime must match the native browser platform")
            path = self.evidence / f"{name}-image.json"
            path.write_bytes(inspection)
            command(["python3", "scripts/artwork/check-image.py", str(path), image, source, inputs])
        for key in ("HITO_OUTAGE_NODE_IMAGE", "HITO_OUTAGE_BROWSER_IMAGE"):
            image = os.environ[key]
            if not re.fullmatch(r"sha256:[0-9a-f]{64}", image):
                raise ValueError("Immutable tool image pins are required")
            [found] = json.loads(command(["docker", "image", "inspect", image]))
            if found["Id"] != image:
                raise ValueError("Owned tool image identity changed")
            if found.get("Os") != "linux" or found.get("Architecture") != "arm64":
                raise ValueError("Owned tool platform changed")
        for relative in ("apps/web/node_modules", "fixtures/e2e-generated"):
            command(["git", "check-ignore", f"{relative}/"])
            (ROOT / relative).mkdir(parents=True, exist_ok=True)
        (self.evidence / "compose.json").write_bytes(self.compose_run("config", "--format", "json"))

    def module_pin(self, name):
        raw = self.compose_run(
            "run", "-T", "--rm", "--no-deps", "--pull", "never",
            "outage-prepare", "node", "/workspace/scripts/lyrics/provider-choice/module-pin.mjs",
            seconds=30,
        )
        rows = [json.loads(line) for line in raw.splitlines() if line.startswith(b"{")]
        if len(rows) != 1:
            raise ValueError("Exactly one browser module receipt is required")
        receipt = rows[0]
        if (
            set(receipt) != {"version", "files", "bytes", "sha256"}
            or receipt.get("version") != "1.56.1"
            or not isinstance(receipt.get("files"), int)
            or not 0 < receipt["files"] <= 10000
            or not isinstance(receipt.get("bytes"), int)
            or not 0 < receipt["bytes"] <= 128 * 1024 * 1024
            or not re.fullmatch(r"[0-9a-f]{64}", receipt.get("sha256", ""))
        ):
            raise ValueError("Exact browser execution modules rejected")
        (self.evidence / f"{name}.json").write_text(json.dumps(receipt, sort_keys=True))
        return receipt

    def start_browser(self):
        self.compose_run("create", "--no-build", "--pull", "never", "outage-browser")
        self.service("outage-browser")
        self.browser = subprocess.Popen(
            [*self.compose, "up", "--no-deps", "--no-build", "--no-recreate",
             "--pull", "never", "--abort-on-container-exit", "--exit-code-from",
             "outage-browser", "outage-browser"],
            cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        def consume():
            while chunk := self.browser.stdout.read(65536):
                if len(self.browser_output) + len(chunk) > 2 * 1024 * 1024:
                    self.browser_overflow = True
                    return
                self.browser_output.extend(chunk)
        self.reader = threading.Thread(target=consume, daemon=True)
        self.reader.start()

    def assert_assets(self, service, marker):
        expected = marker.get("assets")
        paths = ("/assets/app.js", "/assets/app.css")
        if not isinstance(expected, dict) or set(expected) != set(paths):
            raise ValueError("Both actual served asset witnesses are required")
        raw = command([
            "docker", "exec", self.service(service), "sha256sum",
            "/app/web/assets/app.js", "/app/web/assets/app.css",
        ], maximum=4096)
        actual = {}
        for line in raw.decode().splitlines():
            digest, path = line.split()
            if not re.fullmatch(r"[0-9a-f]{64}", digest):
                raise ValueError("Runtime asset digest rejected")
            actual[path.removeprefix("/app/web")] = digest
        if actual != expected:
            raise ValueError("Served assets do not match the exact runtime")
        return actual

    def check_logs(self):
        browser = self.service("outage-browser")
        registry = command([
            "docker", "exec", browser, "node", "--input-type=module", "-e",
            "import fs from 'node:fs/promises'; "
            "process.stdout.write(await fs.readFile('/control/private-canaries.json'));",
        ], maximum=65536)
        canaries = json.loads(registry)
        if (
            not isinstance(canaries, list) or not canaries or len(canaries) > 256
            or any(not isinstance(item, str) or not item or len(item) > 4096
                   for item in canaries)
        ):
            raise ValueError("Owned private registry rejected")
        result = {
            "private_matches": 0, "malformed": 0,
            "unavailable_commits": 0, "ready_commits": 0,
            "log_bytes": 0, "log_sha256": {},
        }
        for service in ("outage-legacy", "outage-current"):
            raw = command(["docker", "logs", self.service(service)], maximum=2 * 1024 * 1024)
            result["log_bytes"] += len(raw)
            result["log_sha256"][service] = hashlib.sha256(raw).hexdigest()
            result["private_matches"] += sum(item.encode() in raw for item in canaries)
            for line in raw.splitlines():
                try:
                    item = json.loads(line)
                    fields = item["fields"]
                    if not isinstance(fields, dict):
                        raise ValueError("Structured log fields rejected")
                    if service == "outage-current" and fields.get("event") == "source_resolution_committed":
                        if fields.get("outcome") == "provider_unavailable": result["unavailable_commits"] += 1
                        if fields.get("outcome") == "ready": result["ready_commits"] += 1
                except (ValueError, KeyError, TypeError):
                    result["malformed"] += 1
        self.publish("logs-checked", "logs_checked", **result)

    def execute(self):
        self.preflight()
        self.modules_before = self.module_pin("modules-before")
        self.before = source_snapshot()
        (self.evidence / "gate-source.json").write_text(json.dumps(self.before, sort_keys=True))
        self.compose_run("run", "-T", "--rm", "--no-deps", "--pull", "never", "outage-prepare", seconds=60)
        self.compose_run("up", "-d", "--no-build", "--pull", "never", "--wait", "outage-legacy", seconds=60)
        self.service("outage-keeper")
        self.service("outage-legacy")
        self.start_browser()
        legacy = self.wait_marker("legacy-ready", "legacy_ready", 90)
        self.assert_assets("outage-legacy", legacy)
        self.compose_run("stop", "-t", "15", "outage-legacy", seconds=25)
        migration_output = self.compose_run(
            "run", "-T", "--rm", "--no-deps", "--pull", "never",
            "outage-migrate", seconds=30,
        )
        migration_rows = [json.loads(line) for line in migration_output.splitlines() if line.startswith(b"{")]
        if len(migration_rows) != 1:
            raise ValueError("Exactly one real migration receipt is required")
        migration = migration_rows[0]
        inventory_path = self.evidence / "current-migration-inventory.json"
        if not inventory_path.is_file():
            raise ValueError("The exact candidate migration inventory was not captured")
        inventory_receipt = bounded_json(inventory_path, 65536)
        inventory = migration_inventory_from_receipt(inventory_receipt)
        current_tree = Path(os.environ["HITO_PACKAGED_WORKTREE"])
        if (
            inventory_receipt.get("source_head") != os.environ["HITO_PACKAGED_SOURCE"]
            or inventory_receipt.get("fingerprint_schema") != "current-server-dockerfile-v1"
        ):
            raise ValueError("The migration inventory is not bound to the current source pin")
        live_inventory = migration_inventory(current_tree / "crates/core/migrations")
        if live_inventory != inventory:
            raise ValueError("The candidate migration inventory changed after preflight")
        if not migration_receipt_matches(migration, inventory):
            raise ValueError("The actual populated version54 migration was not proven")
        self.compose_run("up", "-d", "--no-build", "--pull", "never", "--wait", "outage-current", seconds=45)
        self.service("outage-current")
        self.publish(
            "migrated", "migrated",
            previous_version=migration["previous_version"],
            current_version=migration["current_version"],
            applied_count=migration["applied_count"],
        )
        current = self.wait_marker("log-check-ready", "log_check_ready", 60)
        self.assert_assets("outage-current", current)
        self.check_logs()
        status = self.browser.wait(timeout=20)
        self.reader.join(timeout=5)
        if status or self.browser_overflow or self.reader.is_alive():
            raise ValueError("The owned packaged browser did not pass")
        report = bounded_json(self.evidence / "result.json")
        stats = report["stats"]
        if stats["expected"] != 1 or any(stats[key] for key in ("unexpected", "flaky", "skipped")):
            raise ValueError("Exactly one complete browser case is required")
        case = bounded_json(self.evidence / "provider-outage.json")
        if case.get("event") != "provider_outage_packaged_verified" or not case.get("preserved_migration_state"):
            raise ValueError("Owned upgrade/playback evidence is incomplete")
        if self.before != source_snapshot():
            raise ValueError("Gate source changed during qualification")

    def container_absent(self, identifier):
        if not re.fullmatch(r"[0-9a-f]{64}", identifier):
            raise ValueError("An immutable cleanup container identity is required")
        found = command([
            "docker", "ps", "-aq", "--no-trunc", "--filter", f"id={identifier}",
        ])
        return not found.strip()

    def volume_absent(self, name):
        found = command(["docker", "volume", "ls", "-q", "--filter", f"name=^{name}$"])
        return not found.strip()

    def cleanup(self):
        failures = []
        errors = (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError)
        try:
            if self.browser and self.browser.poll() is None:
                os.killpg(self.browser.pid, signal.SIGTERM)
                try:
                    self.browser.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(self.browser.pid, signal.SIGKILL)
                    self.browser.wait(timeout=5)
        except errors:
            failures.append("browser_stop")
        if self.modules_before is not None:
            try:
                if self.module_pin("modules-after") != self.modules_before:
                    raise ValueError("Browser execution module content changed")
            except errors:
                failures.append("module_pin")
        identifiers = []
        try:
            identifiers = command([
                "docker", "ps", "-aq", "--no-trunc", "--filter",
                f"label=com.docker.compose.project={self.project}",
            ]).decode().split()
        except errors:
            failures.append("container_discovery")
        owned = []
        for identifier in identifiers:
            try:
                owned.append(self.identity(identifier))
            except errors:
                try:
                    if not self.container_absent(identifier):
                        failures.append("container_identity")
                except errors:
                    failures.append("container_absence")
        for identifier in sorted(owned, key=lambda item: self.container_services[item] == "outage-keeper"):
            try:
                command(["docker", "rm", "-f", identifier])
            except errors:
                try:
                    if not self.container_absent(identifier):
                        failures.append("container_remove")
                except errors:
                    failures.append("container_absence")
        for suffix in ("data", "control"):
            name = f"{self.project}-{suffix}"
            try:
                if self.volume_absent(name):
                    continue
                try:
                    [volume] = json.loads(command(["docker", "volume", "inspect", name]))
                except errors:
                    if self.volume_absent(name):
                        continue
                    raise
                labels = volume.get("Labels") or {}
                options = volume.get("Options")
                if (
                    volume["Name"] != name
                    or labels.get("com.docker.compose.project") != self.project
                    or labels.get("com.docker.compose.volume") != f"outage-{suffix}"
                    or labels.get("org.hito.fixture-run") != self.run_id
                    or labels.get("org.hito.owned-fixture") != "provider-outage"
                    or volume.get("Driver") != "local"
                    or not isinstance(options, dict)
                    or options.get("type") != "tmpfs"
                    or options.get("device") != "tmpfs"
                ):
                    raise ValueError("Exact owned tmpfs volume identity changed")
                try:
                    command(["docker", "volume", "rm", name])
                except errors:
                    if not self.volume_absent(name):
                        raise
            except errors:
                failures.append("volume_cleanup")
        remaining = None
        remaining_volumes = None
        try:
            remaining = len(command([
                "docker", "ps", "-aq", "--no-trunc", "--filter",
                f"label=com.docker.compose.project={self.project}",
            ]).split())
            if remaining:
                failures.append("remaining_containers")
        except errors:
            failures.append("container_verification")
        try:
            remaining_volumes = len(command([
                "docker", "volume", "ls", "-q", "--filter",
                f"label=com.docker.compose.project={self.project}",
            ]).split())
            if remaining_volumes:
                failures.append("remaining_volumes")
        except errors:
            failures.append("volume_verification")
        try:
            if hasattr(self, "before") and self.before != source_snapshot():
                raise ValueError("Gate source changed during qualification")
        except errors:
            failures.append("source_pin")
        (self.evidence / "cleanup.json").write_text(json.dumps({
            "containers": remaining, "volumes": remaining_volumes,
            "failures": failures,
        }))
        (self.evidence / "browser-output.json").write_text(json.dumps({
            "bytes": len(self.browser_output),
            "sha256": hashlib.sha256(self.browser_output).hexdigest(),
        }))
        if failures:
            raise ValueError("Owned cleanup failed: " + ",".join(sorted(set(failures))))


def main():
    for number in (signal.SIGTERM, signal.SIGHUP, signal.SIGINT):
        signal.signal(number, interrupted)
    gate = Gate()
    status = 0
    phase = "execute"
    try:
        gate.execute()
    except KeyboardInterrupt:
        status = 130
    except Interrupted as interruption:
        status = interruption.status
    except (OSError, ValueError, KeyError, TypeError, json.JSONDecodeError, subprocess.SubprocessError):
        status = 1
    finally:
        for number in (signal.SIGTERM, signal.SIGHUP, signal.SIGINT):
            signal.signal(number, signal.SIG_IGN)
        try:
            gate.cleanup()
        except (OSError, ValueError, KeyError, TypeError, json.JSONDecodeError, subprocess.SubprocessError):
            if status == 0: status = 1
            phase = "cleanup"
    print(json.dumps({
        "event": "provider_outage_gate_finished", "status": status,
        "phase": phase, "evidence": str(gate.evidence),
    }))
    return status


if __name__ == "__main__":
    raise SystemExit(main())
