"""Bounded Docker commands and cleanup restricted to one unique audit owner."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import uuid

from source import ROOT


OWNER_LABEL = "org.hito.all-debrid-audit"


class Docker:
    def __init__(self, image):
        self.image = image
        self.owner = "hito-ad-audit-" + uuid.uuid4().hex
        self.volume = self.owner + "-inputs"
        self.names = []
        self.volume_attempted = False
        self.commands = []
        self.environment = dict(os.environ, HITO_ALL_DEBRID_AUDIT_IMAGE=image,
                                HITO_ALL_DEBRID_AUDIT_OWNER=self.owner,
                                HITO_ALL_DEBRID_AUDIT_VOLUME=self.volume)
        self.compose = ["compose", "--project-directory", str(ROOT), "-p", self.owner,
                        "-f", str(Path(__file__).with_name("audit.compose.yaml"))]

    def call(self, *arguments, data=None, limit=262144, timeout=45, merge=False):
        self.commands.append(["docker", *arguments])
        with tempfile.TemporaryFile() as output, tempfile.TemporaryFile() as errors:
            try:
                completed = subprocess.run(
                    ["docker", *arguments], input=data, stdout=output, stderr=output if merge else errors,
                    env=self.environment, timeout=timeout, check=False,
                )
            except subprocess.TimeoutExpired:
                raise RuntimeError("Docker command exceeded its deadline: " + arguments[0]) from None
            if completed.returncode != 0:
                raise RuntimeError("Docker command failed: {} (exit {})".format(
                    arguments[0], completed.returncode
                ))
            if output.tell() > limit or errors.tell() > 65536:
                raise RuntimeError("Docker command output exceeded its bound")
            output.seek(0)
            return output.read()

    def inspect(self, identifier):
        [container] = json.loads(self.call("container", "inspect", identifier))
        self.check_owner(container)
        return container

    def check_owner(self, container):
        labels = container["Config"]["Labels"] or {}
        if (labels.get(OWNER_LABEL), labels.get("com.docker.compose.project"), container["Image"]) != (
            self.owner, self.owner, self.image
        ):
            raise RuntimeError("Container ownership or immutable image changed")

    def create_volume(self):
        if self.volume in self.call("volume", "ls", "--format", "{{.Name}}").decode().splitlines():
            raise RuntimeError("Unique audit volume already exists")
        self.volume_attempted = True
        self.call("volume", "create", "--label", OWNER_LABEL + "=" + self.owner, self.volume)
        self.check_volume()

    def check_volume(self):
        [volume] = json.loads(self.call("volume", "inspect", self.volume))
        if volume["Name"] != self.volume or (volume["Labels"] or {}).get(OWNER_LABEL) != self.owner:
            raise RuntimeError("Volume ownership changed")

    def helper(self, service, suffix, data=None):
        name = self.owner + "-" + suffix
        self.names.append(name)
        output = self.call(*self.compose, "run", "--no-deps", "-T", "--pull", "never",
                           "--name", name, service, data=data, limit=16384)
        container = self.inspect(name)
        self.check_exit(container)
        return output

    def start(self):
        name = self.owner + "-plugin"
        self.names.append(name)
        self.call(*self.compose, "up", "-d", "--no-build", "--pull", "never", "--no-deps", "plugin")
        return self.inspect(name)

    def stop(self, identifier):
        self.inspect(identifier)
        self.call("stop", "--time", "30", identifier, timeout=40)
        container = self.inspect(identifier)
        self.check_exit(container)
        return container

    @staticmethod
    def check_exit(container):
        state = container["State"]
        if state["Running"] or state["ExitCode"] != 0 or state["OOMKilled"] or state["Error"]:
            raise RuntimeError("Owned container did not join successfully")

    def cleanup(self):
        failures = []
        removed = []
        for name in reversed(self.names):
            try:
                rows = self.call("container", "ls", "-a", "--no-trunc", "--format",
                                 "{{.ID}} {{.Names}}").decode().splitlines()
                matches = [row.split()[0] for row in rows if row.split()[1] == name]
                if not matches:
                    continue
                [identifier] = matches
                container = self.inspect(identifier)
                if container["State"]["Running"]:
                    try:
                        self.stop(identifier)
                    except RuntimeError as error:
                        failures.append(str(error))
                    container = self.inspect(identifier)
                if container["State"]["Running"]:
                    raise RuntimeError("Owned container still running; removal refused")
                self.call("container", "rm", identifier)
                removed.append(identifier)
            except (RuntimeError, ValueError, KeyError) as error:
                failures.append(str(error))
        volume_removed = False
        if self.volume_attempted:
            try:
                volumes = self.call("volume", "ls", "--format", "{{.Name}}").decode().splitlines()
                if self.volume in volumes:
                    self.check_volume()
                    self.call("volume", "rm", self.volume)
                    volume_removed = True
            except (RuntimeError, ValueError, KeyError) as error:
                failures.append(str(error))
        survivors = None
        survivor_counts = {"containers": None, "volumes": None}
        try:
            containers = set()
            for label in (OWNER_LABEL, "com.docker.compose.project"):
                containers.update(self.call(
                    "container", "ls", "-a", "--no-trunc", "--filter", "label=" + label + "=" + self.owner,
                    "--format", "{{.ID}}", limit=16384, timeout=10,
                ).decode().splitlines())
            volumes = set(self.call(
                "volume", "ls", "--filter", "label=" + OWNER_LABEL + "=" + self.owner,
                "--format", "{{.Name}}", limit=16384, timeout=10,
            ).decode().splitlines())
            exact_volume = self.call(
                "volume", "ls", "--filter", "name=" + self.volume,
                "--format", "{{.Name}}", limit=16384, timeout=10,
            ).decode().splitlines()
            if self.volume in exact_volume:
                volumes.add(self.volume)
            survivors = {"container_ids": sorted(containers), "volume_names": sorted(volumes)}
            survivor_counts = {"containers": len(containers), "volumes": len(volumes)}
            if containers or volumes:
                failures.append("Final inventory found surviving owned audit resources")
        except (RuntimeError, ValueError, KeyError) as error:
            failures.append("Final owned resource inventory failed: " + str(error))
        return {"removed_container_ids": removed, "removed_volume": self.volume if volume_removed else None,
                "survivors": survivors, "survivor_counts": survivor_counts, "failures": failures}
