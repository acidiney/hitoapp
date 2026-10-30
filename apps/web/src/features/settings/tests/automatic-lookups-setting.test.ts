import { createElement } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import AutomaticLookupsSettings from "../../metadata/components/AutomaticLookupsSettings";

const SETTINGS = "/api/v1/metadata/settings";
const AUTOMATION = "/api/v1/metadata/automation";
const LABELS = [
  "Look up artists on MusicBrainz automatically",
  "Fetch album artwork automatically",
  "Search connected services automatically",
];

function settings(over: Record<string, boolean> = {}) {
  return {
    revision: 1,
    enabled: true,
    providers: [{ kind: "builtin_musicbrainz" }],
    automatic_artist_lookup: true,
    automatic_artwork: true,
    automatic_service_search: true,
    ...over,
  };
}

afterEach(() => vi.unstubAllGlobals());

const toggle = (name: string) => screen.findByRole("switch", { name });

test("three switches show the server choices and say what is sent and that they are on by default", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => Response.json(settings({ automatic_artwork: false }))),
  );
  render(createElement(AutomaticLookupsSettings, { account: "a" }));
  expect((await toggle(LABELS[0])).getAttribute("aria-checked")).toBe("true");
  expect((await toggle(LABELS[1])).getAttribute("aria-checked")).toBe("false");
  expect((await toggle(LABELS[2])).getAttribute("aria-checked")).toBe("true");
  expect(screen.getAllByText(/on by default/i)).toHaveLength(3);
  expect(
    screen.getByText(/never sends album titles, file paths or audio/),
  ).toBeTruthy();
});

test("a switch sends only its own field to the automation endpoint and shows the saved answer", async () => {
  const bodies: unknown[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, options?: RequestInit) => {
      if (path === SETTINGS) return Response.json(settings());
      expect(path).toBe(AUTOMATION);
      expect(options?.method).toBe("PUT");
      bodies.push(JSON.parse(options?.body as string));
      return Response.json(settings({ automatic_service_search: false }));
    }),
  );
  render(createElement(AutomaticLookupsSettings, { account: "a" }));
  fireEvent.click(await toggle(LABELS[2]));
  await waitFor(async () =>
    expect((await toggle(LABELS[2])).getAttribute("aria-checked")).toBe(
      "false",
    ),
  );
  expect(bodies).toEqual([{ automatic_service_search: false }]);
  expect(localStorage.getItem("hito:search-services-auto:a")).toBeNull();
});

test("a failed save keeps the old choice and says so", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string) =>
      path === SETTINGS
        ? Response.json(settings())
        : Response.json({ error: "busy" }, { status: 500 }),
    ),
  );
  render(createElement(AutomaticLookupsSettings, { account: "a" }));
  fireEvent.click(await toggle(LABELS[0]));
  await screen.findByRole("alert");
  expect((await toggle(LABELS[0])).getAttribute("aria-checked")).toBe("true");
});
