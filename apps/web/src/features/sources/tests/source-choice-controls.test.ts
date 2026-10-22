import { router } from "@inertiajs/react";
import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import RecordingDetailsPage from "../../recordings/pages/RecordingDetailsPage";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { PlaybackProvider } from "../../playback/components/PlaybackProvider";
import { attachedPage, savedRecording } from "../fixtures/sourceAttachment";
import {
  automaticChoice,
  choiceSourceA,
  choiceSourceB,
  currentManualChoice,
} from "../fixtures/sourceChoice";
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

test("source choice retries and resets current state while preserving a draft", async () => {
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const logs = (["debug", "info", "warn", "error", "log"] as const).map(
    (level) => vi.spyOn(console, level).mockImplementation(() => {}),
  );
  const path = `/api/v1/recordings/${savedRecording.id}/source-choice`;
  const resetPath = `${path}/reset`;
  const downloadPath = `/api/v1/recordings/${savedRecording.id}/download`;
  const sourcesPath = `/api/v1/recordings/${savedRecording.id}/sources`;
  const privateCanary = "choice-private-canary";
  const privateFailure = `private source response https://owned.invalid/audio?secret=${privateCanary}`;
  const draft = "Unsent source choice title";
  const uuid =
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
  const initialDownload = heldResponse();
  const finalDownload = heldResponse();
  const choiceReads = [heldResponse(), heldResponse(), heldResponse()];
  const lostChoice = heldResponse();
  const retriedChoice = heldResponse();
  const resetChoice = heldResponse();
  let readCount = 0;
  let downloadReads = 0;
  let choiceWrites = 0;
  let resetWrites = 0;
  const unexpected: { url: string; method: string }[] = [];
  const fetch = vi.fn(async (url: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (url === downloadPath && method === "GET") {
      downloadReads++;
      if (downloadReads === 1) return initialDownload.response;
      if (downloadReads === 3) return finalDownload.response;
      return Response.json({ error: "Not found" }, { status: 404 });
    }
    if (url === sourcesPath && method === "GET") return Response.json([]);
    if (url === path && method === "GET" && readCount < choiceReads.length)
      return choiceReads[readCount++].response;
    if (url === path && method === "POST") {
      choiceWrites++;
      if (choiceWrites === 1) return lostChoice.response;
      if (choiceWrites === 2) return retriedChoice.response;
    }
    if (url === resetPath && method === "POST" && resetWrites++ === 0)
      return resetChoice.response;
    unexpected.push({ url, method });
    return Response.json(
      { error: "Unexpected source choice request" },
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
  await waitFor(() => expect(downloadReads).toBe(1));
  await act(async () => {
    initialDownload.resolve(
      Response.json({ error: "Not found" }, { status: 404 }),
    );
    choiceReads[0].resolve(Response.json(automaticChoice));
  });
  await screen.findByRole("region", { name: "Playback source" });
  const controls = () =>
    within(screen.getByRole("region", { name: "Playback source" }));
  await controls().findByText("Automatic source selection");
  expect(readCount).toBe(1);
  expect(unexpected).toEqual([]);
  expect(
    fetch.mock.calls.every(([, options]) => options?.method === "GET"),
  ).toBe(true);
  expect(
    controls().getByRole("radio", { name: choiceSourceA.label }),
  ).toHaveProperty("checked", false);
  expect(
    controls().getByRole("radio", { name: choiceSourceB.label }),
  ).toHaveProperty("checked", false);
  expect(play).not.toHaveBeenCalled();
  expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();
  const title = () =>
    screen.getByRole("textbox", { name: "Track title" }) as HTMLInputElement;
  fireEvent.change(title(), { target: { value: draft } });
  fireEvent.click(controls().getByRole("radio", { name: choiceSourceB.label }));
  const use = controls().getByRole("button", { name: "Use source" });
  act(() => {
    fireEvent.click(use);
    fireEvent.click(use);
  });
  const posted = () =>
    fetch.mock.calls.filter(([, options]) => options?.method === "POST");
  await waitFor(() => expect(posted()).toHaveLength(1));
  const original = posted()[0];
  expect(original[0]).toBe(path);
  expectOwnedRequest(original[1]);
  const request = JSON.parse(String(original[1]?.body));
  expect(request).toEqual({
    request_id: expect.stringMatching(uuid),
    source_id: choiceSourceB.id,
  });
  expect(use).toHaveProperty("disabled", true);
  await act(async () => lostChoice.reject(new TypeError(privateFailure)));
  const retry = await controls().findByRole("button", {
    name: "Retry source choice",
  });
  expect(controls().getByRole("alert").textContent).toMatch(/source choice/i);
  expect(controls().getByRole("alert").textContent).toMatch(/confirm/i);
  expect(document.body.textContent).not.toContain(privateCanary);
  expect(controls().getByRole("button", { name: "Use source" })).toHaveProperty(
    "disabled",
    true,
  );
  fireEvent.click(controls().getByRole("button", { name: "Use source" }));
  expect(posted()).toHaveLength(1);
  expect(title().value).toBe(draft);
  act(() => {
    fireEvent.click(retry);
    fireEvent.click(retry);
  });
  await waitFor(() => expect(posted()).toHaveLength(2));
  const repeated = posted()[1];
  expect(repeated[0]).toBe(path);
  expect(repeated[1]?.body).toBe(original[1]?.body);
  expectOwnedRequest(repeated[1]);
  expect(repeated[1]?.signal).not.toBe(original[1]?.signal);
  expect(readCount).toBe(1);
  await act(async () => {
    retriedChoice.resolve(
      Response.json({
        request_id: request.request_id,
        recording_id: savedRecording.id,
        source_id: choiceSourceB.id,
        revision: 1,
      }),
    );
  });
  await waitFor(() => expect(readCount).toBe(2));
  expect(controls().queryByText("Manual source selection")).toBeNull();
  expect(
    controls().queryByRole("button", { name: "Return to automatic" }),
  ).toBeNull();
  await act(async () =>
    choiceReads[1].resolve(Response.json(currentManualChoice)),
  );
  await controls().findByText("Manual source selection");
  await waitFor(() => expect(downloadReads).toBe(2));
  expect(
    controls().getByRole("radio", { name: choiceSourceB.label }),
  ).toHaveProperty("checked", true);
  expect(
    controls().queryByRole("button", { name: "Retry source choice" }),
  ).toBeNull();
  const reset = controls().getByRole("button", { name: "Return to automatic" });
  act(() => {
    fireEvent.click(reset);
    fireEvent.click(reset);
  });
  await waitFor(() => expect(posted()).toHaveLength(3));
  const resetRequest = posted()[2];
  expect(resetRequest[0]).toBe(resetPath);
  expectOwnedRequest(resetRequest[1]);
  const resetBody = JSON.parse(String(resetRequest[1]?.body));
  expect(resetBody).toEqual({
    request_id: expect.stringMatching(uuid),
    expected_revision: 3,
  });
  expect(resetBody.request_id).not.toBe(request.request_id);
  expect(reset).toHaveProperty("disabled", true);
  await act(async () => {
    resetChoice.resolve(
      Response.json({
        request_id: resetBody.request_id,
        recording_id: savedRecording.id,
        revision: 4,
      }),
    );
  });
  await waitFor(() => expect(readCount).toBe(3));
  expect(controls().queryByText("Automatic source selection")).toBeNull();
  await act(async () =>
    choiceReads[2].resolve(Response.json({ ...automaticChoice, revision: 4 })),
  );
  await controls().findByText("Automatic source selection");
  await waitFor(() => expect(downloadReads).toBe(3));
  expect(
    controls().getByRole("radio", { name: choiceSourceA.label }),
  ).toHaveProperty("checked", false);
  expect(
    controls().getByRole("radio", { name: choiceSourceB.label }),
  ).toHaveProperty("checked", false);
  expect(
    controls().queryByRole("button", { name: "Return to automatic" }),
  ).toBeNull();
  expect(
    controls().queryByRole("button", { name: "Retry source choice" }),
  ).toBeNull();
  expect(title().value).toBe(draft);
  expect(posted()).toHaveLength(3);
  expect(
    fetch.mock.calls
      .filter(([, options]) => options?.method !== "GET")
      .map(([url, options]) => [url, options?.method]),
  ).toEqual([
    [path, "POST"],
    [path, "POST"],
    [resetPath, "POST"],
  ]);
  expect(unexpected).toEqual([]);
  expect(play).not.toHaveBeenCalled();
  expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();
  expect(router.reload).not.toHaveBeenCalled();
  expect(router.visit).not.toHaveBeenCalled();
  expect(document.body.textContent).not.toContain(privateCanary);
  const finalRead = fetch.mock.calls
    .filter(
      ([url, options]) => url === downloadPath && options?.method === "GET",
    )
    .at(-1)!;
  expectOwnedRequest(finalRead[1]);
  const beforeUnmount = fetch.mock.calls.length;
  view.unmount();
  expect(finalRead[1]?.signal?.aborted).toBe(true);
  expect(document.querySelectorAll("audio")).toHaveLength(0);
  await act(async () => {
    finalDownload.resolve(
      Response.json({ error: "Not found" }, { status: 404 }),
    );
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
