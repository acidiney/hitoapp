import { createElement, type ReactNode } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import type { Schema } from "../../../shared/schema";
import { setAutomaticLyrics } from "../../lyrics/fixtures/automaticLyrics";
import RecordingDetailsPage from "../../recordings/pages/RecordingDetailsPage";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { PlaybackProvider } from "../../playback/components/PlaybackProvider";
import {
  attachmentSearch,
  attachmentSettings,
  savedRecording,
  sourceAttachmentPage,
} from "../fixtures/sourceAttachment";

setAutomaticLyrics(false);

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

async function reviewWith(conflict: Record<string, unknown>) {
  const base = `/api/v1/recordings/${savedRecording.id}/sources`;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, options?: RequestInit) => {
      if (path === `${base}/review`)
        return Response.json(conflict, { status: 409 });
      if (path === "/api/v1/plugins") return Response.json(attachmentSettings);
      if (path === "/api/v1/plugins/video-connection/search")
        return Response.json(attachmentSearch);
      if ((options?.method ?? "GET") === "GET")
        return Response.json({ error: "Not found" }, { status: 404 });
      throw new Error("Unexpected request");
    }),
  );
  const props: Schema<"RecordingDetailsPageProps"> = sourceAttachmentPage;
  render(
    createElement(
      NoticeProvider,
      null,
      createElement(PlaybackProvider, {
        account: props.user.id,
        children: createElement(RecordingDetailsPage, props),
      }),
    ),
  );
  fireEvent.click(screen.getByRole("button", { name: "Find a source" }));
  await screen.findByRole("combobox", { name: "Music service" });
  fireEvent.change(screen.getByRole("searchbox", { name: "Search music" }), {
    target: { value: "Owned track audio" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Search" }));
  fireEvent.click(await screen.findByRole("button", { name: "Review match" }));
  return screen.findByText(/already the source of another track/);
}

test("a source that already belongs to another track says so in plain words and links to that track", async () => {
  const message = await reviewWith({
    error: "This source is already used by another track",
    code: "already_bound",
    recording_id: "other track/1",
  });
  expect(message.textContent).toContain(
    "This video is already the source of another track.",
  );
  expect(message.textContent).not.toContain("provider recording");
  expect(
    screen.getByRole("link", { name: "Open that track" }).getAttribute("href"),
  ).toBe("/recordings/other%20track%2F1");
});

test("an already used source without a visible track offers no link", async () => {
  await reviewWith({
    error: "This source is already used by another track",
    code: "already_bound",
  });
  expect(screen.queryByRole("link", { name: "Open that track" })).toBeNull();
});
