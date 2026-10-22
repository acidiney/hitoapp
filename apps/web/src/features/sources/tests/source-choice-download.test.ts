import { router } from "@inertiajs/react";
import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import RecordingDetailsPage from "../../recordings/pages/RecordingDetailsPage";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { PlaybackProvider } from "../../../player/PlaybackProvider";
import { attachedPage, savedRecording } from "../fixtures/sourceAttachment";
import {
  automaticChoice,
  choiceSourceA,
  choiceSourceB,
} from "../fixtures/sourceChoice";
import {
  previousDownloadA,
  readyDownloadA,
  readyDownloadB,
  unavailableManualChoice,
} from "../fixtures/sourceChoiceDownload";
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

test("a previous-source download is kept until an explicit replacement succeeds", async () => {
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const logs = (["debug", "info", "warn", "error", "log"] as const).map(
    (level) => vi.spyOn(console, level).mockImplementation(() => {}),
  );
  const choicePath = `/api/v1/recordings/${savedRecording.id}/source-choice`;
  const downloadPath = `/api/v1/recordings/${savedRecording.id}/download`;
  const sourcesPath = `/api/v1/recordings/${savedRecording.id}/sources`;
  const privateCanary = "selected-download-private-canary";
  const privateFailure = `unavailable source https://owned.invalid/audio?secret=${privateCanary}`;
  const draft = "Unsent replacement download title";
  const uuid =
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
  const choiceWrite = heldResponse();
  const currentChoice = heldResponse();
  const rejectedDownload = heldResponse();
  const acceptedDownload = heldResponse();
  const downloadReads = [
    heldResponse(),
    heldResponse(),
    heldResponse(),
    heldResponse(),
  ];
  let choiceReadCount = 0;
  let downloadReadCount = 0;
  let choiceWriteCount = 0;
  let downloadWriteCount = 0;
  let sourceBAvailable = false;
  const unexpected: { url: string; method: string }[] = [];
  const fetch = vi.fn(async (url: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (url === choicePath && method === "GET") {
      choiceReadCount++;
      if (choiceReadCount === 1) return Response.json(automaticChoice);
      if (choiceReadCount === 2) return currentChoice.response;
    }
    if (url === choicePath && method === "POST" && choiceWriteCount++ === 0)
      return choiceWrite.response;
    if (
      url === downloadPath &&
      method === "GET" &&
      downloadReadCount < downloadReads.length
    )
      return downloadReads[downloadReadCount++].response;
    if (url === downloadPath && method === "POST") {
      downloadWriteCount++;
      if (downloadWriteCount === 1 && !sourceBAvailable)
        return rejectedDownload.response;
      if (downloadWriteCount === 2 && sourceBAvailable)
        return acceptedDownload.response;
    }
    if (url === sourcesPath && method === "GET") return Response.json([]);
    unexpected.push({ url, method });
    return Response.json(
      { error: "Unexpected selected download request" },
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
  await waitFor(() => expect(downloadReadCount).toBe(1));
  await act(async () =>
    downloadReads[0].resolve(Response.json(readyDownloadA)),
  );
  await download().findByText("Saved on this server");
  await choice().findByText("Automatic source selection");
  expect(
    choice().getByRole("radio", { name: choiceSourceA.label }),
  ).toHaveProperty("checked", false);
  expect(
    choice().getByRole("radio", { name: choiceSourceB.label }),
  ).toHaveProperty("checked", false);
  expect(
    fetch.mock.calls.every(([, options]) => options?.method === "GET"),
  ).toBe(true);
  expect(unexpected).toEqual([]);
  expect(play).not.toHaveBeenCalled();
  expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();
  fireEvent.change(title(), { target: { value: draft } });
  fireEvent.click(choice().getByRole("radio", { name: choiceSourceB.label }));
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
    source_id: choiceSourceB.id,
  });
  await act(async () =>
    choiceWrite.resolve(
      Response.json({
        request_id: choiceBody.request_id,
        recording_id: savedRecording.id,
        source_id: choiceSourceB.id,
        revision: 1,
      }),
    ),
  );
  await waitFor(() => expect(choiceReadCount).toBe(2));
  await act(async () =>
    currentChoice.resolve(Response.json(unavailableManualChoice)),
  );
  await choice().findByText("Manual source selection");
  await waitFor(() => expect(downloadReadCount).toBe(2));
  expect(unexpected).toEqual([]);
  expect(download().queryByText("Saved on this server")).toBeNull();
  for (const action of download().queryAllByRole("button", {
    name: /^Download(?: selected source)?$/,
  })) {
    expect(action).toHaveProperty("disabled", true);
    fireEvent.click(action);
  }
  expect(posted(downloadPath)).toHaveLength(0);
  await act(async () =>
    downloadReads[1].resolve(Response.json(previousDownloadA)),
  );
  expect(unexpected).toEqual([]);
  await download().findByText(/previous source/i);
  expect(
    download()
      .getAllByRole("status")
      .some((status) => /previous source/i.test(status.textContent ?? "")),
  ).toBe(true);
  expect(download().queryByText("Saved on this server")).toBeNull();
  const replace = download().getByRole("button", {
    name: "Download selected source",
  });
  expect(replace).toHaveProperty("disabled", false);
  expect(
    download().getByRole("button", { name: "Remove download" }),
  ).toHaveProperty("disabled", false);
  expect(posted(downloadPath)).toHaveLength(0);
  expect(title().value).toBe(draft);
  act(() => {
    fireEvent.click(replace);
    fireEvent.click(replace);
  });
  await waitFor(() => expect(posted(downloadPath)).toHaveLength(1));
  const rejected = posted(downloadPath)[0][1];
  expectOwnedRequest(rejected);
  const rejectedBody = JSON.parse(String(rejected?.body));
  expect(rejectedBody).toEqual({ request_id: expect.stringMatching(uuid) });
  await act(async () =>
    rejectedDownload.resolve(
      Response.json({ error: privateFailure }, { status: 409 }),
    ),
  );
  expect((await download().findByRole("alert")).textContent).toBe(
    "Hito could not start this download. Check audio availability and try again.",
  );
  expect(download().getByText(/previous source/i)).toBeTruthy();
  expect(download().queryByText("Saved on this server")).toBeNull();
  expect(
    download().queryByRole("button", { name: "Retry download request" }),
  ).toBeNull();
  expect(
    download().getByRole("button", { name: "Remove download" }),
  ).toHaveProperty("disabled", false);
  const retry = download().getByRole("button", {
    name: "Download selected source",
  });
  expect(retry).toHaveProperty("disabled", false);
  expect(posted(downloadPath)).toHaveLength(1);
  expect(document.body.textContent).not.toContain(privateCanary);
  expect(title().value).toBe(draft);
  sourceBAvailable = true;
  act(() => {
    fireEvent.click(retry);
    fireEvent.click(retry);
  });
  await waitFor(() => expect(posted(downloadPath)).toHaveLength(2));
  const accepted = posted(downloadPath)[1][1];
  expectOwnedRequest(accepted);
  const acceptedBody = JSON.parse(String(accepted?.body));
  expect(acceptedBody).toEqual({ request_id: expect.stringMatching(uuid) });
  expect(acceptedBody.request_id).not.toBe(rejectedBody.request_id);
  expect(accepted?.signal).not.toBe(rejected?.signal);
  await act(async () =>
    acceptedDownload.resolve(
      Response.json(
        {
          request_id: acceptedBody.request_id,
          recording_id: savedRecording.id,
          intent_id: readyDownloadB.intent_id,
        },
        { status: 202 },
      ),
    ),
  );
  await waitFor(() => expect(downloadReadCount).toBe(3));
  expect(download().queryByText("Saved on this server")).toBeNull();
  await act(async () =>
    downloadReads[2].resolve(Response.json(readyDownloadB)),
  );
  await download().findByText("Saved on this server");
  expect(download().queryByText(/previous source/i)).toBeNull();
  expect(
    download().queryByRole("button", { name: "Download selected source" }),
  ).toBeNull();
  expect(
    download().getByRole("button", { name: "Remove download" }),
  ).toHaveProperty("disabled", false);
  expect(title().value).toBe(draft);
  expect(
    fetch.mock.calls
      .filter(([, options]) => options?.method !== "GET")
      .map(([url, options]) => [url, options?.method]),
  ).toEqual([
    [choicePath, "POST"],
    [downloadPath, "POST"],
    [downloadPath, "POST"],
  ]);
  expect(unexpected).toEqual([]);
  expect(play).not.toHaveBeenCalled();
  expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();
  expect(router.reload).not.toHaveBeenCalled();
  expect(router.visit).not.toHaveBeenCalled();
  await act(async () => {
    window.dispatchEvent(new Event("focus"));
  });
  await waitFor(() => expect(downloadReadCount).toBe(4));
  const heldRead = fetch.mock.calls
    .filter(
      ([url, options]) => url === downloadPath && options?.method === "GET",
    )
    .at(-1)!;
  expectOwnedRequest(heldRead[1]);
  const beforeUnmount = fetch.mock.calls.length;
  view.unmount();
  expect(heldRead[1]?.signal?.aborted).toBe(true);
  expect(document.querySelectorAll("audio")).toHaveLength(0);
  await act(async () => {
    downloadReads[3].resolve(Response.json(previousDownloadA));
    window.dispatchEvent(new Event("focus"));
  });
  expect(fetch).toHaveBeenCalledTimes(beforeUnmount);
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
