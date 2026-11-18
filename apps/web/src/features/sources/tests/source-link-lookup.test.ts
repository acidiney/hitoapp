import { createElement, type ReactNode } from "react";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { expect, test, vi } from "vitest";
import type { Schema } from "../../../shared/schema";
import { setAutomaticLyrics } from "../../lyrics/fixtures/automaticLyrics";
import RecordingDetailsPage from "../../recordings/pages/RecordingDetailsPage";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { PlaybackProvider } from "../../playback/components/PlaybackProvider";
import {
  attachedRecording,
  attachedSources,
  attachmentReview,
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

const linkSettings: Schema<"PluginSettings"> = {
  ...attachmentSettings,
  services: [
    ...attachmentSettings.services,
    {
      id: "link-service",
      name: "Link archive",
      base_url: "http://owned-links.invalid/",
      revision: 1,
    },
  ],
  installations: [
    ...attachmentSettings.installations,
    {
      id: "link-connection",
      service_id: "link-service",
      revision: 3,
      checked_at: 1,
      credential_set: false,
      manifest: {
        id: "org.hito.link-fixture",
        name: "Owned links",
        protocol_version: 1,
        capabilities: ["sources", "source_links"],
      },
    },
  ],
};

const linkCandidate: Schema<"ProviderCandidate"> = {
  installation_id: "link-connection",
  installation_revision: 3,
  provider_recording_id: "link/candidate",
};

function renderPage(settings: Schema<"PluginSettings">, found = true) {
  const base = `/api/v1/recordings/${savedRecording.id}/sources`;
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (path === "/api/v1/plugins") return Response.json(settings);
    if (path === `${base}/lookup` && method === "POST")
      return found
        ? Response.json(linkCandidate)
        : Response.json({ error: "Resource not found" }, { status: 404 });
    if (path === `${base}/review` && method === "POST")
      return Response.json(attachmentReview);
    if (path === `${base}/attach` && method === "POST")
      return Response.json(attachedRecording);
    if (path === `${base}/refresh`) return Response.json(attachedSources);
    if (method === "GET")
      return Response.json({ error: "Not found" }, { status: 404 });
    throw new Error("Unexpected request");
  });
  vi.stubGlobal("fetch", fetch);
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
  return fetch;
}

function body(fetch: ReturnType<typeof renderPage>, suffix: string) {
  const call = fetch.mock.calls.find(([path]) => path.endsWith(suffix));
  return call ? JSON.parse(String(call[1]?.body)) : undefined;
}

test("a pasted link is looked up through a link service and reviewed before it becomes a source", async () => {
  const fetch = renderPage(linkSettings);
  fireEvent.click(screen.getByRole("button", { name: "Find a source" }));
  const link = await screen.findByRole("textbox", { name: "Paste a link" });
  fireEvent.change(link, {
    target: { value: "  https://owned-links.invalid/track/1  " },
  });
  fireEvent.click(screen.getByRole("button", { name: "Look up link" }));
  const review = await screen.findByRole("dialog", {
    name: "Review source match",
  });
  expect(body(fetch, "/sources/lookup")).toEqual({
    installation_id: "link-connection",
    installation_revision: 3,
    reference: "https://owned-links.invalid/track/1",
  });
  expect(body(fetch, "/sources/review")).toEqual(linkCandidate);
  fireEvent.click(
    within(review).getByRole("button", { name: "Use this source" }),
  );
  await screen.findByText("Source added to this track.");
  expect(body(fetch, "/sources/attach")).toEqual({
    candidate: linkCandidate,
    review_stamp: attachmentReview.review_stamp,
  });
});

test("search and the paste field are offered together, so a link is there when search finds nothing", async () => {
  renderPage(linkSettings);
  fireEvent.click(screen.getByRole("button", { name: "Find a source" }));
  await screen.findByRole("textbox", { name: "Paste a link" });
  expect(screen.getByRole("combobox", { name: "Music service" })).toBeTruthy();
  expect(screen.getByRole("searchbox")).toBeTruthy();
  expect(
    screen.getByText(
      "Not in the search results? Paste a link from Link archive.",
    ),
  ).toBeTruthy();
});

test("without a link service the paste field is not offered", async () => {
  renderPage(attachmentSettings);
  fireEvent.click(screen.getByRole("button", { name: "Find a source" }));
  await screen.findByRole("combobox", { name: "Music service" });
  expect(screen.queryByRole("textbox", { name: "Paste a link" })).toBeNull();
});

test("a link service alone offers the paste field without a search service", async () => {
  renderPage({
    ...linkSettings,
    installations: linkSettings.installations.filter(
      (item) => item.id === "link-connection",
    ),
  });
  fireEvent.click(screen.getByRole("button", { name: "Find a source" }));
  await screen.findByRole("textbox", { name: "Paste a link" });
  expect(screen.getByText("Paste a link from Link archive.")).toBeTruthy();
  await waitFor(() =>
    expect(screen.queryByText(/No connected service offers both/)).toBeNull(),
  );
});

test("a link the service cannot find says so in plain words and keeps the link for another try", async () => {
  renderPage(linkSettings, false);
  fireEvent.click(screen.getByRole("button", { name: "Find a source" }));
  const link = await screen.findByRole("textbox", { name: "Paste a link" });
  fireEvent.change(link, {
    target: { value: "https://owned-links.invalid/x" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Look up link" }));
  const alert = await screen.findByRole("alert");
  expect(alert.textContent).toContain(
    "The service couldn't find a track at that link. Check the link and try again.",
  );
  expect((link as HTMLInputElement).value).toBe(
    "https://owned-links.invalid/x",
  );
  expect(screen.queryByRole("dialog")).toBeNull();
});
