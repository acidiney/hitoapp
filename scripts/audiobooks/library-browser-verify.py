import json
from pathlib import Path


def image_contract(image, image_id, source, inputs):
    labels = image.get("Config", {}).get("Labels", {})
    return (
        image.get("Id") == image_id
        and labels.get("org.opencontainers.image.revision") == source
        and labels.get("org.hito.product-input-sha256") == inputs
    )


def container_contract(container, image_id, project, music):
    host = container["HostConfig"]
    binds = container["Mounts"]
    return (
        container["Image"] == image_id
        and container["Config"]["User"] == "10001:10001"
        and container["Config"]["Labels"].get("com.docker.compose.project") == project
        and container["Config"]["Labels"].get("org.hito.owned-fixture") == "audiobook-library-browser"
        and host["ReadonlyRootfs"]
        and host["CapDrop"] == ["ALL"]
        and "no-new-privileges:true" in host["SecurityOpt"]
        and not host.get("PortBindings")
        and not host.get("Privileged")
        and "/data" in host["Tmpfs"]
        and len(binds) == 1
        and binds[0]["Type"] == "bind"
        and binds[0]["Destination"] == "/music"
        and Path(binds[0]["Source"]).resolve() == music.resolve()
        and not binds[0]["RW"]
    )


def evidence(directory, workflow, engine):
    summary = json.loads((directory / "summary.json").read_text())
    canaries = json.loads((directory / "canaries.json").read_text())
    if not isinstance(canaries, list) or not 20 <= len(canaries) <= 512 or any(not isinstance(value, str) or not value or len(value) > 16384 for value in canaries):
        raise ValueError("The bounded privacy artifact is invalid")
    for file in [directory / "server.log", directory / "browser.log"]:
        if file.stat().st_size > 5 * 1024 * 1024:
            raise ValueError("The owned log artifact exceeded its bound")
        text = file.read_text()
        if any(value in text for value in canaries):
            raise ValueError("A private canary appeared in an operational log")
    if summary["workflow"] != workflow or summary["browser"] != engine or summary["outcome"] != "passed" or not all(summary["checks"].values()):
        raise ValueError("The packaged browser workflow did not pass")
    transport = summary["transport"]
    if not 0 < transport["requests"] <= 250 or not 0 < summary["public"]["requests"] <= 180:
        raise ValueError("The packaged browser request budget changed")
    for field in ["apiBookReads", "sync", "audio", "source", "downloads", "foreign", "unexpectedWrites", "unexpectedReads", "failures", "preparationFailures", "invalid"]:
        if transport[field] != 0:
            raise ValueError("The browser performed unexpected work")
    if workflow == "privacy":
        controls = summary.get("controls", [])
        if [value["kind"] for value in controls] != ["javascript", "unowned_native", "owned_native"] or any(value["invalid"] != 0 or not 0 < value["requests"] <= 20 for value in controls):
            raise ValueError("The real browser privacy controls were incomplete")
        if any(value["privateConsole"] < 1 or value["nativeNetworkDiagnostics"] != 0 for value in controls[:2]) or controls[2]["privateConsole"] != 0 or controls[2]["nativeNetworkDiagnostics"] < 1 or controls[2]["staleWitnessRejected"] is not True:
            raise ValueError("The native diagnostic classifier did not preserve its rejection boundary")
        if transport["requests"] + sum(value["requests"] for value in controls) > 250:
            raise ValueError("The privacy controls exceeded the combined browser request budget")
    for line in (directory / "server.log").read_text().splitlines():
        value = json.loads(line)
        if not isinstance(value, dict) or value.get("level") not in ["TRACE", "DEBUG", "INFO", "WARN", "ERROR"]:
            raise ValueError("The packaged operational log is not structured")
        fields = value.get("fields", {})
        if any(key.lower() in ["password", "token", "cookie", "authorization", "cursor", "title", "name", "path", "body", "query", "url"] for key in fields):
            raise ValueError("The packaged log contains a private field")
    return summary
