import { createElement } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import KeepPlayedTracks from "../components/KeepPlayedTracks";

const PATH = "/api/v1/account/automatic-copies";
const GIB = 1024 ** 3;

function copies(keep = true, used = 1.2 * GIB, budget = 5 * GIB) {
  return {
    keep_played_tracks: keep,
    used_bytes: Math.round(used),
    budget_bytes: budget,
  };
}

async function control() {
  const found = await screen.findByRole("switch", {
    name: "Keep played tracks",
  });
  await waitFor(() => expect(found.hasAttribute("disabled")).toBe(false));
  return found;
}

test("Keep played tracks shows the saved choice and the space it uses against the account's budget", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string) => {
      expect(path).toBe(PATH);
      return Response.json(copies());
    }),
  );
  render(createElement(KeepPlayedTracks, { account: "a", native: false }));
  expect((await control()).getAttribute("aria-checked")).toBe("true");
  expect(screen.getByText("Space used")).toBeTruthy();
  expect(screen.getByText("1.2 GiB of 5 GiB")).toBeTruthy();
  expect(screen.getByText(/next two in your queue/)).toBeTruthy();
  expect(
    screen.getByText(/Turning this off removes these copies/),
  ).toBeTruthy();
  expect(screen.getByText(/Your Downloads stay/)).toBeTruthy();
});

test("a budget of zero says nothing can be kept", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => Response.json(copies(true, 0, 0))),
  );
  render(createElement(KeepPlayedTracks, { account: "a", native: false }));
  await control();
  expect(
    screen.getByText("Your administrator has set no space for played tracks."),
  ).toBeTruthy();
});

test("turning it off sends only the choice and stops showing space", async () => {
  const bodies: unknown[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (_path: string, options?: RequestInit) => {
      if (options?.method === "PUT") {
        bodies.push(JSON.parse(String(options.body)));
        return Response.json(copies(false, 0));
      }
      return Response.json(copies(true, 300 * 1024 ** 2));
    }),
  );
  render(createElement(KeepPlayedTracks, { account: "a", native: false }));
  await control();
  expect(screen.getByText("300 MiB of 5 GiB")).toBeTruthy();
  fireEvent.click(await control());
  await waitFor(async () =>
    expect((await control()).getAttribute("aria-checked")).toBe("false"),
  );
  expect(bodies).toEqual([{ keep_played_tracks: false }]);
  expect(screen.queryByText("Space used")).toBeNull();
});

test("a failed save keeps the old choice and says so", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (_path: string, options?: RequestInit) =>
      options?.method === "PUT"
        ? Response.json({ error: "Storage is busy" }, { status: 500 })
        : Response.json(copies()),
    ),
  );
  render(createElement(KeepPlayedTracks, { account: "a", native: false }));
  fireEvent.click(await control());
  expect((await screen.findByRole("alert")).textContent).toContain(
    "Storage is busy",
  );
  expect((await control()).getAttribute("aria-checked")).toBe("true");
});

test("a failed load offers a reload", async () => {
  let calls = 0;
  vi.stubGlobal(
    "fetch",
    vi.fn(async () =>
      ++calls === 1
        ? Response.json({ error: "Not now" }, { status: 500 })
        : Response.json(copies()),
    ),
  );
  render(createElement(KeepPlayedTracks, { account: "a", native: false }));
  fireEvent.click(
    await screen.findByRole("button", { name: "Reload played tracks" }),
  );
  expect((await control()).getAttribute("aria-checked")).toBe("true");
});

test("the desktop player only mentions copies after a full play", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => Response.json(copies())),
  );
  render(createElement(KeepPlayedTracks, { account: "a", native: true }));
  await control();
  expect(screen.queryByText(/next two in your queue/)).toBeNull();
  expect(screen.getByText(/after you play them to the end/)).toBeTruthy();
});
