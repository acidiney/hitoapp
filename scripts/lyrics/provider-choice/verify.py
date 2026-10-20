import hashlib
import json
from pathlib import Path
import re
import subprocess

UUID = re.compile(r"^[a-f0-9]{8}-[a-f0-9]{4}-4[a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$")


def overlay(root):
    paths = [root / "apps/web/e2e/recording-lyric-provider-choice.spec.ts",
             root / "apps/web/playwright.lyric-provider-choice.config.ts",
             *root.glob("apps/web/e2e/fixtures/recording-lyric-provider-choice-*.ts"),
             *root.glob("scripts/lyrics/provider-choice/*")]
    files = [[str(path.relative_to(root)), hashlib.sha256(path.read_bytes()).hexdigest()]
             for path in sorted(paths) if path.is_file()]
    old = sorted([root / "apps/web/e2e/recording-lyrics.spec.ts",
                  *root.glob("apps/web/e2e/fixtures/recording-lyrics-*.ts"),
                  *root.glob("scripts/lyrics/recording-browser*")])
    if len(old) != 11 or not 10 <= len(files) <= 16:
        raise ValueError("The bounded chooser or historical gate is incomplete")
    for path in old:
        if path.read_bytes() != subprocess.check_output(["git", "-C", str(root), "show", f"HEAD:{path.relative_to(root)}"]):
            raise ValueError("A historical lyric gate changed")
    old_files = [[str(path.relative_to(root)), hashlib.sha256(path.read_bytes()).hexdigest()] for path in old]
    return {"files": files, "old_gate": old_files,
            "sha256": hashlib.sha256(json.dumps([files, old_files], separators=(",", ":")).encode()).hexdigest()}


def image_contract(image, image_id, source, inputs):
    labels = image.get("Config", {}).get("Labels", {})
    return (image.get("Id") == image_id and labels.get("org.opencontainers.image.revision") == source
            and labels.get("org.hito.product-input-sha256") == inputs)


def runtime_contract(container, image_id, project, run_id, root, browser=False, app=None, evidence=None):
    host, mounts, config = container["HostConfig"], container["Mounts"], container["Config"]
    labels = config["Labels"]
    common = (container["Image"] == image_id and config["User"] == "10001:10001"
              and labels.get("com.docker.compose.project") == project
              and labels.get("org.hito.owned-fixture") == "lyric-provider-choice"
              and labels.get("org.hito.fixture-run") == run_id
              and host["ReadonlyRootfs"] and host["CapDrop"] == ["ALL"]
              and "no-new-privileges:true" in host["SecurityOpt"]
              and not host.get("PortBindings") and not host.get("Privileged"))
    if not common:
        return False
    if browser:
        expected = {"/workspace": ("bind", str(root.resolve()), False),
                    "/workspace/apps/web/node_modules": ("volume", "hito_node-modules", False),
                    "/evidence": ("bind", str(evidence.resolve()), True)}
        return (host["NetworkMode"] == f"container:{app}" and host["Memory"] == 1024 ** 3
                and len(mounts) == 3 and all(
                    mount["Destination"] in expected
                    and (mount["Type"], mount.get("Name") if mount["Type"] == "volume" else str(Path(mount["Source"]).resolve()), mount["RW"]) == expected[mount["Destination"]]
                    for mount in mounts)
                and "size=256m" in host["Tmpfs"].get("/tmp", ""))
    return (host["NetworkMode"] == "none" and set(container["NetworkSettings"]["Networks"]) == {"none"}
            and host["Memory"] == 512 * 1024 ** 2 and len(mounts) == 1
            and mounts[0]["Type"] == "bind" and mounts[0]["Destination"] == "/music"
            and Path(mounts[0]["Source"]).resolve() == (root / "fixtures/e2e-generated").resolve() and not mounts[0]["RW"]
            and "size=128m" in host["Tmpfs"].get("/data", "")
            and "mode=0700" in host["Tmpfs"].get("/data", "")
            and "size=16m" in host["Tmpfs"].get("/tmp", ""))


def evidence(directory, engine):
    summary = json.loads((directory / "summary.json").read_text())
    canaries = json.loads((directory / "canaries.json").read_text())
    if not isinstance(canaries, list) or not 30 <= len(canaries) <= 128 or any(not isinstance(value, str) or not value or len(value) > 16384 for value in canaries):
        raise ValueError("The bounded chooser privacy artifact is invalid")
    variants = set(canaries)
    for value in canaries:
        variants.add(json.dumps(value, ensure_ascii=False)[1:-1])
        variants.add(json.dumps(value, ensure_ascii=True)[1:-1])
    for name in ["server.log", "browser.log"]:
        file = directory / name
        if file.stat().st_size > 5 * 1024 * 1024 or any(value in file.read_text() for value in variants):
            raise ValueError("A chooser operational log violated its privacy bound")
    if summary["workflow"] != "provider-choice" or summary["browser"] != engine or summary["outcome"] != "passed" or summary["checkpoint"] != 10 or len(summary["checks"]) != 8 or not all(summary["checks"].values()):
        raise ValueError("The packaged chooser did not pass its numeric checkpoints")
    counts = summary["browserEvidence"]["counts"]
    for field in ["foreign", "forbidden", "audio", "prepare", "downloads", "implicitWrites", "pageErrors", "requestFailures", "unexpectedResponses", "privateConsole", "captureFailures"]:
        if counts[field] != 0:
            raise ValueError("The packaged chooser performed unexpected work")
    if not 0 < counts["requests"] <= 250 or counts["lists"] != 1 or counts["lyrics"] != 1 or counts["downloadReads"] < 1 or counts["absent"] != counts["downloadReads"] or counts["nativeCorrelations"] != counts["absent"] + counts["artwork404"]:
        raise ValueError("The chooser request or negative-response evidence is incomplete")
    providers = summary["providers"]
    if providers["primary"]["lyrics"] != 0 or providers["primary"]["descriptor"] != 0 or providers["attached"]["lyrics"] != 1 or providers["attached"]["descriptor"] != 1 or any(value["audio"] or value["unexpected"] or value["unsafeHeaders"] for value in providers.values()):
        raise ValueError("The chooser did not use only the selected provider")
    expected = {"Recording lyric provider list completed": ("recording_lyric_providers", "read"),
                "Recording lyric lookup completed": ("recording_lyrics", "ready")}
    completed = {}
    correlations = set()
    for line in (directory / "server.log").read_text().splitlines():
        event = json.loads(line)
        fields = event.get("fields", {})
        if any(key.lower() in ["password", "token", "cookie", "authorization", "cursor", "title", "name", "path", "body", "query", "url", "lyrics"] for key in fields):
            raise ValueError("A chooser event contains a private field")
        message = fields.get("message")
        if message not in expected:
            continue
        operation, outcome = expected[message]
        owner = [span for span in event.get("spans", []) if span.get("operation") == operation]
        if message in completed or fields.get("outcome") != outcome or not isinstance(fields.get("elapsed_ms"), int) or fields["elapsed_ms"] < 0 or len(owner) != 1 or not UUID.fullmatch(owner[0].get("request_id", "")) or owner[0]["request_id"] in correlations:
            raise ValueError("A chooser completion lacks its exact safe outcome and correlation")
        correlations.add(owner[0]["request_id"])
        completed[message] = outcome
    if set(completed) != set(expected):
        raise ValueError("The chooser completion evidence is missing")
    return summary
