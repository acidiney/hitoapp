import { createElement } from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import SharedCorrectionsSettings from "../components/SharedCorrectionsSettings";
import type { components } from "../../../generated";

type Schema = components["schemas"];

afterEach(() => cleanup());

function item(
  id: string,
  applied: boolean,
  revision: number,
  retired = 0,
): Schema["SharedArtistCorrection"] {
  return {
    id,
    kind: id.startsWith("split") ? "split" : "unlink",
    recordings: 2,
    retired,
    applied,
    revision,
    promoted_by_me: false,
  };
}

function owned(options: { total?: number; lostFirst?: boolean } = {}) {
  const items = [item("split-one", true, 1, 1), item("unlink-two", false, 2)];
  const bodies: { path: string; body: Record<string, unknown> }[] = [];
  const reads: string[] = [];
  let lost = options.lostFirst ?? false;
  const fetch = vi.fn(async (path: string, init?: RequestInit) => {
    const method = init?.method ?? "GET";
    if (method === "GET" && path.startsWith("/api/v1/artist-corrections?")) {
      reads.push(path);
      const offset = Number(
        new URL(path, "http://hito.local").searchParams.get("offset"),
      );
      return Response.json({
        revision: 2,
        items: offset === 0 ? items : [],
        total: options.total ?? 2,
        offset,
        limit: 25,
      } satisfies Schema["SharedArtistCorrections"]);
    }
    if (method === "POST") {
      const body = JSON.parse(init?.body as string);
      bodies.push({ path, body });
      if (lost) {
        lost = false;
        throw new TypeError("network lost");
      }
      const id = path.split("/")[4];
      const current = items.find((candidate) => candidate.id === id)!;
      expect(body.expected_revision).toBe(current.revision);
      current.applied = !current.applied;
      current.revision += 1;
      return Response.json({
        request_id: body.request_id,
        correction_id: id,
        revision: current.revision,
        applied: current.applied,
        recordings: 2,
      } satisfies Schema["SharedArtistCorrectionReceipt"]);
    }
    throw new Error(`Unexpected request ${method} ${path}`);
  });
  vi.stubGlobal("fetch", fetch);
  return { bodies, reads };
}

test("an administrator withdraws and restores shared corrections from the list", async () => {
  const { bodies } = owned();
  render(createElement(SharedCorrectionsSettings, { account: "admin" }));
  expect(
    await screen.findByText(
      /Split · 2 recordings · 1 no longer matches the file tags · Applied/,
    ),
  ).toBeTruthy();
  fireEvent.click(
    screen.getByRole("button", { name: "Withdraw shared correction 1" }),
  );
  await screen.findByRole("button", { name: "Restore shared correction 1" });
  fireEvent.click(
    screen.getByRole("button", { name: "Restore shared correction 2" }),
  );
  await screen.findByRole("button", { name: "Withdraw shared correction 2" });
  expect(bodies.map(({ path }) => path)).toEqual([
    "/api/v1/artist-corrections/split-one/undo",
    "/api/v1/artist-corrections/unlink-two/undo",
  ]);
  expect(bodies[0].body).toEqual({
    request_id: expect.any(String),
    expected_revision: 1,
  } satisfies Schema["UndoSharedArtistCorrection"]);
  expect(JSON.stringify(bodies)).not.toContain("title");
});

test("a lost acknowledgement retries the same withdraw request, and long lists page", async () => {
  const { bodies, reads } = owned({ total: 60, lostFirst: true });
  render(createElement(SharedCorrectionsSettings, { account: "admin" }));
  fireEvent.click(
    await screen.findByRole("button", { name: "Withdraw shared correction 1" }),
  );
  fireEvent.click(
    await screen.findByRole("button", {
      name: "Retry the same shared correction request",
    }),
  );
  await screen.findByRole("status");
  expect(bodies).toHaveLength(2);
  expect(bodies[1]).toEqual(bodies[0]);
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  await waitFor(() =>
    expect(reads.at(-1)).toBe("/api/v1/artist-corrections?offset=25&limit=25"),
  );
});
