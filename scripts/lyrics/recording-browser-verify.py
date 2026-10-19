import hashlib
import json
from pathlib import Path
import re


def overlay(root):
    paths = [*root.glob("scripts/lyrics/recording-browser*"),
             root / "apps/web/e2e/recording-lyrics.spec.ts",
             *root.glob("apps/web/e2e/fixtures/recording-lyrics-*.ts")]
    files = [[str(path.relative_to(root)), hashlib.sha256(path.read_bytes()).hexdigest()]
             for path in sorted(paths) if path.is_file()]
    if not 8 <= len(files) <= 16:
        raise ValueError("The bounded lyric gate overlay is incomplete")
    return {"files": files, "sha256": hashlib.sha256(json.dumps(files, separators=(",", ":")).encode()).hexdigest()}


def image_contract(image, image_id, source, inputs):
    labels = image.get("Config", {}).get("Labels", {})
    return (image.get("Id") == image_id
            and labels.get("org.opencontainers.image.revision") == source
            and labels.get("org.hito.product-input-sha256") == inputs)


def container_contract(container, image_id, project, music):
    host, binds = container["HostConfig"], container["Mounts"]
    return (container["Image"] == image_id and container["Config"]["User"] == "10001:10001"
            and container["Config"]["Labels"].get("com.docker.compose.project") == project
            and container["Config"]["Labels"].get("org.hito.owned-fixture") == "recording-lyrics-browser"
            and host["ReadonlyRootfs"] and host["CapDrop"] == ["ALL"]
            and "no-new-privileges:true" in host["SecurityOpt"]
            and not host.get("PortBindings") and not host.get("Privileged")
            and "/data" in host["Tmpfs"] and len(binds) == 1
            and binds[0]["Type"] == "bind" and binds[0]["Destination"] == "/music"
            and Path(binds[0]["Source"]).resolve() == music.resolve() and not binds[0]["RW"])


def evidence(directory, workflow, engine):
    summary = json.loads((directory / "summary.json").read_text())
    canaries = json.loads((directory / "canaries.json").read_text())
    if not isinstance(canaries, list) or not 20 <= len(canaries) <= 128 or any(not isinstance(value, str) or not value or len(value) > 16384 for value in canaries):
        raise ValueError("The bounded lyric privacy artifact is invalid")
    variants = set(canaries)
    for value in canaries:
        variants.add(json.dumps(value, ensure_ascii=False)[1:-1])
        variants.add(json.dumps(value, ensure_ascii=True)[1:-1])
    for name in ["server.log", "browser.log"]:
        file = directory / name
        if file.stat().st_size > 5 * 1024 * 1024 or any(value in file.read_text() for value in variants):
            raise ValueError("An owned lyric operational log violated its privacy bound")
    if summary["workflow"] != workflow or summary["browser"] != engine or summary["outcome"] != "passed" or not summary["checks"] or not all(summary["checks"].values()):
        raise ValueError("The packaged lyric workflow did not pass")
    counts = summary["browserEvidence"]["counts"]
    for field in ["foreign", "forbidden", "audio", "prepare", "downloads", "implicitWrites", "pageErrors", "privateConsole", "captureFailures", "javascriptErrors", "unexpectedRequestFailures"]:
        if counts[field] != 0:
            raise ValueError("The packaged lyric browser performed unexpected work")
    if not 0 < counts["requests"] <= 250 or counts["downloadReads"] < 1 or counts["absent"] != counts["downloadReads"]:
        raise ValueError("The lyric request or negative download evidence is invalid")
    completions = []
    for line in (directory / "server.log").read_text().splitlines():
        value = json.loads(line)
        fields = value.get("fields", {})
        if any(key.lower() in ["password", "token", "cookie", "authorization", "cursor", "title", "name", "path", "body", "query", "url", "lyrics"] for key in fields):
            raise ValueError("A lyric operational event contains a private field")
        if fields.get("message") == "Recording lyric lookup completed":
            completions.append(value)
    expected = ["ready", "not_found"] if workflow == "reading" else ["cancelled", "ready", "cancelled", "ready"]
    actual = [value["fields"].get("outcome") for value in completions]
    (directory / "host-outcomes.json").write_text(json.dumps({"expected": expected, "actual": actual}))
    if sorted(actual) != sorted(expected):
        raise ValueError("The packaged host lyric completion outcomes are incomplete")
    correlations = set()
    for value in completions:
        fields = value["fields"]
        spans = value.get("spans", [])
        owner = [span for span in spans if span.get("operation") == "recording_lyrics"]
        if len(owner) != 1 or not re.fullmatch(r"[a-f0-9]{8}-[a-f0-9]{4}-4[a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}", owner[0].get("request_id", "")) or owner[0]["request_id"] in correlations or not isinstance(fields.get("elapsed_ms"), int):
            raise ValueError("The packaged lyric completion lacks its safe correlation")
        correlations.add(owner[0]["request_id"])
    return summary
