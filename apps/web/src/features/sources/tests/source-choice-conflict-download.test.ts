import { router } from "@inertiajs/react";
import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import RecordingDetailsPage from "../../recordings/pages/RecordingDetailsPage";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { PlaybackProvider } from "../../playback/components/PlaybackProvider";
import { attachedPage, savedRecording } from "../fixtures/sourceAttachment";
import { choiceSourceA, choiceSourceB } from "../fixtures/sourceChoice";
import {
  currentChoiceB2,
  initialChoiceA1,
  previousA2,
  savedA1,
} from "../fixtures/sourceChoiceConflictDownload";
import { heldResponse } from "../../../test/async";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

function expectOwnedRequest(options: RequestInit | undefined) {
  const headers = new Headers(options?.headers);
  expect(headers.get("content-type")).toBe("application/json");
  expect(headers.get("x-hito-account")).toBe(attachedPage.user.id);
  expect(headers.get("x-hito-request")).toBe("1");
  expect(options?.credentials).toBe("same-origin");
  expect(options?.signal).toBeInstanceOf(AbortSignal);
  expect(options?.signal?.aborted).toBe(false);
}

test("a conflicting reset reconciles download eligibility without trusting stale revisions", async () => {
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const logs = (["debug", "info", "warn", "error", "log"] as const).map(
    (level) => vi.spyOn(console, level).mockImplementation(() => {}),
  );
  const base = `/api/v1/recordings/${savedRecording.id}`;
  const choicePath = `${base}/source-choice`;
  const resetPath = `${choicePath}/reset`;
  const downloadPath = `${base}/download`;
  const sourcesPath = `${base}/sources`;
  const privateCanary = "conflicting-choice-private-canary";
  const privateFailure = `private conflict https://owned.invalid/audio?secret=${privateCanary}`;
  const draft = "Unsent conflicting source title";
  const uuid =
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
  const olderSnapshot = heldResponse();
  const rejectedReset = heldResponse();
  const reconciledChoice = heldResponse();
  const inconsistentFreshStatus = heldResponse();
  const currentStatus = heldResponse();
  const cleanupStatus = heldResponse();
  let otherClientAcceptedB2 = false;
  let choiceReads = 0;
  let downloadReads = 0;
  let resetWrites = 0;
  const unexpected: { url: string; method: string }[] = [];
  const fetch = vi.fn(async (url: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (url === choicePath && method === "GET") {
      choiceReads++;
      if (choiceReads === 1) return Response.json(initialChoiceA1);
      if (choiceReads === 2 && otherClientAcceptedB2)
        return reconciledChoice.response;
    }
    if (
      url === resetPath &&
      method === "POST" &&
      ++resetWrites === 1 &&
      otherClientAcceptedB2
    )
      return rejectedReset.response;
    if (url === downloadPath && method === "GET") {
      downloadReads++;
      if (downloadReads === 1 && !otherClientAcceptedB2)
        return Response.json(savedA1);
      if (downloadReads === 2 && !otherClientAcceptedB2)
        return olderSnapshot.response;
      if (downloadReads === 3 && otherClientAcceptedB2)
        return inconsistentFreshStatus.response;
      if (downloadReads === 4 && otherClientAcceptedB2)
        return currentStatus.response;
      if (downloadReads === 5 && otherClientAcceptedB2)
        return cleanupStatus.response;
    }
    if (url === sourcesPath && method === "GET") return Response.json([]);
    unexpected.push({ url, method });
    return Response.json(
      { error: "Unexpected source choice conflict request" },
      { status: 500 },
    );
  });
  vi.stubGlobal("fetch", fetch);
  const view = render(
    createElement(
      NoticeProvider,
      null,
      createElement(PlaybackProvider, {
        account: attachedPage.user.id,
        children: createElement(RecordingDetailsPage, attachedPage),
      }),
    ),
  );
  const download = () =>
    within(screen.getByRole("region", { name: "Download" }));
  const choice = () =>
    within(screen.getByRole("region", { name: "Playback source" }));
  const title = () =>
    screen.getByRole("textbox", { name: "Track title" }) as HTMLInputElement;
  const requests = (path: string, method: string) =>
    fetch.mock.calls.filter(
      ([url, options]) => url === path && options?.method === method,
    );
  function expectDownloadUnverified() {
    expect(download().queryByText("Saved on this server")).toBeNull();
    const before = fetch.mock.calls.length;
    for (const name of [
      "Download",
      "Download selected source",
      "Remove download",
    ]) {
      for (const button of download().queryAllByRole("button", { name })) {
        expect(button).toHaveProperty("disabled", true);
        fireEvent.click(button);
      }
    }
    expect(fetch).toHaveBeenCalledTimes(before);
    expect(play).not.toHaveBeenCalled();
    expect(title().value).toBe(draft);
    expect(document.body.textContent).not.toContain(privateCanary);
    expect(unexpected).toEqual([]);
  }
  await choice().findByText("Manual source selection");
  await download().findByText("Saved on this server");
  expect(
    choice().getByRole("radio", { name: choiceSourceA.label }),
  ).toHaveProperty("checked", true);
  expect(
    download().getByRole("button", { name: "Remove download" }),
  ).toHaveProperty("disabled", false);
  expect(downloadReads).toBe(1);
  expect(choiceReads).toBe(1);
  expect(
    fetch.mock.calls.every(([, options]) => options?.method === "GET"),
  ).toBe(true);
  for (const [, options] of fetch.mock.calls) {
    expectOwnedRequest(options);
    expect(options?.body).toBeUndefined();
  }
  fireEvent.change(title(), { target: { value: draft } });
  await act(async () => {
    window.dispatchEvent(new Event("focus"));
  });
  await waitFor(() => expect(downloadReads).toBe(2));
  const oldRead = requests(downloadPath, "GET")[1][1];
  expectOwnedRequest(oldRead);
  expect(oldRead?.body).toBeUndefined();
  expectDownloadUnverified();
  otherClientAcceptedB2 = true;
  const reset = choice().getByRole("button", { name: "Return to automatic" });
  act(() => {
    fireEvent.click(reset);
    fireEvent.click(reset);
  });
  await waitFor(() => expect(resetWrites).toBe(1));
  const resetRequest = requests(resetPath, "POST")[0][1];
  expectOwnedRequest(resetRequest);
  expect(JSON.parse(String(resetRequest?.body))).toEqual({
    request_id: expect.stringMatching(uuid),
    expected_revision: 1,
  });
  expectDownloadUnverified();
  await act(async () =>
    rejectedReset.resolve(
      Response.json({ error: privateFailure }, { status: 409 }),
    ),
  );
  await waitFor(() => expect(choiceReads).toBe(2));
  expectOwnedRequest(requests(choicePath, "GET")[1][1]);
  expect(choice().getByRole("alert").textContent).toBe(
    "Hito could not change the source choice. Check the current choice and try again.",
  );
  expect(
    choice().queryByRole("button", { name: "Retry source choice" }),
  ).toBeNull();
  expectDownloadUnverified();
  await act(async () =>
    reconciledChoice.resolve(Response.json(currentChoiceB2)),
  );
  await waitFor(() =>
    expect(
      choice().getByRole("radio", { name: choiceSourceB.label }),
    ).toHaveProperty("checked", true),
  );
  expect(unexpected).toEqual([]);
  await waitFor(() => expect(downloadReads).toBe(3));
  const freshRead = requests(downloadPath, "GET")[2][1];
  expectOwnedRequest(freshRead);
  expect(freshRead?.signal).not.toBe(oldRead?.signal);
  expect(oldRead?.signal?.aborted).toBe(true);
  expectDownloadUnverified();

  // This new post-B2 response deliberately violates the revision contract.
  await act(async () =>
    inconsistentFreshStatus.resolve(Response.json(savedA1)),
  );
  const check = await download().findByRole("button", {
    name: "Check download",
  });
  expect(download().getByRole("alert").textContent).toBe(
    "Hito could not check this download. Try again.",
  );
  expect(check).toHaveProperty("disabled", false);
  expectDownloadUnverified();
  fireEvent.click(check);
  await waitFor(() => expect(downloadReads).toBe(4));
  const currentRead = requests(downloadPath, "GET")[3][1];
  expectOwnedRequest(currentRead);
  expectDownloadUnverified();
  await act(async () => currentStatus.resolve(Response.json(previousA2)));
  await download().findByText(/previous source/i);
  expect(download().queryByText("Saved on this server")).toBeNull();
  expect(
    download().getByRole("button", { name: "Download selected source" }),
  ).toHaveProperty("disabled", false);
  expect(
    download().getByRole("button", { name: "Remove download" }),
  ).toHaveProperty("disabled", false);

  // This older response really was snapshotted before the other client's B2.
  await act(async () => olderSnapshot.resolve(Response.json(savedA1)));
  expect(download().getByText(/previous source/i)).toBeTruthy();
  expect(download().queryByText("Saved on this server")).toBeNull();
  expect(
    choice().getByRole("radio", { name: choiceSourceB.label }),
  ).toHaveProperty("checked", true);
  expect(title().value).toBe(draft);
  expect(resetWrites).toBe(1);
  expect(choiceReads).toBe(2);
  expect(
    fetch.mock.calls
      .filter(([, options]) => options?.method !== "GET")
      .map(([url, options]) => [url, options?.method]),
  ).toEqual([[resetPath, "POST"]]);
  expect(play).not.toHaveBeenCalled();
  expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();
  expect(router.reload).not.toHaveBeenCalled();
  expect(router.visit).not.toHaveBeenCalled();
  expect(unexpected).toEqual([]);
  for (const [, options] of fetch.mock.calls) {
    const headers = new Headers(options?.headers);
    expect(headers.get("content-type")).toBe("application/json");
    expect(headers.get("x-hito-account")).toBe(attachedPage.user.id);
    expect(headers.get("x-hito-request")).toBe("1");
    expect(options?.credentials).toBe("same-origin");
    expect(options?.signal).toBeInstanceOf(AbortSignal);
    if (options?.method === "GET") expect(options.body).toBeUndefined();
  }
  await act(async () => {
    window.dispatchEvent(new Event("focus"));
  });
  await waitFor(() => expect(downloadReads).toBe(5));
  const cleanupRead = requests(downloadPath, "GET")[4][1];
  expectOwnedRequest(cleanupRead);
  const beforeUnmount = fetch.mock.calls.length;
  view.unmount();
  expect(cleanupRead?.signal?.aborted).toBe(true);
  expect(document.querySelectorAll("audio")).toHaveLength(0);
  await act(async () => {
    cleanupStatus.resolve(Response.json(previousA2));
    window.dispatchEvent(new Event("focus"));
  });
  expect(fetch).toHaveBeenCalledTimes(beforeUnmount);
  expect(unexpected).toEqual([]);
  expect(play).not.toHaveBeenCalled();
  const logged = JSON.stringify(
    logs.flatMap((entry) => entry.mock.calls),
    (_key, value) =>
      value instanceof Error
        ? { name: value.name, message: value.message, stack: value.stack }
        : value,
  );
  expect(logged).not.toContain(privateCanary);
  expect(logged).not.toContain(draft);
  expect(logged).not.toContain("expected_revision");
  expect(logged).not.toContain("matches_source_choice");
});
