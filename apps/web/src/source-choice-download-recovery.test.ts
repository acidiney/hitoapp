import { router } from "@inertiajs/react";
import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import RecordingDetailsPage from "./pages/RecordingDetailsPage";
import { NoticeProvider } from "./shared/ui/Notices";
import { PlaybackProvider } from "./player/PlaybackProvider";
import { attachedPage, savedRecording } from "./fixtures/sourceAttachment";
import {
  choiceSourceA,
  choiceSourceB,
  currentManualChoice,
} from "./fixtures/sourceChoice";
import {
  automaticAfterReset,
  manualSourceA,
  recoveredDownloadB,
} from "./fixtures/sourceChoiceDownloadRecovery";
import { heldResponse } from "./test/async";

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

test("source changes preserve a pending download and its exact uncertain retry", async () => {
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const logs = (["debug", "info", "warn", "error", "log"] as const).map(
    (level) => vi.spyOn(console, level).mockImplementation(() => {}),
  );
  const choicePath = `/api/v1/recordings/${savedRecording.id}/source-choice`;
  const resetPath = `${choicePath}/reset`;
  const downloadPath = `/api/v1/recordings/${savedRecording.id}/download`;
  const sourcesPath = `/api/v1/recordings/${savedRecording.id}/sources`;
  const privateCanary = "overlapped-download-private-canary";
  const privateFailure = `lost response https://owned.invalid/audio?secret=${privateCanary}`;
  const draft = "Unsent overlapping source title";
  const uuid =
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
  const originalDownload = heldResponse();
  const retriedDownload = heldResponse();
  const resetChoice = heldResponse();
  const chooseSourceA = heldResponse();
  const automaticRead = heldResponse();
  const manualRead = heldResponse();
  const recoveredRead = heldResponse();
  const cleanupRead = heldResponse();
  let retainedReadyB = false;
  let downloadWrites = 0;
  let downloadReads = 0;
  let choiceReads = 0;
  let resetWrites = 0;
  let choiceWrites = 0;
  const unexpected: { url: string; method: string }[] = [];
  const fetch = vi.fn(async (url: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (url === choicePath && method === "GET") {
      choiceReads++;
      if (choiceReads === 1) return Response.json(currentManualChoice);
      if (choiceReads === 2) return automaticRead.response;
      if (choiceReads === 3) return manualRead.response;
    }
    if (url === resetPath && method === "POST" && resetWrites++ === 0)
      return resetChoice.response;
    if (url === choicePath && method === "POST" && choiceWrites++ === 0)
      return chooseSourceA.response;
    if (url === downloadPath && method === "GET") {
      downloadReads++;
      if (downloadReads === 1 && !retainedReadyB)
        return Response.json({ error: "Not found" }, { status: 404 });
      if (downloadReads === 2 && retainedReadyB) return recoveredRead.response;
      if (downloadReads === 3 && retainedReadyB) return cleanupRead.response;
    }
    if (url === downloadPath && method === "POST") {
      downloadWrites++;
      if (downloadWrites === 1) {
        retainedReadyB = true;
        return originalDownload.response;
      }
      if (downloadWrites === 2 && retainedReadyB)
        return retriedDownload.response;
    }
    if (url === sourcesPath && method === "GET") return Response.json([]);
    unexpected.push({ url, method });
    return Response.json(
      { error: "Unexpected overlapping download request" },
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
  const posted = (path: string) =>
    fetch.mock.calls.filter(
      ([url, options]) => url === path && options?.method === "POST",
    );
  await choice().findByText("Manual source selection");
  await waitFor(() =>
    expect(download().getByRole("button", { name: "Download" })).toHaveProperty(
      "disabled",
      false,
    ),
  );
  const start = download().getByRole("button", { name: "Download" });
  expect(downloadReads).toBe(1);
  expect(
    choice().getByRole("radio", { name: choiceSourceB.label }),
  ).toHaveProperty("checked", true);
  expect(
    fetch.mock.calls.every(([, options]) => options?.method === "GET"),
  ).toBe(true);
  expect(unexpected).toEqual([]);
  expect(play).not.toHaveBeenCalled();
  expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();
  fireEvent.change(title(), { target: { value: draft } });
  act(() => {
    fireEvent.click(start);
    fireEvent.click(start);
  });
  await waitFor(() => expect(posted(downloadPath)).toHaveLength(1));
  const original = posted(downloadPath)[0][1];
  expectOwnedRequest(original);
  const downloadBody = JSON.parse(String(original?.body));
  expect(downloadBody).toEqual({ request_id: expect.stringMatching(uuid) });
  const reset = choice().getByRole("button", { name: "Return to automatic" });
  act(() => {
    fireEvent.click(reset);
    fireEvent.click(reset);
  });
  await waitFor(() => expect(posted(resetPath)).toHaveLength(1));
  const resetRequest = posted(resetPath)[0][1];
  expectOwnedRequest(resetRequest);
  const resetBody = JSON.parse(String(resetRequest?.body));
  expect(resetBody).toEqual({
    request_id: expect.stringMatching(uuid),
    expected_revision: 3,
  });
  expect(resetBody.request_id).not.toBe(downloadBody.request_id);
  expect(original?.signal?.aborted).toBe(false);
  await act(async () =>
    resetChoice.resolve(
      Response.json({
        request_id: resetBody.request_id,
        recording_id: savedRecording.id,
        revision: 4,
      }),
    ),
  );
  await waitFor(() => expect(choiceReads).toBe(2));
  await act(async () =>
    automaticRead.resolve(Response.json(automaticAfterReset)),
  );
  await choice().findByText("Automatic source selection");
  expect(original?.signal?.aborted).toBe(false);
  expect(download().getByText("Checking saved copy…")).toBeTruthy();
  expect(posted(downloadPath)).toHaveLength(1);
  expect(downloadReads).toBe(1);
  expect(title().value).toBe(draft);
  await act(async () => originalDownload.reject(new TypeError(privateFailure)));
  const uncertain = await download().findByRole("button", {
    name: "Retry download request",
  });
  expect(uncertain).toHaveProperty("disabled", false);
  expect(download().getByRole("alert").textContent).toBe(
    "Hito could not confirm the download request. Retry the same request to recover its result.",
  );
  expect(document.body.textContent).not.toContain(privateCanary);
  fireEvent.click(choice().getByRole("radio", { name: choiceSourceA.label }));
  const use = choice().getByRole("button", { name: "Use source" });
  act(() => {
    fireEvent.click(use);
    fireEvent.click(use);
  });
  await waitFor(() => expect(posted(choicePath)).toHaveLength(1));
  const chosen = posted(choicePath)[0][1];
  expectOwnedRequest(chosen);
  const choiceBody = JSON.parse(String(chosen?.body));
  expect(choiceBody).toEqual({
    request_id: expect.stringMatching(uuid),
    source_id: choiceSourceA.id,
  });
  expect(choiceBody.request_id).not.toBe(downloadBody.request_id);
  expect(choiceBody.request_id).not.toBe(resetBody.request_id);
  await act(async () =>
    chooseSourceA.resolve(
      Response.json({
        request_id: choiceBody.request_id,
        recording_id: savedRecording.id,
        source_id: choiceSourceA.id,
        revision: 5,
      }),
    ),
  );
  await waitFor(() => expect(choiceReads).toBe(3));
  await act(async () => manualRead.resolve(Response.json(manualSourceA)));
  await choice().findByText("Manual source selection");
  expect(
    choice().getByRole("radio", { name: choiceSourceA.label }),
  ).toHaveProperty("checked", true);
  const retry = download().getByRole("button", {
    name: "Retry download request",
  });
  expect(retry).toHaveProperty("disabled", false);
  expect(download().getByText("Checking saved copy…")).toBeTruthy();
  expect(posted(downloadPath)).toHaveLength(1);
  expect(downloadReads).toBe(1);
  expect(title().value).toBe(draft);
  const beforeFocus = fetch.mock.calls.length;
  await act(async () => {
    window.dispatchEvent(new Event("focus"));
  });
  expect(fetch).toHaveBeenCalledTimes(beforeFocus);
  act(() => {
    fireEvent.click(retry);
    fireEvent.click(retry);
  });
  await waitFor(() => expect(posted(downloadPath)).toHaveLength(2));
  const repeated = posted(downloadPath)[1][1];
  expect(repeated?.body).toBe(original?.body);
  expectOwnedRequest(repeated);
  expect(repeated?.signal).not.toBe(original?.signal);
  expect(downloadReads).toBe(1);
  expect(download().queryByText("Saved on this server")).toBeNull();
  await act(async () =>
    retriedDownload.resolve(
      Response.json(
        {
          request_id: downloadBody.request_id,
          recording_id: savedRecording.id,
          intent_id: recoveredDownloadB.intent_id,
        },
        { status: 202 },
      ),
    ),
  );
  await waitFor(() => expect(downloadReads).toBe(2));
  expect(download().queryByText("Saved on this server")).toBeNull();
  expect(download().queryByText(/previous source/i)).toBeNull();
  expect(download().getByRole("button", { name: "Download" })).toHaveProperty(
    "disabled",
    true,
  );
  await act(async () =>
    recoveredRead.resolve(Response.json(recoveredDownloadB)),
  );
  await download().findByText(/previous source/i);
  expect(download().queryByText("Saved on this server")).toBeNull();
  expect(
    download().queryByRole("button", { name: "Retry download request" }),
  ).toBeNull();
  expect(
    download().getByRole("button", { name: "Download selected source" }),
  ).toHaveProperty("disabled", false);
  expect(
    download().getByRole("button", { name: "Remove download" }),
  ).toHaveProperty("disabled", false);
  expect(title().value).toBe(draft);
  expect(
    fetch.mock.calls
      .filter(([, options]) => options?.method !== "GET")
      .map(([url, options]) => [url, options?.method]),
  ).toEqual([
    [downloadPath, "POST"],
    [resetPath, "POST"],
    [choicePath, "POST"],
    [downloadPath, "POST"],
  ]);
  expect(unexpected).toEqual([]);
  expect(play).not.toHaveBeenCalled();
  expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();
  expect(router.reload).not.toHaveBeenCalled();
  expect(router.visit).not.toHaveBeenCalled();
  expect(document.body.textContent).not.toContain(privateCanary);
  await act(async () => {
    window.dispatchEvent(new Event("focus"));
  });
  await waitFor(() => expect(downloadReads).toBe(3));
  const heldRead = fetch.mock.calls
    .filter(
      ([url, options]) => url === downloadPath && options?.method === "GET",
    )
    .at(-1)!;
  expectOwnedRequest(heldRead[1]);
  expect(heldRead[1]?.body).toBeUndefined();
  const beforeUnmount = fetch.mock.calls.length;
  view.unmount();
  expect(heldRead[1]?.signal?.aborted).toBe(true);
  expect(document.querySelectorAll("audio")).toHaveLength(0);
  await act(async () => {
    cleanupRead.resolve(Response.json(recoveredDownloadB));
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
});
