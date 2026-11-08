import { router } from "@inertiajs/react";
import { createElement, type ReactNode } from "react";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { setAutomaticLyrics } from "../../lyrics/fixtures/automaticLyrics";
import type { Schema } from "../../../shared/schema";
import { ArtworkProvider } from "../../artwork/components/ArtworkProvider";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { attributedRecordingPage } from "../fixtures/recordingAttribution";
import { automaticChoice } from "../../sources/fixtures/sourceChoice";
import RecordingDetailsPage from "../pages/RecordingDetailsPage";
import { PlaybackProvider } from "../../playback/components/PlaybackProvider";

setAutomaticLyrics(false);

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

const { detail, user } = attributedRecordingPage;
const base = `/api/v1/recordings/${detail.recording_id}`;
const imported = "11111111-2222-4333-8444-555555555555";

const copies: Schema<"RecordingCopies"> = {
  canonical_id: detail.recording_id,
  copies: [
    {
      recording_id: detail.recording_id,
      title: detail.title.value,
      artist: detail.artist.value,
      album: null,
      proof: null,
      canonical: true,
      available: true,
      sources: [
        {
          id: "local-source",
          recording_id: detail.recording_id,
          provider: "local",
          label: "Local audio",
          device_id: null,
          ready: true,
          can_prepare: false,
          match_kind: "verified",
        },
      ],
    },
    {
      recording_id: imported,
      title: "Imported spelling",
      artist: detail.artist.value,
      album: "Imported album",
      proof: "isrc",
      canonical: false,
      available: false,
      sources: [
        {
          id: "service-source",
          recording_id: imported,
          provider: "plugin",
          label: "Audio",
          device_id: null,
          ready: false,
          can_prepare: true,
          match_kind: "verified",
        },
      ],
    },
  ],
};

function open(page: Schema<"RecordingDetailsPageProps">, split = 200) {
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (method === "GET" && path === `${base}/download`)
      return Response.json({ error: "Not found" }, { status: 404 });
    if (method === "GET" && path === `${base}/source-choice`)
      return Response.json(automaticChoice);
    if (
      method === "POST" &&
      path === `/api/v1/recordings/${imported}/copies/split`
    )
      return split === 200
        ? Response.json({ recording_id: imported, separate: true })
        : Response.json({ error: "Busy" }, { status: split });
    throw new Error("Unexpected request outside the owned recording reads");
  });
  vi.stubGlobal("fetch", fetch);
  render(
    createElement(
      NoticeProvider,
      null,
      createElement(ArtworkProvider, {
        account: user.id,
        children: createElement(PlaybackProvider, {
          account: user.id,
          children: createElement(RecordingDetailsPage, page),
        }),
      }),
    ),
  );
  return fetch;
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.mocked(router.reload).mockClear();
});

test("a recording alone shows no copies section", async () => {
  open(attributedRecordingPage);
  await screen.findByRole("heading", { name: detail.title.value, level: 1 });
  expect(
    screen.queryByRole("region", { name: "Copies of this song" }),
  ).toBeNull();
});

test("a grouped song lists every copy with its proof and sources, and only a copy that is not the card offers the split", async () => {
  open({ ...attributedRecordingPage, copies });
  const region = within(
    await screen.findByRole("region", { name: "Copies of this song" }),
  );
  expect(region.getByText("Local audio · ready")).toBeTruthy();
  expect(region.getByText("Audio")).toBeTruthy();
  expect(region.getByText(/Same ISRC/)).toBeTruthy();
  expect(
    region
      .getByRole("link", { name: "Imported spelling" })
      .getAttribute("href"),
  ).toBe(`/recordings/${imported}`);
  expect(
    region.getAllByRole("button", { name: "These are different songs" }),
  ).toHaveLength(1);
});

test("separating a copy sends one account-scoped request and reloads the page", async () => {
  const fetch = open({ ...attributedRecordingPage, copies });
  fireEvent.click(
    await screen.findByRole("button", { name: "These are different songs" }),
  );
  await waitFor(() => expect(router.reload).toHaveBeenCalledTimes(1));
  const writes = fetch.mock.calls.filter(
    ([, options]) => options?.method === "POST",
  );
  expect(writes).toHaveLength(1);
  expect(writes[0][0]).toBe(`/api/v1/recordings/${imported}/copies/split`);
  expect(
    (writes[0][1]?.headers as Record<string, string>)["x-hito-account"],
  ).toBe(user.id);
});

test("a failed split says so, keeps the copies and does not reload", async () => {
  vi.spyOn(console, "warn").mockImplementation(() => {});
  open({ ...attributedRecordingPage, copies }, 409);
  fireEvent.click(
    await screen.findByRole("button", { name: "These are different songs" }),
  );
  const alert = await screen.findByText(
    "Hito could not separate these songs. Try again.",
  );
  expect(alert.getAttribute("role")).toBe("alert");
  expect(
    screen.getByRole("button", { name: "These are different songs" }),
  ).toBeTruthy();
  expect(router.reload).not.toHaveBeenCalled();
});
