"""Image restrictions and the metadata-only public protocol oracle."""

import json
import time


HEALTH = "curl -fsS --noproxy '*' --max-time 2 --output /dev/null http://127.0.0.1:8097/v1/manifest || exit 1"
COMMAND = [
    "--provider-token-file", "/run/secrets/all-debrid-provider-token",
    "--service-token-file", "/run/secrets/all-debrid-service-token",
    "--receipt-key-file", "/run/secrets/all-debrid-receipt-key",
    "--catalog-file", "/run/secrets/all-debrid-catalog.json", "--listen", "0.0.0.0:8097",
]


def require(condition, reason):
    if not condition:
        raise ValueError(reason)


def image_contract(docker, manifest):
    [image] = json.loads(docker.call("image", "inspect", docker.image))
    config = image["Config"]
    labels = config["Labels"] or {}
    require(image["Id"] == docker.image, "Packaged image identity changed")
    require(labels.get("org.opencontainers.image.revision") == manifest["source_id"] and
            labels.get("org.hito.product-input-sha256") == manifest["input_sha256"],
            "Image does not match the frozen source")
    require(config["User"] == "10001:10001" and config["StopSignal"] == "SIGTERM",
            "Image user or stop signal differs")
    require(config["Entrypoint"] == ["/usr/local/bin/hito-plugin-all-debrid"] and
            config["Cmd"] == COMMAND, "Image executable or private input arguments differ")
    require(config["Healthcheck"] == {
        "Test": ["CMD-SHELL", HEALTH], "Interval": 5000000000, "Timeout": 3000000000,
        "StartPeriod": 5000000000, "Retries": 3,
    }, "Image public-manifest healthcheck differs")
    require(set(config["ExposedPorts"]) == {"8097/tcp"}, "Unexpected exposed image port")


def container_contract(container, volume):
    host, config = container["HostConfig"], container["Config"]
    require(config["User"] == "10001:10001" and config["StopSignal"] == "SIGTERM" and
            config["StopTimeout"] == 30 and config["Cmd"] == COMMAND,
            "Service runtime identity or approved stop differs")
    require(host["ReadonlyRootfs"] and host["Init"] and host["NetworkMode"] == "none" and
            not host["PortBindings"] and not host["PublishAllPorts"],
            "Audit must have a read-only root, init, network none and no ports")
    require(host["CapDrop"] == ["ALL"] and not host["CapAdd"] and
            host["SecurityOpt"] == ["no-new-privileges:true"], "Service privilege bounds differ")
    require((host["Memory"], host["NanoCpus"], host["PidsLimit"]) == (268435456, 1000000000, 64),
            "Service resource bounds differ")
    require(host["RestartPolicy"]["Name"] == "no" and host["LogConfig"] == {
        "Type": "json-file", "Config": {"max-size": "10m", "max-file": "3"},
    }, "Service restart or log bounds differ")
    [mount] = container["Mounts"]
    require((mount["Type"], mount["Name"], mount["Destination"], mount["RW"]) ==
            ("volume", volume, "/run/secrets", False), "Private inputs are not the owned read-only volume")
    require(all(value is None for value in (container["NetworkSettings"]["Ports"] or {}).values()),
            "Audit unexpectedly published a port")


def wait_healthy(docker, identifier):
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        container = docker.inspect(identifier)
        require(container["State"]["Running"], "Service exited before health became ready")
        if container["State"]["Health"]["Status"] == "healthy":
            return
        time.sleep(0.2)
    raise ValueError("Public-manifest health did not become ready within its bound")


def request(docker, identifier, method, path, token=None, payload=None):
    options = {"url": "http://127.0.0.1:8097" + path, "request": method,
               "noproxy": "*", "connect-timeout": "1", "max-time": "2",
               "max-filesize": "65536", "write-out": "\n%{http_code}"}
    lines = ["silent", "show-error"]
    lines.extend(key + " = " + json.dumps(value) for key, value in options.items())
    headers = ["Content-Type: application/json", "x-hito-plugin-protocol: 1"]
    if token is not None:
        headers.append("Authorization: Bearer " + token)
    lines.extend("header = " + json.dumps(header) for header in headers)
    if payload is not None:
        lines.append("data-binary = " + json.dumps(json.dumps(payload)))
    output = docker.call("exec", "-i", identifier, "curl", "--config", "-",
                         data=("\n".join(lines) + "\n").encode(), limit=65536, timeout=5)
    body, status = output.rsplit(b"\n", 1)
    return int(status), json.loads(body)


def metadata_workflow(docker, identifier, inputs):
    status, manifest = request(docker, identifier, "GET", "/v1/manifest")
    require(status == 200 and manifest == {
        "id": "org.hito.all-debrid", "name": "AllDebrid curated WAV", "protocol_version": 1,
        "capabilities": ["search", "sources", "source_preparation"],
        "content_policy": {"source_persistence": "playback_only"},
    }, "Public manifest differs")
    search = {"query": inputs.recording["title"], "cursor": None, "limit": 1}
    for token in (None, inputs.wrong, inputs.provider):
        status, body = request(docker, identifier, "POST", "/v1/search", token, search)
        require(status == 401 and body == {"error": "Plugin authorization is required"},
                "Metadata authorization separation failed")
    status, body = request(docker, identifier, "POST", "/v1/search?" + inputs.prefix + "=private",
                           inputs.service, search)
    require(status == 200 and body == {"items": [inputs.recording], "next_cursor": None},
            "Authenticated search differs from the exact owned catalog")
    status, body = request(docker, identifier, "POST", "/v1/recordings/detail", inputs.service,
                           {"recording_id": inputs.recording["id"]})
    require(status == 200 and body == inputs.recording, "Authenticated detail differs from the owned catalog")
