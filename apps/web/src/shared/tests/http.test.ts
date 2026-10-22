import { expect, test, vi } from "vitest";
import { sessionApi } from "../../api";

test("HTTP logs correlate server responses without retaining authentication contents", async () => {
  const info = vi.spyOn(console, "info").mockImplementation(() => {});
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  vi.stubGlobal(
    "fetch",
    vi.fn().mockResolvedValue(
      new Response(JSON.stringify({ error: "private-server-error" }), {
        status: 401,
        headers: { "x-request-id": "request-456" },
      }),
    ),
  );
  await expect(
    sessionApi.signIn(false, "private-username", "private-password"),
  ).rejects.toThrow("private-server-error");
  expect(warn).toHaveBeenCalledWith(
    "[hito]",
    expect.objectContaining({
      event: "http.failed",
      request_id: "request-456",
      status: 401,
      action: "session.login",
    }),
  );
  const logged = JSON.stringify([...info.mock.calls, ...warn.mock.calls]);
  expect(logged).not.toContain("private-username");
  expect(logged).not.toContain("private-password");
  expect(logged).not.toContain("private-server-error");
});
