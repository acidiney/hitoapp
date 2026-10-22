import { expect, test, vi } from "vitest";
import { HttpError, request } from "../http";

function stalledFailure() {
  const cancel = vi.fn();
  const body = new ReadableStream<Uint8Array>({ cancel });
  const response = new Response(body, { status: 503 });
  const json = vi.spyOn(response, "json");
  return { response, cancel, json };
}

test("an error body is cancelled unread when the caller does not want it", async () => {
  const { response, cancel, json } = stalledFailure();
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(response));
  const failure = await request("/sources/x/successful-playback", {
    action: "community.playback.report",
    method: "POST",
    errorBody: false,
  }).catch((error: unknown) => error);
  expect(failure).toBeInstanceOf(HttpError);
  expect((failure as HttpError).status).toBe(503);
  expect(json).not.toHaveBeenCalled();
  expect(cancel).toHaveBeenCalled();
});

test("an error body is parsed for its message by default", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn().mockResolvedValue(
      new Response(JSON.stringify({ error: "Busy", code: "busy" }), {
        status: 429,
      }),
    ),
  );
  const failure = await request("/x", { action: "x.read" }).catch(
    (error: unknown) => error,
  );
  expect(failure).toMatchObject({ status: 429, message: "Busy", code: "busy" });
});
