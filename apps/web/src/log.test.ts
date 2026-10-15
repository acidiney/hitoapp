import { expect, test, vi } from "vitest";
import { createLogger, type LogFields } from "./log";

test("logs retain request correlation and discard sensitive or unknown fields", () => {
  const sink = vi.fn();
  const log = createLogger("debug", sink);
  const fields = {
    request_id: "request-123",
    status: 401,
    password: "secret",
    token: "bearer-secret",
    body: { username: "private-user" },
    title: "private-title",
    url: "https://provider.test/stream?token=signed-secret",
    path: "/home/private/music",
    query: "private-search",
  };
  log.info("http.response", fields as LogFields);
  expect(sink).toHaveBeenCalledWith("info", "http.response", {
    request_id: "request-123",
    status: 401,
  });
});

test("configured levels suppress frequent events and invalid values", () => {
  const sink = vi.fn();
  const log = createLogger("warn", sink);
  log.debug("playback.position", { position_ms: 123 });
  log.info("http.response", { status: 200 });
  log.warn("http.failed", { duration_ms: Infinity, status: 503 });
  expect(sink).toHaveBeenCalledTimes(1);
  expect(sink).toHaveBeenCalledWith("warn", "http.failed", { status: 503 });
});
