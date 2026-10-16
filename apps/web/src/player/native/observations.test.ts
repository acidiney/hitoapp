import { expect, test } from "vitest";
import { ObservationError, readObservations } from "./observations";

test("observations decode fragmented UTF-8 and CRLF frames without emitting keepalives", async () => {
  const bytes = new TextEncoder().encode(
    ': keepalive\r\n\r\ndata: {"selection":1,"value":"ação"}\r\n\r\n',
  );
  const response = new Response(
    new ReadableStream({
      start(controller) {
        for (const byte of bytes) controller.enqueue(new Uint8Array([byte]));
        controller.close();
      },
    }),
    { headers: { "content-type": "text/event-stream" } },
  );
  const values: unknown[] = [];
  await readObservations(response, new AbortController().signal, (value) =>
    values.push(value),
  );
  expect(values).toEqual([{ selection: 1, value: "ação" }]);
});

test("invalid or oversized observations close their body with a safe protocol error", async () => {
  for (const bytes of [
    new Uint8Array([255]),
    new TextEncoder().encode("data: {broken}\n\n"),
    new TextEncoder().encode("data: null\n\n"),
    new TextEncoder().encode(":" + "x".repeat(65537)),
  ]) {
    let cancelled = false;
    const response = new Response(
      new ReadableStream({
        start(controller) {
          controller.enqueue(bytes);
        },
        cancel() {
          cancelled = true;
        },
      }),
      { headers: { "content-type": "text/event-stream" } },
    );
    await expect(
      readObservations(response, new AbortController().signal, () => {}),
    ).rejects.toBeInstanceOf(ObservationError);
    expect(cancelled).toBe(true);
  }
});
