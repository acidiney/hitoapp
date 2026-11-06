import type { Schema } from "../../../shared/schema";
import { router } from "@inertiajs/react";
import { createElement, type ReactNode } from "react";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { openTrackEditors } from "../../recordings/fixtures/trackEditors";
import { setAutomaticLyrics } from "../../lyrics/fixtures/automaticLyrics";
import RecordingDetailsPage from "../../recordings/pages/RecordingDetailsPage";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { PlaybackProvider } from "../../playback/components/PlaybackProvider";
import {
  attachedRecording,
  attachmentCandidate,
  attachmentReview,
  attachmentSearch,
  attachmentSettings,
  reviewedAttachment,
  savedRecording,
  sourceAttachmentPage,
} from "../fixtures/sourceAttachment";
import {
  automaticChoice,
  choiceSourceA,
  choiceSourceB,
} from "../fixtures/sourceChoice";

import { heldResponse } from "../../../test/async";

setAutomaticLyrics(false);

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

function expectOwnedRequest(options: RequestInit | undefined) {
  const headers = new Headers(options?.headers);
  expect(headers.get("content-type")).toBe("application/json");
  expect(headers.get("x-hito-account")).toBe(sourceAttachmentPage.user.id);
  expect(headers.get("x-hito-request")).toBe("1");
  expect(options?.credentials).toBe("same-origin");
  expect(options?.signal).toBeInstanceOf(AbortSignal);
  expect(options?.signal?.aborted).toBe(false);
}

test("attaching a reviewed source refreshes sibling choices and fences an older read", async () => {
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const logs = (["debug", "info", "warn", "error", "log"] as const).map(
    (level) => vi.spyOn(console, level).mockImplementation(() => {}),
  );
  const base = `/api/v1/recordings/${savedRecording.id}`;
  const choicePath = `${base}/source-choice`;
  const sourcesPath = `${base}/sources`;
  const searchPath = "/api/v1/plugins/video-connection/search";
  const reviewPath = `${sourcesPath}/review`;
  const attachPath = `${sourcesPath}/attach`;
  const query = "Owned track audio";
  const draft = "Unsent attached-source title";
  const originalChoice: Schema<"SourceChoice"> = {
    ...automaticChoice,
    sources: automaticChoice.sources.filter(
      (entry) => entry.source.id === choiceSourceA.id,
    ),
  };
  const olderChoice = heldResponse();
  const attachment = heldResponse();
  const freshChoice = heldResponse();
  let choiceReads = 0;
  let attachmentWrites = 0;
  let accepted = false;
  const unexpected: { url: string; method: string }[] = [];
  const fetch = vi.fn(async (url: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (url === choicePath && method === "GET") {
      choiceReads++;
      if (choiceReads === 1 && !accepted) return olderChoice.response;
      if (choiceReads === 2 && accepted) return freshChoice.response;
    }
    if (url === `${base}/download` && method === "GET")
      return Response.json({ error: "Not found" }, { status: 404 });
    if (url === "/api/v1/plugins" && method === "GET")
      return Response.json(attachmentSettings);
    if (url === searchPath && method === "POST")
      return Response.json(attachmentSearch);
    if (url === reviewPath && method === "POST")
      return Response.json(attachmentReview);
    if (url === attachPath && method === "POST" && ++attachmentWrites === 1)
      return attachment.response;
    if (url === sourcesPath && method === "GET") return Response.json([]);
    unexpected.push({ url, method });
    return Response.json(
      { error: "Unexpected source attachment request" },
      { status: 500 },
    );
  });
  vi.stubGlobal("fetch", fetch);
  const view = render(
    createElement(
      NoticeProvider,
      null,
      createElement(PlaybackProvider, {
        account: sourceAttachmentPage.user.id,
        children: createElement(RecordingDetailsPage, sourceAttachmentPage),
      }),
    ),
  );
  const requests = (path: string, method: string) =>
    fetch.mock.calls.filter(
      ([url, options]) => url === path && options?.method === method,
    );
  const choice = () =>
    within(screen.getByRole("region", { name: "Playback source" }));
  const add = () => within(screen.getByRole("region", { name: "Find audio" }));
  openTrackEditors();
  const title = () =>
    screen.getByRole("textbox", { name: "Track title" }) as HTMLInputElement;
  await waitFor(() => expect(choiceReads).toBe(1));
  const oldRead = requests(choicePath, "GET")[0][1];
  expectOwnedRequest(oldRead);
  expect(oldRead?.body).toBeUndefined();
  expect(
    fetch.mock.calls.every(([, options]) => options?.method === "GET"),
  ).toBe(true);
  const draftInput = title();
  fireEvent.change(draftInput, { target: { value: draft } });
  fireEvent.click(add().getByRole("button", { name: "Find a source" }));
  const service = await add().findByRole("combobox", {
    name: "Music service",
  });
  expect(service).toHaveProperty("value", "video-connection");
  expectOwnedRequest(requests("/api/v1/plugins", "GET")[0][1]);
  fireEvent.change(add().getByRole("searchbox", { name: "Search music" }), {
    target: { value: query },
  });
  fireEvent.click(add().getByRole("button", { name: "Search" }));
  const review = await add().findByRole("button", { name: "Review match" });
  const searchRequest = requests(searchPath, "POST")[0][1];
  expectOwnedRequest(searchRequest);
  expect(JSON.parse(String(searchRequest?.body))).toEqual({
    installation_revision: 7,
    query,
    cursor: null,
    limit: 20,
  });
  fireEvent.click(review);
  const comparison = within(
    await screen.findByRole("dialog", { name: "Review source match" }),
  );
  expect(comparison.getByText(savedRecording.title)).toBeTruthy();
  expect(comparison.getByText(attachmentReview.candidate.title)).toBeTruthy();
  const reviewRequest = requests(reviewPath, "POST")[0][1];
  expectOwnedRequest(reviewRequest);
  expect(JSON.parse(String(reviewRequest?.body))).toEqual(attachmentCandidate);
  const use = comparison.getByRole("button", { name: "Use this source" });
  act(() => {
    fireEvent.click(use);
    fireEvent.click(use);
  });
  await waitFor(() => expect(attachmentWrites).toBe(1));
  const attachRequest = requests(attachPath, "POST")[0][1];
  expectOwnedRequest(attachRequest);
  expect(JSON.parse(String(attachRequest?.body))).toEqual(reviewedAttachment);
  expect(choiceReads).toBe(1);
  expect(oldRead?.signal?.aborted).toBe(false);
  expect(title()).toBe(draftInput);
  expect(title().value).toBe(draft);
  expect(unexpected).toEqual([]);
  accepted = true;
  await act(async () => attachment.resolve(Response.json(attachedRecording)));
  await add().findByText("Source added to this track.");
  await waitFor(() => expect(choiceReads).toBe(2));
  const freshRead = requests(choicePath, "GET")[1][1];
  expectOwnedRequest(freshRead);
  expect(freshRead?.body).toBeUndefined();
  expect(freshRead?.signal).not.toBe(oldRead?.signal);
  expect(oldRead?.signal?.aborted).toBe(true);
  await act(async () => freshChoice.resolve(Response.json(automaticChoice)));
  await choice().findByText("Automatic source selection");
  const sourceB = choice().getByRole("radio", { name: choiceSourceB.label });
  expect(sourceB).toHaveProperty("disabled", false);
  expect(sourceB).toHaveProperty("checked", false);
  expect(
    choice().getByRole("radio", { name: choiceSourceA.label }),
  ).toHaveProperty("checked", false);
  fireEvent.click(sourceB);
  expect(sourceB).toHaveProperty("checked", true);
  expect(choice().getByRole("button", { name: "Use source" })).toHaveProperty(
    "disabled",
    false,
  );
  await act(async () => olderChoice.resolve(Response.json(originalChoice)));
  expect(choice().getByRole("radio", { name: choiceSourceB.label })).toBe(
    sourceB,
  );
  expect(sourceB).toHaveProperty("checked", true);
  expect(sourceB).toHaveProperty("disabled", false);
  expect(title()).toBe(draftInput);
  expect(title().value).toBe(draft);
  expect(choiceReads).toBe(2);
  expect(attachmentWrites).toBe(1);
  expect(requests("/api/v1/plugins", "GET")).toHaveLength(1);
  expect(
    fetch.mock.calls
      .filter(([, options]) => options?.method !== "GET")
      .map(([url, options]) => [url, options?.method]),
  ).toEqual([
    [searchPath, "POST"],
    [reviewPath, "POST"],
    [attachPath, "POST"],
  ]);
  expect(unexpected).toEqual([]);
  expect(play).not.toHaveBeenCalled();
  expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();
  expect(router.reload).not.toHaveBeenCalled();
  expect(router.visit).not.toHaveBeenCalled();
  expect(document.body.textContent).not.toContain(
    attachmentReview.review_stamp,
  );
  for (const [, options] of fetch.mock.calls) {
    const headers = new Headers(options?.headers);
    expect(headers.get("content-type")).toBe("application/json");
    expect(headers.get("x-hito-account")).toBe(sourceAttachmentPage.user.id);
    expect(headers.get("x-hito-request")).toBe("1");
    expect(options?.credentials).toBe("same-origin");
    expect(options?.signal).toBeInstanceOf(AbortSignal);
    if (options?.method === "GET") expect(options.body).toBeUndefined();
  }
  const beforeUnmount = fetch.mock.calls.length;
  view.unmount();
  expect(document.querySelectorAll("audio")).toHaveLength(0);
  await act(async () => {
    window.dispatchEvent(new Event("focus"));
  });
  expect(fetch).toHaveBeenCalledTimes(beforeUnmount);
  expect(unexpected).toEqual([]);
  const logged = JSON.stringify(
    logs.flatMap((entry) => entry.mock.calls),
    (_key, value) =>
      value instanceof Error
        ? { name: value.name, message: value.message, stack: value.stack }
        : value,
  );
  expect(logged).not.toContain(draft);
  expect(logged).not.toContain(query);
  expect(logged).not.toContain(attachmentReview.review_stamp);
  expect(logged).not.toContain(attachmentReview.candidate.title);
});
