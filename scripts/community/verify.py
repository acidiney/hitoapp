#!/usr/bin/env python3
"""Bounded packaged HTTP and private lifecycle-log verification."""

import json
import os
from pathlib import Path
import re
import signal
import socket
import stat
import sys
import time
import uuid


STATE = Path("/var/lib/hito-community-verify")
CREDENTIALS = Path("/run/secrets")
SOURCE = {"provider": "jamendo", "recording_id": "42"}
OTHER = {"provider": "youtube", "recording_id": "HitoTone001"}
MAX_BODY = 262_144


class VerificationFailed(Exception):
    pass


def require(condition):
    if not condition:
        raise VerificationFailed()


def expire(_signal, _frame):
    raise VerificationFailed()


def remaining(deadline):
    duration = deadline - time.monotonic()
    require(duration > 0)
    return duration


def private_directory(path):
    metadata = path.lstat()
    require(stat.S_ISDIR(metadata.st_mode))
    require(metadata.st_uid == os.geteuid())
    require(stat.S_IMODE(metadata.st_mode) == 0o700)


def read_private(path, limit):
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor, "rb") as entry:
        metadata = os.fstat(entry.fileno())
        require(stat.S_ISREG(metadata.st_mode) and metadata.st_nlink == 1)
        require(metadata.st_uid == os.geteuid())
        require(stat.S_IMODE(metadata.st_mode) in (0o400, 0o600))
        require(metadata.st_size <= limit)
        content = entry.read(limit + 1)
        require(len(content) <= limit)
        return content


def write_private(name, content):
    descriptor = os.open(
        STATE / name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600
    )
    with os.fdopen(descriptor, "wb") as entry:
        entry.write(content)
        entry.flush()
        os.fsync(entry.fileno())
    descriptor = os.open(STATE, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def encoded(value):
    return json.dumps(value, separators=(",", ":"), ensure_ascii=True).encode("ascii")


def valid_uuid(value):
    require(isinstance(value, str))
    parsed = uuid.UUID(value)
    require(parsed.version == 4 and str(parsed) == value)


def credential(value):
    require(isinstance(value, dict) and set(value) == {"reporter_id", "token"})
    valid_uuid(value["reporter_id"])
    require(re.fullmatch(r"[0-9a-f]{64}", value["token"]) is not None)
    return value


class Connection:
    def __init__(self, deadline):
        self.deadline = deadline
        self.stream = socket.create_connection(("127.0.0.1", 8095), remaining(deadline))

    def __enter__(self):
        return self

    def __exit__(self, *_args):
        self.stream.close()

    def send(self, data):
        self.stream.settimeout(remaining(self.deadline))
        self.stream.sendall(data)

    def read(self, size):
        result = bytearray()
        while len(result) < size:
            self.stream.settimeout(remaining(self.deadline))
            part = self.stream.recv(size - len(result))
            require(bool(part))
            result.extend(part)
        return bytes(result)

    def start(self, method, path, token, body, held=False, private=False):
        require(len(body) <= 16_384)
        headers = [
            f"{method} {path} HTTP/1.1",
            "Host: localhost",
            "Connection: close",
            "Content-Type: application/json",
            f"Content-Length: {len(body)}",
        ]
        if token is not None:
            require(re.fullmatch(r"[0-9a-f]{64}", token) is not None)
            headers.append(f"Authorization: Bearer {token}")
        if held:
            headers.append("Expect: 100-continue")
        if private:
            headers.extend([
                "Cookie: private-community-verification-cookie",
                "X-Private-Request: private-community-verification-header",
            ])
        self.send(("\r\n".join(headers) + "\r\n\r\n").encode("ascii"))
        if not held:
            self.send(body)

    def head(self):
        header = bytearray()
        while not header.endswith(b"\r\n\r\n"):
            require(len(header) < 16_384)
            header.extend(self.read(1))
        lines = header[:-4].decode("ascii").split("\r\n")
        status = lines[0].split(" ", 2)
        require(status[0] == "HTTP/1.1" and len(status[1]) == 3)
        require(status[1].isdigit())
        headers = {}
        for line in lines[1:]:
            name, value = line.split(":", 1)
            name = name.lower()
            require(name not in headers)
            headers[name] = value.strip()
        return int(status[1]), headers

    def finish(self, expected):
        status, headers = self.head()
        require(status == expected)
        require(headers.get("cache-control") == "no-store")
        valid_uuid(headers.get("x-request-id"))
        self.request_id = headers["x-request-id"]
        require(headers.get("content-type", "").split(";", 1)[0] == "application/json")
        require("transfer-encoding" not in headers)
        length = headers.get("content-length", "")
        require(length.isascii() and length.isdigit() and len(length) <= 6)
        length = int(length)
        require(0 < length <= MAX_BODY)
        value = json.loads(self.read(length))
        require(isinstance(value, dict))
        remaining(self.deadline)
        return value


def request(deadline, method, path, expected, token=None, value=None, private=False):
    with Connection(deadline) as connection:
        connection.start(method, path, token, b"" if value is None else encoded(value), private=private)
        return connection.finish(expected)


def schema(deadline):
    document = request(deadline, "GET", "/v1/openapi.json", 200)
    require(document["openapi"].startswith("3."))
    require(set(document["paths"]) == {
        "/v1/openapi.json", "/v1/admin/reporters", "/v1/outcomes", "/v1/rankings"
    })
    for path, authority in (("/v1/admin/reporters", "administratorBearer"),
                            ("/v1/outcomes", "reporterBearer")):
        operation = document["paths"][path]["post"]
        require(operation["security"] == [{authority: []}])
        scheme = document["components"]["securitySchemes"][authority]
        require(scheme["type"] == "http" and scheme["scheme"] == "bearer")
    return document


def rankings(deadline):
    ranked = request(deadline, "POST", "/v1/rankings", 200,
                     value={"candidates": [OTHER, SOURCE]})
    require(ranked == {"ranked": [
        {"source": SOURCE, "successful_reporters": 1},
        {"source": OTHER, "successful_reporters": 0},
    ]})
    filtered = request(deadline, "POST", "/v1/rankings", 200,
                       value={"candidates": [OTHER]})
    require(filtered == {"ranked": [{"source": OTHER, "successful_reporters": 0}]})


def first(deadline, admin):
    require(not any(STATE.iterdir()))
    document = schema(deadline)
    require(admin not in json.dumps(document))
    reporter = credential(request(deadline, "POST", "/v1/admin/reporters", 201,
                                  admin, {}))
    for token in (None, reporter["token"]):
        require(request(deadline, "POST", "/v1/admin/reporters", 401,
                        token, {}) == {"error": "unauthorized"})
    outcome = {"submission_id": str(uuid.uuid4()), "source": SOURCE,
               "outcome": "successful_playback"}
    receipt = request(deadline, "POST", "/v1/outcomes", 201, reporter["token"], outcome)
    require(set(receipt) == {"receipt_id", "submission_id", "status"})
    valid_uuid(receipt["receipt_id"])
    require(receipt["submission_id"] == outcome["submission_id"])
    require(receipt["status"] == "accepted")
    rankings(deadline)
    rejection = request(
        deadline, "POST",
        "/v1/admin/reporters?private-community-verification-query=private-community-verification-value",
        422, admin, {"private-community-verification-body": "private-community-verification-value"},
        private=True,
    )
    require(rejection == {"error": "invalid_input"})
    require(request(deadline, "POST", "/v1/rankings", 400,
                    value={"candidates": [SOURCE, SOURCE]}) == {"error": "invalid_input"})
    write_private("accepted.json", encoded({"reporter": reporter, "outcome": outcome,
                                            "receipt": receipt}))


def restart(deadline, _admin):
    require(not os.path.lexists(STATE / "restart.ok"))
    schema(deadline)
    saved = json.loads(read_private(STATE / "accepted.json", 16_384))
    require(set(saved) == {"reporter", "outcome", "receipt"})
    reporter = credential(saved["reporter"])
    replay = request(deadline, "POST", "/v1/outcomes", 200, reporter["token"], saved["outcome"])
    require(replay == saved["receipt"])
    rankings(deadline)
    write_private("restart.ok", b"restart verified\n")


def drain(deadline, admin):
    require(read_private(STATE / "restart.ok", 32) == b"restart verified\n")
    for name in ("drain-held", "drain-release", "drain-reporter.json"):
        require(not os.path.lexists(STATE / name))
    body = b"{}\n"
    with Connection(deadline) as connection:
        connection.start("POST", "/v1/admin/reporters", admin, body, held=True)
        status, _headers = connection.head()
        require(status == 100)
        write_private("drain-held", b"held\n")
        try:
            while not os.path.lexists(STATE / "drain-release"):
                time.sleep(min(0.02, remaining(deadline)))
            require(read_private(STATE / "drain-release", 0) == b"")
            connection.send(body)
            reporter = credential(connection.finish(201))
            write_private("drain-reporter.json", encoded({
                "reporter": reporter, "request_id": connection.request_id,
            }))
        finally:
            (STATE / "drain-held").unlink(missing_ok=True)
            (STATE / "drain-release").unlink(missing_ok=True)


def logs(deadline, admin):
    accepted = json.loads(read_private(STATE / "accepted.json", 16_384))
    drained = json.loads(read_private(STATE / "drain-reporter.json", 16_384))
    require(set(drained) == {"reporter", "request_id"})
    reporter = credential(accepted["reporter"])
    drain_reporter = credential(drained["reporter"])
    valid_uuid(drained["request_id"])
    captured = sys.stdin.buffer.read(2_097_153)
    require(0 < len(captured) <= 2_097_152)
    lines = captured.decode("utf-8").splitlines()
    require(0 < len(lines) <= 10_000)
    events = [json.loads(line) for line in lines]
    serialized = json.dumps(events, ensure_ascii=False)
    for forbidden in (
        admin, reporter["token"], drain_reporter["token"],
        "private-community-verification", "HitoTone001",
        str(CREDENTIALS), str(STATE), "/var/lib/hito-community",
        "hito-community-admin-token",
    ):
        require(forbidden not in serialized)
    lifecycle = (
        "community_service_ready", "community_shutdown_requested",
        "community_service_state_closed", "community_service_stopped",
    )
    runtime = []
    for index, event in enumerate(events):
        require(isinstance(event, dict) and isinstance(event["fields"], dict))
        target = event["target"]
        require(target == "hito_community" or target.startswith("hito_community::"))
        name = event["fields"]["event"]
        require(name != "community_service_failed")
        if name in lifecycle:
            spans = [span for span in event.get("spans", [])
                     if span.get("name") == "community_service"]
            require(len(spans) == 1)
            service_id = spans[0]["service_id"]
            valid_uuid(service_id)
            runtime.append((index, name, service_id))
        remaining(deadline)
    require([entry[1] for entry in runtime] == list(lifecycle) * 2)
    instances = []
    library_close_indices = []
    for offset in (0, 4):
        ready, requested, closed, stopped = runtime[offset:offset + 4]
        require(len({entry[2] for entry in (ready, requested, closed, stopped)}) == 1)
        instances.append(ready[2])
        for entry, outcome in zip((ready, requested, closed, stopped),
                                  ("ready", "draining", "closed", "stopped")):
            require(events[entry[0]]["fields"]["outcome"] == outcome)
        require(events[requested[0]]["fields"]["signal"] == "SIGTERM")
        library_closed = [index for index, event in enumerate(events)
                          if requested[0] < index < closed[0]
                          and event["fields"]["event"] == "community_state_closed"]
        require(len(library_closed) == 1)
        library_close_indices.append(library_closed[0])
    require(instances[0] != instances[1])
    requested, closed = runtime[5][0], runtime[6][0]
    matching = [index for index, event in enumerate(events)
                if requested < index < closed
                and event["fields"]["event"] == "community_http_response"
                and event["fields"].get("status") == 201
                and any(span.get("name") == "community_http_request"
                        and span.get("request_id") == drained["request_id"]
                        and span.get("route") == "/v1/admin/reporters"
                        and span.get("method") == "POST"
                        for span in event.get("spans", []))]
    require(len(matching) == 1)
    require(matching[0] < library_close_indices[-1])


def main():
    phases = {"first": first, "restart": restart, "drain": drain, "logs": logs}
    require(len(sys.argv) == 2 and sys.argv[1] in phases)
    signal.signal(signal.SIGALRM, expire)
    signal.setitimer(signal.ITIMER_REAL, 10)
    deadline = time.monotonic() + 10
    os.umask(0o077)
    volumes = [os.environ.get(name, "") for name in (
        "HITO_COMMUNITY_STATE_VOLUME", "HITO_COMMUNITY_CREDENTIAL_VOLUME",
        "HITO_COMMUNITY_VERIFY_VOLUME",
    )]
    require(all(volumes) and len(set(volumes)) == 3)
    require(os.geteuid() == 10001)
    private_directory(STATE)
    private_directory(CREDENTIALS)
    admin = read_private(CREDENTIALS / "hito-community-admin-token", 66)
    require(re.fullmatch(rb"[0-9a-f]{64}(?:\r?\n)?", admin) is not None)
    phase = sys.argv[1]
    phases[phase](deadline, admin[:64].decode("ascii"))
    remaining(deadline)
    signal.setitimer(signal.ITIMER_REAL, 0)
    print(f"community verification {phase} passed", flush=True)


if __name__ == "__main__":
    try:
        main()
    except (Exception, KeyboardInterrupt):
        signal.setitimer(signal.ITIMER_REAL, 0)
        print("community verification failed", file=sys.stderr, flush=True)
        sys.exit(1)
