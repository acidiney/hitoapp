// @vitest-environment jsdom
import { router } from "@inertiajs/react";
import { createElement, type ReactNode } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import RecordingDetailsPage from "./pages/RecordingDetailsPage";
import { NoticeProvider } from "./components/Notices";
import { PlaybackProvider } from "./player/PlaybackProvider";
import { attachedPage, savedRecording } from "./fixtures/sourceAttachment";
import {
  automaticChoice,
  choiceSourceA,
  choiceSourceB,
} from "./fixtures/sourceChoice";
import type {
  ChooseSource,
  PrepareSource,
  Source,
  SourceChoice,
  SourceChoiceReceipt,
  SourcePreparationReceipt,
  SourcePreparationStatus,
} from "./types";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
  localStorage.clear();
});

function heldResponse() {
  let resolve!: (response: Response) => void;
  let reject!: (error: Error) => void;
  const response = new Promise<Response>((accept, fail) => {
    resolve = accept;
    reject = fail;
  });
  return { response, resolve, reject };
}

function expectOwnedRequest(options: RequestInit | undefined) {
  const headers = new Headers(options?.headers);
  expect(headers.get("content-type")).toBe("application/json");
  expect(headers.get("x-hito-account")).toBe(attachedPage.user.id);
  expect(headers.get("x-hito-request")).toBe("1");
  expect(options?.credentials).toBe("same-origin");
  expect(options?.signal).toBeInstanceOf(AbortSignal);
  expect(options?.signal?.aborted).toBe(false);
}

test("confirming an unavailable choice invalidates old readiness without losing its uncertain preparation request", async () => {
  vi.useFakeTimers();
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const logs = (["debug", "info", "warn", "error", "log"] as const).map(
    (level) => vi.spyOn(console, level).mockImplementation(() => {}),
  );
  const sourcesPath = `/api/v1/recordings/${savedRecording.id}/sources`;
  const refreshPath = `${sourcesPath}/refresh`;
  const choicePath = `/api/v1/recordings/${savedRecording.id}/source-choice`;
  const downloadPath = `/api/v1/recordings/${savedRecording.id}/download`;
  const preparationPath = `${sourcesPath}/${choiceSourceB.id}/preparation`;
  const sourceB: Source = { ...choiceSourceB, ready: false, can_prepare: true };
  const initialSources: Source[] = [choiceSourceA, sourceB];
  const unavailableSources: Source[] = [
    { ...choiceSourceA, ready: false },
    sourceB,
  ];
  const automatic: SourceChoice = {
    ...automaticChoice,
    sources: initialSources.map((source) => ({
      source,
      selected: false,
      selectable: true,
    })),
  };
  const manual: SourceChoice = {
    ...automatic,
    revision: 1,
    mode: "manual",
    sources: unavailableSources.map((source) => ({
      source,
      selected: source.id === sourceB.id,
      selectable: true,
    })),
  };
  const preparationId = "b4019d4f-8bb7-44b7-9930-cf34f23ba41d";
  const preparing: SourcePreparationStatus = {
    preparation_id: preparationId,
    recording_id: savedRecording.id,
    source_id: sourceB.id,
    phase: "preparing",
    progress_basis_points: 2500,
    eta_seconds: null,
    reason: null,
    next_check_at: null,
    created_at: 1,
    updated_at: 1,
  };
  const privateCanary = "availability-choice-private-canary";
  const privateFailure = `private acknowledgement https://owned.invalid/audio?secret=${privateCanary}`;
  const draft = "Unsent availability correction title";
  const uuid =
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
  const lostPreparation = heldResponse();
  const retriedPreparation = heldResponse();
  const preparationRead = heldResponse();
  const olderRefresh = heldResponse();
  const freshAvailability = heldResponse();
  const choiceAcknowledgement = heldResponse();
  const currentChoice = heldResponse();
  const finalDownload = heldResponse();
  const accepted: { receipt?: SourcePreparationReceipt; body?: string } = {};
  let refreshCount = 0;
  let sourceReadCount = 0;
  let choiceReadCount = 0;
  let choiceWriteCount = 0;
  let preparationWriteCount = 0;
  let preparationReadCount = 0;
  let logicalPreparations = 0;
  let downloadReadCount = 0;
  const unexpected: { url: string; method: string }[] = [];
  const fetch = vi.fn(
    async (url: string, options?: RequestInit): Promise<Response> => {
      const method = options?.method ?? "GET";
      if (url === downloadPath && method === "GET") {
        downloadReadCount++;
        if (downloadReadCount === 1) return new Response(null, { status: 404 });
        if (downloadReadCount === 2) return finalDownload.response;
      }
      if (url === choicePath && method === "GET") {
        choiceReadCount++;
        if (choiceReadCount === 1) return Response.json(automatic);
        if (choiceReadCount === 2) return currentChoice.response;
      }
      if (url === choicePath && method === "POST" && ++choiceWriteCount === 1)
        return choiceAcknowledgement.response;
      if (url === refreshPath && method === "POST") {
        refreshCount++;
        if (refreshCount === 1) return Response.json(initialSources);
        if (refreshCount === 2) return olderRefresh.response;
      }
      if (url === sourcesPath && method === "GET" && ++sourceReadCount === 1)
        return freshAvailability.response;
      if (url === preparationPath && method === "GET") {
        preparationReadCount++;
        if (preparationReadCount === 1)
          return new Response(null, { status: 404 });
        if (preparationReadCount === 2 && preparationWriteCount === 2)
          return preparationRead.response;
      }
      if (url === preparationPath && method === "POST") {
        preparationWriteCount++;
        const body = String(options?.body);
        if (preparationWriteCount === 1) {
          const request = JSON.parse(body) as PrepareSource;
          accepted.body = body;
          accepted.receipt = {
            request_id: request.request_id,
            recording_id: savedRecording.id,
            source_id: sourceB.id,
            preparation_id: preparationId,
          };
          logicalPreparations++;
          return lostPreparation.response;
        }
        if (preparationWriteCount === 2 && accepted.body === body)
          return retriedPreparation.response;
      }
      unexpected.push({ url, method });
      return Response.json(
        { error: "Unexpected source availability request" },
        { status: 500 },
      );
    },
  );
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
  await act(async () => {});
  const availability = () =>
    within(screen.getByRole("region", { name: "Audio availability" }));
  const choice = () =>
    within(screen.getByRole("region", { name: "Playback source" }));
  const requests = (path: string, method: string) =>
    fetch.mock.calls.filter(
      ([url, options]) => url === path && options?.method === method,
    );
  const title = screen.getByRole("textbox", {
    name: "Track title",
  }) as HTMLInputElement;
  fireEvent.change(title, { target: { value: draft } });
  expect(choice().getByText("Automatic source selection")).toBeTruthy();
  expect(sourceReadCount).toBe(0);
  expect(unexpected).toEqual([]);
  await act(async () => {
    fireEvent.click(
      availability().getByRole("button", { name: "Check availability" }),
    );
  });
  expect(availability().getByText("Audio is ready to play.")).toBeTruthy();
  expect(
    availability().getByRole("button", { name: "Play track" }),
  ).toHaveProperty("disabled", false);
  expect(
    availability().getByRole("radio", { name: sourceB.label }),
  ).toHaveProperty("checked", true);
  expect(preparationReadCount).toBe(1);
  const prepare = availability().getByRole("button", { name: "Prepare audio" });
  await act(async () => {
    fireEvent.click(prepare);
    fireEvent.click(prepare);
  });
  expect(preparationWriteCount).toBe(1);
  expect(logicalPreparations).toBe(1);
  expect(choiceWriteCount).toBe(0);
  const originalPreparation = requests(preparationPath, "POST")[0][1];
  expectOwnedRequest(originalPreparation);
  expect(JSON.parse(String(originalPreparation?.body))).toEqual({
    request_id: expect.stringMatching(uuid),
  });
  await act(async () => lostPreparation.reject(new TypeError(privateFailure)));
  const retry = availability().getByRole("button", {
    name: "Retry preparation request",
  });
  expect(
    availability().getByText(
      "Hito could not confirm the preparation request. Retry the same request to recover its result.",
    ),
  ).toBeTruthy();
  expect(title.value).toBe(draft);
  expect(document.body.textContent).not.toContain(privateCanary);
  await act(async () => {
    fireEvent.click(
      availability().getByRole("button", { name: "Check again" }),
    );
  });
  expect(refreshCount).toBe(2);
  const refreshRequest = requests(refreshPath, "POST")[1][1];
  expectOwnedRequest(refreshRequest);
  expect(
    availability().getByRole("button", { name: "Retry preparation request" }),
  ).toBe(retry);
  fireEvent.click(choice().getByRole("radio", { name: sourceB.label }));
  const use = choice().getByRole("button", { name: "Use source" });
  await act(async () => {
    fireEvent.click(use);
    fireEvent.click(use);
  });
  expect(choiceWriteCount).toBe(1);
  const chosen = requests(choicePath, "POST")[0][1];
  expectOwnedRequest(chosen);
  const chosenBody = JSON.parse(String(chosen?.body)) as ChooseSource;
  expect(chosenBody).toEqual({
    request_id: expect.stringMatching(uuid),
    source_id: sourceB.id,
  });
  const choiceReceipt: SourceChoiceReceipt = {
    request_id: chosenBody.request_id,
    recording_id: savedRecording.id,
    source_id: sourceB.id,
    revision: 1,
  };
  await act(async () =>
    choiceAcknowledgement.resolve(Response.json(choiceReceipt)),
  );
  expect(choiceReadCount).toBe(2);
  expect(sourceReadCount).toBe(0);
  await act(async () => currentChoice.resolve(Response.json(manual)));
  expect(choice().getByText("Manual source selection")).toBeTruthy();
  expect(
    sourceReadCount,
    "a confirmed source choice must re-read current availability without a new provider refresh",
  ).toBe(1);
  const freshRead = requests(sourcesPath, "GET")[0][1];
  expectOwnedRequest(freshRead);
  expect(freshRead?.body).toBeUndefined();
  expect(refreshRequest?.signal?.aborted).toBe(true);
  expect(downloadReadCount).toBe(2);
  expect(refreshCount).toBe(2);
  expect(preparationWriteCount).toBe(1);
  expect(preparationReadCount).toBe(1);
  expect(availability().queryByText("Audio is ready to play.")).toBeNull();
  for (const button of availability().queryAllByRole("button", {
    name: "Play track",
  })) {
    expect(button).toHaveProperty("disabled", true);
    fireEvent.click(button);
  }
  expect(
    availability().getByRole("button", { name: "Retry preparation request" }),
  ).toBe(retry);
  expect(title).toBe(screen.getByRole("textbox", { name: "Track title" }));
  expect(title.value).toBe(draft);
  await act(async () =>
    freshAvailability.resolve(Response.json(unavailableSources)),
  );
  expect(availability().getByText("No audio is ready yet.")).toBeTruthy();
  expect(
    availability().queryByRole("button", { name: "Play track" }),
  ).toBeNull();
  await act(async () => olderRefresh.resolve(Response.json(initialSources)));
  expect(availability().getByText("No audio is ready yet.")).toBeTruthy();
  expect(
    availability().queryByRole("button", { name: "Play track" }),
  ).toBeNull();
  expect(
    availability().getByRole("button", { name: "Retry preparation request" }),
  ).toBe(retry);
  expect(preparationReadCount).toBe(1);
  const settledReads = fetch.mock.calls.length;
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
  });
  expect(fetch).toHaveBeenCalledTimes(settledReads);
  await act(async () => {
    fireEvent.click(retry);
    fireEvent.click(retry);
  });
  expect(preparationWriteCount).toBe(2);
  expect(logicalPreparations).toBe(1);
  const repeatedPreparation = requests(preparationPath, "POST")[1][1];
  expectOwnedRequest(repeatedPreparation);
  expect(repeatedPreparation?.body).toBe(originalPreparation?.body);
  expect(repeatedPreparation?.signal).not.toBe(originalPreparation?.signal);
  expect(retry).toHaveProperty("disabled", true);
  expect(accepted.receipt).toBeDefined();
  await act(async () =>
    retriedPreparation.resolve(
      Response.json(accepted.receipt, { status: 202 }),
    ),
  );
  expect(preparationReadCount).toBe(2);
  expect(
    availability().queryByRole("button", { name: "Retry preparation request" }),
  ).toBeNull();
  expect(sourceReadCount).toBe(1);
  expect(title.value).toBe(draft);
  expect(unexpected).toEqual([]);
  expect(
    fetch.mock.calls
      .filter(([, options]) => options?.method === "POST")
      .map(([url]) => url),
  ).toEqual([
    refreshPath,
    preparationPath,
    refreshPath,
    choicePath,
    preparationPath,
  ]);
  expect(fetch.mock.calls.every(([url]) => !url.includes("/audio"))).toBe(true);
  expect(requests(downloadPath, "POST")).toHaveLength(0);
  expect(play).not.toHaveBeenCalled();
  expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();
  expect(router.reload).not.toHaveBeenCalled();
  expect(router.visit).not.toHaveBeenCalled();
  expect(document.body.textContent).not.toContain(privateCanary);
  for (const [, options] of fetch.mock.calls) {
    const headers = new Headers(options?.headers);
    expect(headers.get("x-hito-account")).toBe(attachedPage.user.id);
    expect(options?.credentials).toBe("same-origin");
    expect(options?.signal).toBeInstanceOf(AbortSignal);
  }
  const observedPreparation = requests(preparationPath, "GET")[1][1];
  const downloadRead = requests(downloadPath, "GET")[1][1];
  expectOwnedRequest(observedPreparation);
  expectOwnedRequest(downloadRead);
  const beforeUnmount = fetch.mock.calls.length;
  view.unmount();
  expect(observedPreparation?.signal?.aborted).toBe(true);
  expect(downloadRead?.signal?.aborted).toBe(true);
  expect(freshRead?.signal?.aborted).toBe(true);
  expect(document.querySelectorAll("audio")).toHaveLength(0);
  await act(async () => {
    preparationRead.resolve(Response.json(preparing));
    finalDownload.resolve(new Response(null, { status: 404 }));
    window.dispatchEvent(new Event("focus"));
    await vi.advanceTimersByTimeAsync(60_000);
  });
  expect(fetch).toHaveBeenCalledTimes(beforeUnmount);
  expect(vi.getTimerCount()).toBe(0);
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
