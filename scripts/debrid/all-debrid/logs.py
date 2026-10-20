"""Exact safe workflow fields, correlations and metadata request outcomes."""

import json
import uuid

from protocol import require


def keys(value, expected):
    require(isinstance(value, dict) and set(value) == set(expected), "Unexpected structured log fields")


def correlation(value):
    identifier = uuid.UUID(value)
    require(identifier.version == 4, "Workflow correlation is not UUIDv4")
    return identifier


def metadata_lifecycle(output):
    require(len(output) <= 65536, "TRACE log output exceeded its bound")
    text = output.decode()
    for private in ("owned-private-package-", "/run/secrets", "all-debrid-provider-token",
                    "all-debrid-service-token", "all-debrid-receipt-key", "all-debrid-catalog.json",
                    "http://", "https://", "http%3A", "https%3A"):
        require(private not in text, "TRACE logs exposed private input or request material")
    events = [json.loads(line) for line in text.splitlines()]
    lifecycle, requests = [], []
    service_ids, request_ids = set(), set()
    for event in events:
        keys(event, ["timestamp", "level", "fields", "target", "span", "spans"])
        fields, spans = event["fields"], event["spans"]
        require(isinstance(event["timestamp"], str) and spans and event["span"] == spans[-1],
                "Structured workflow ownership missing")
        require(type(fields.get("elapsed_ms")) is int and 0 <= fields["elapsed_ms"] <= 60000,
                "Workflow timing missing or outside the audit bound")
        name = fields["event"]
        if name in ("all_debrid_service_ready", "all_debrid_shutdown_requested", "all_debrid_service_stopped"):
            require(event["level"] == "INFO" and event["target"] == "hito_plugin_all_debrid::runtime"
                    and len(spans) == 1, "Lifecycle log owner differs")
            keys(spans[0], ["name", "service_id"])
            require(spans[0]["name"] == "all_debrid_service", "Lifecycle span differs")
            service_ids.add(correlation(spans[0]["service_id"]))
            if name == "all_debrid_shutdown_requested":
                keys(fields, ["event", "signal", "outcome", "elapsed_ms"])
                require((fields["signal"], fields["outcome"]) == ("SIGTERM", "draining"),
                        "Shutdown did not follow the approved SIGTERM path")
            else:
                keys(fields, ["event", "outcome", "elapsed_ms"])
                require(fields["outcome"] == ("ready" if name.endswith("ready") else "stopped"),
                        "Lifecycle outcome differs")
            lifecycle.append(name)
        elif name == "all_debrid_plugin_request_finished":
            require(event["level"] == "DEBUG" and event["target"] == "hito_plugin_all_debrid::http"
                    and 1 <= len(spans) <= 2, "Request log owner differs")
            keys(fields, ["event", "outcome", "status", "elapsed_ms"])
            keys(spans[-1], ["name", "request_id", "operation"])
            require(spans[-1]["name"] == "all_debrid_plugin_request", "Request span differs")
            identifier = correlation(spans[-1]["request_id"])
            require(identifier not in request_ids, "Request correlation repeated")
            request_ids.add(identifier)
            for parent in spans[:-1]:
                keys(parent, ["name", "service_id"])
                require(parent["name"] == "all_debrid_service" and
                        correlation(parent["service_id"]) in service_ids, "Request service correlation differs")
            status = fields["status"]
            require(type(status) is int and status in (200, 401), "Unexpected metadata status")
            require(fields["outcome"] == ("success" if status == 200 else "unauthorized"),
                    "Request outcome differs")
            requests.append((spans[-1]["operation"], status))
        else:
            raise ValueError("Metadata-only audit entered another provider or media workflow")
    require(lifecycle == ["all_debrid_service_ready", "all_debrid_shutdown_requested",
                          "all_debrid_service_stopped"] and len(service_ids) == 1,
            "Lifecycle did not become ready and join SIGTERM exactly once")
    require([request for request in requests if request[0] != "manifest"] == [
        ("search", 401), ("search", 401), ("search", 401), ("search", 200), ("detail", 200),
    ], "Metadata request outcomes differ")
    manifests = [request for request in requests if request[0] == "manifest"]
    require(2 <= len(manifests) <= 8 and all(status == 200 for _, status in manifests),
            "Public manifest probes or health request bound differ")
    require(len(events) == 3 + len(requests), "Unexpected TRACE workflow events")
    return {"events": len(events), "public_manifest_requests": len(manifests),
            "authenticated_search": 1, "authenticated_detail": 1, "authorization_rejections": 3,
            "service_id": str(next(iter(service_ids))), "request_ids": len(request_ids)}
