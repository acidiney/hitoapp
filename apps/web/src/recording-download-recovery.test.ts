import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import RecordingDetailsPage from "./pages/RecordingDetailsPage";
import { NoticeProvider } from "./shared/ui/Notices";
import { PlaybackProvider } from "./player/PlaybackProvider";
import { attachedPage, savedRecording } from "./fixtures/sourceAttachment";
import type { DownloadReceipt, DownloadRequest, DownloadStatus } from "./shared/types";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

test("a lost download acknowledgement retries the exact request with a new live signal before accepting its saved copy", async () => {
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const warnings = vi.spyOn(console, "warn").mockImplementation(() => {});
  const path = `/api/v1/recordings/${savedRecording.id}/download`;
  const sourceChoice = `/api/v1/recordings/${savedRecording.id}/source-choice`;
  const privateFailure = "private signed download response failure";
  const ready: DownloadStatus = {
    status: "ready",
    intent_id: "2e98962a-29a4-4e53-ad1c-b4c13c010cda",
    recording_id: savedRecording.id,
    source_id: "4a79410c-7ddb-4bc4-bc36-4e8672745034",
    host_device_id: "owned-download-host",
    choice_revision: 0,
    matches_source_choice: true,
    bytes_received: 16_044,
    total_bytes: 16_044,
    error: null,
  };
  let committed = false;
  let writes = 0;
  let finishRetry!: (response: Response) => void;
  const fetch = vi.fn(async (url: string, options?: RequestInit) => {
    if (url === sourceChoice && options?.method === "GET")
      return Response.json({ error: "Not found" }, { status: 404 });
    if (url !== path)
      throw new Error("Unexpected request outside owned download recovery");
    if (options?.method === "POST") {
      writes++;
      committed = true;
      if (writes === 1) throw new TypeError(privateFailure);
      return new Promise<Response>((resolve) => {
        finishRetry = resolve;
      });
    }
    if (options?.method === "GET")
      return committed
        ? Response.json(ready)
        : Response.json({ error: "Not found" }, { status: 404 });
    throw new Error("Download recovery must not remove or play audio");
  });
  vi.stubGlobal("fetch", fetch);
  render(
    createElement(
      NoticeProvider,
      null,
      createElement(PlaybackProvider, {
        account: attachedPage.user.id,
        children: createElement(RecordingDetailsPage, attachedPage),
      }),
    ),
  );
  await act(async () => {});
  const choiceReads = fetch.mock.calls.filter(
    ([url, options]) => url === sourceChoice && options?.method === "GET",
  );
  expect(choiceReads).toHaveLength(1);
  expect(choiceReads[0][1]).toMatchObject({
    credentials: "same-origin",
    headers: { "x-hito-account": attachedPage.user.id },
    signal: expect.any(AbortSignal),
  });
  expect(choiceReads[0][1]?.signal?.aborted).toBe(false);
  expect(choiceReads[0][1]?.body).toBeUndefined();
  expect(writes).toBe(0);
  fireEvent.click(screen.getByRole("button", { name: "Download" }));
  const retry = await screen.findByRole("button", {
    name: "Retry download request",
  });
  const posted = () =>
    fetch.mock.calls.filter(([, options]) => options?.method === "POST");
  expect(posted()).toHaveLength(1);
  const original = posted()[0][1]!;
  const body: DownloadRequest = JSON.parse(String(original.body));
  expect(body).toEqual({
    request_id: expect.stringMatching(
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
    ),
  });
  expect(screen.getByRole("alert").textContent).toBe(
    "Hito could not confirm the download request. Retry the same request to recover its result.",
  );
  expect(screen.queryByText(privateFailure)).toBeNull();
  expect(screen.queryByText("Saved on this server")).toBeNull();
  const beforeFocus = fetch.mock.calls.length;
  await act(async () => {
    window.dispatchEvent(new Event("focus"));
  });
  expect(fetch).toHaveBeenCalledTimes(beforeFocus);
  const title = screen.getByRole("textbox", {
    name: "Track title",
  }) as HTMLInputElement;
  fireEvent.change(title, { target: { value: "Unsent recovery draft" } });
  act(() => {
    fireEvent.click(retry);
    fireEvent.click(retry);
  });
  await waitFor(() => expect(posted()).toHaveLength(2));
  const repeated = posted()[1];
  expect(repeated[0]).toBe(path);
  expect(repeated[1]?.body).toBe(original.body);
  expect(repeated[1]?.headers).toMatchObject({
    "x-hito-account": attachedPage.user.id,
    "x-hito-request": "1",
  });
  expect(repeated[1]?.credentials).toBe("same-origin");
  expect(repeated[1]?.signal).toBeInstanceOf(AbortSignal);
  expect(repeated[1]?.signal).not.toBe(original.signal);
  expect(repeated[1]?.signal?.aborted).toBe(false);
  expect(screen.queryByText("Saved on this server")).toBeNull();
  await act(async () =>
    finishRetry(
      Response.json(
        {
          request_id: body.request_id,
          intent_id: ready.intent_id,
          recording_id: savedRecording.id,
        } satisfies DownloadReceipt,
        { status: 202 },
      ),
    ),
  );
  await screen.findByText("Saved on this server");
  expect(posted()).toHaveLength(2);
  expect(
    screen.queryByRole("button", { name: "Retry download request" }),
  ).toBeNull();
  expect(title.value).toBe("Unsent recovery draft");
  expect(play).not.toHaveBeenCalled();
  expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();
  expect(JSON.stringify(warnings.mock.calls)).not.toContain(privateFailure);
  expect(
    fetch.mock.calls.filter(
      ([url, options]) => url === sourceChoice && options?.method === "GET",
    ),
  ).toHaveLength(1);
});
