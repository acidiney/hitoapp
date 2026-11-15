import { expect, test } from "vitest";
import { confirmedRecordings } from "../api/trackListings";

function response(applied: string | null) {
  const headers = new Headers({ "content-type": "application/json" });
  if (applied) headers.set("Hito-Recordings-Availability", applied);
  return new Response(
    JSON.stringify({ items: [], total: 0, offset: 0, limit: 50 }),
    { headers },
  );
}

test("a filtered page is accepted only when the server says it applied that filter", async () => {
  await expect(
    confirmedRecordings(response("no_source"), "no_source"),
  ).resolves.toMatchObject({ total: 0 });
  await expect(
    confirmedRecordings(response(null), "no_source"),
  ).rejects.toThrow(
    "This server didn't apply the filter. Update the server to use it.",
  );
  await expect(
    confirmedRecordings(response("all"), "no_source"),
  ).rejects.toThrow("This server didn't apply the filter");
});

test("an unfiltered page needs no acknowledgement", async () => {
  await expect(
    confirmedRecordings(response(null), null),
  ).resolves.toMatchObject({ items: [] });
});
