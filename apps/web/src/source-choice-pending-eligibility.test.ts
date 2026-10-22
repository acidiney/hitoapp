import { router } from "@inertiajs/react";
import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import RecordingDetailsPage from "./pages/RecordingDetailsPage";
import { NoticeProvider } from "./components/Notices";
import { PlaybackProvider } from "./player/PlaybackProvider";
import { attachedPage, savedRecording } from "./fixtures/sourceAttachment";
import {
  readyDownloadA,
  previousDownloadA,
} from "./fixtures/sourceChoiceDownload";
import {
  chosenEligibility,
  chosenSourceB,
  failedPreparation,
  initialChoice,
  initialEligibility,
  preparableSourceB,
} from "./fixtures/sourceChoicePendingEligibility";
import type { SourceChoiceReceipt, SourcePreparationReceipt } from "./types";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

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

test("an unresolved source choice gates new work while preserving exact preparation recovery", async () => {
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const logs = (["debug", "info", "warn", "error", "log"] as const).map(
    (level) => vi.spyOn(console, level).mockImplementation(() => {}),
  );
  const base = `/api/v1/recordings/${savedRecording.id}`;
  const choicePath = `${base}/source-choice`;
  const downloadPath = `${base}/download`;
  const sourcesPath = `${base}/sources`;
  const refreshPath = `${sourcesPath}/refresh`;
  const preparationPath = `${sourcesPath}/${preparableSourceB.id}/preparation`;
  const privateCanary = "pending-source-choice-private-canary";
  const privateFailure = `private failure https://owned.invalid/audio?secret=${privateCanary}`;
  const draft = "Unsent pending source choice title";
  const uuid =
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
  const lostPreparation = heldResponse();
  const repeatedPreparation = heldResponse();
  const lostChoice = heldResponse();
  const repeatedChoice = heldResponse();
  const failedChoiceRead = heldResponse();
  const recoveredChoiceRead = heldResponse();
  const freshAvailability = heldResponse();
  const freshDownload = heldResponse();
  const cleanupDownload = heldResponse();
  let choiceReadCount = 0;
  let choiceWriteCount = 0;
  let refreshCount = 0;
  let preparationReadCount = 0;
  let preparationWriteCount = 0;
  let verifiedSourceReads = 0;
  let verifiedDownloadReads = 0;
  let currentChoiceConfirmed = false;
  let observingCleanup = false;
  let preparationReceipt: SourcePreparationReceipt | undefined;
  const unexpected: { url: string; method: string }[] = [];
  const fetch = vi.fn(async (url: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (url === downloadPath && method === "GET") {
      if (observingCleanup) return cleanupDownload.response;
      if (!currentChoiceConfirmed)
        return Response.json(
          choiceWriteCount > 0 ? previousDownloadA : readyDownloadA,
        );
      verifiedDownloadReads++;
      return freshDownload.response;
    }
    if (url === sourcesPath && method === "GET") {
      if (!currentChoiceConfirmed)
        return Response.json(
          choiceWriteCount > 0 ? chosenEligibility : initialEligibility,
        );
      verifiedSourceReads++;
      return freshAvailability.response;
    }
    if (url === choicePath && method === "GET") {
      choiceReadCount++;
      if (choiceReadCount === 1) return Response.json(initialChoice);
      if (choiceReadCount === 2) return failedChoiceRead.response;
      if (choiceReadCount === 3) return recoveredChoiceRead.response;
    }
    if (url === choicePath && method === "POST") {
      choiceWriteCount++;
      if (choiceWriteCount === 1) return lostChoice.response;
      if (choiceWriteCount === 2) return repeatedChoice.response;
    }
    if (url === refreshPath && method === "POST" && ++refreshCount === 1)
      return Response.json(initialEligibility);
    if (url === preparationPath && method === "GET") {
      preparationReadCount++;
      if (preparationReadCount === 1)
        return new Response(null, { status: 404 });
      if (preparationReadCount === 2 && preparationWriteCount === 2)
        return Response.json(failedPreparation);
    }
    if (url === preparationPath && method === "POST") {
      preparationWriteCount++;
      if (preparationWriteCount === 1) {
        const body = JSON.parse(String(options?.body));
        preparationReceipt = {
          request_id: body.request_id,
          recording_id: savedRecording.id,
          source_id: preparableSourceB.id,
          preparation_id: failedPreparation.preparation_id,
        };
        return lostPreparation.response;
      }
      if (preparationWriteCount === 2) return repeatedPreparation.response;
    }
    unexpected.push({ url, method });
    return Response.json(
      { error: "Unexpected pending source choice request" },
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
  const availability = () =>
    within(screen.getByRole("region", { name: "Audio availability" }));
  const choice = () =>
    within(screen.getByRole("region", { name: "Playback source" }));
  const download = () =>
    within(screen.getByRole("region", { name: "Download" }));
  const title = () =>
    screen.getByRole("textbox", { name: "Track title" }) as HTMLInputElement;
  const requests = (path: string, method: string) =>
    fetch.mock.calls.filter(
      ([url, options]) => url === path && options?.method === method,
    );
  function expectDependentActionsBlocked() {
    const before = fetch.mock.calls.filter(
      ([, options]) => !["GET", "HEAD"].includes(options?.method ?? "GET"),
    ).length;
    for (const name of ["Play track", "Prepare audio"]) {
      for (const button of availability().queryAllByRole("button", {
        name,
      })) {
        expect(button).toHaveProperty("disabled", true);
        fireEvent.click(button);
      }
    }
    for (const name of [
      "Download",
      "Download selected source",
      "Remove download",
    ]) {
      for (const button of download().queryAllByRole("button", {
        name,
      })) {
        expect(button).toHaveProperty("disabled", true);
        fireEvent.click(button);
      }
    }
    expect(
      fetch.mock.calls.filter(
        ([, options]) => !["GET", "HEAD"].includes(options?.method ?? "GET"),
      ),
    ).toHaveLength(before);
    expect(play).not.toHaveBeenCalled();
    expect(unexpected).toEqual([]);
    expect(title().value).toBe(draft);
    expect(document.body.textContent).not.toContain(privateCanary);
  }
  await choice().findByText("Automatic source selection");
  await download().findByText("Saved on this server");
  fireEvent.change(title(), { target: { value: draft } });
  fireEvent.click(
    availability().getByRole("button", { name: "Check availability" }),
  );
  await availability().findByText("Audio is ready to play.");
  expect(
    availability().getByRole("button", { name: "Play track" }),
  ).toHaveProperty("disabled", false);
  expect(
    download().getByRole("button", { name: "Remove download" }),
  ).toHaveProperty("disabled", false);
  const prepare = await availability().findByRole("button", {
    name: "Prepare audio",
  });
  await waitFor(() => expect(prepare).toHaveProperty("disabled", false));
  act(() => {
    fireEvent.click(prepare);
    fireEvent.click(prepare);
  });
  await waitFor(() => expect(preparationWriteCount).toBe(1));
  const originalPreparation = requests(preparationPath, "POST")[0][1];
  expectOwnedRequest(originalPreparation);
  expect(JSON.parse(String(originalPreparation?.body))).toEqual({
    request_id: expect.stringMatching(uuid),
  });
  await act(async () => lostPreparation.reject(new TypeError(privateFailure)));
  expect(
    await availability().findByRole("button", {
      name: "Retry preparation request",
    }),
  ).toHaveProperty("disabled", false);
  expect(document.body.textContent).not.toContain(privateCanary);
  fireEvent.click(
    choice().getByRole("radio", { name: preparableSourceB.label }),
  );
  const use = choice().getByRole("button", { name: "Use source" });
  expect(use).toHaveProperty("disabled", false);
  act(() => {
    fireEvent.click(use);
    fireEvent.click(use);
  });
  await waitFor(() => expect(choiceWriteCount).toBe(1));
  const originalChoice = requests(choicePath, "POST")[0][1];
  expectOwnedRequest(originalChoice);
  const choiceBody = JSON.parse(String(originalChoice?.body));
  expect(choiceBody).toEqual({
    request_id: expect.stringMatching(uuid),
    source_id: preparableSourceB.id,
  });
  expectDependentActionsBlocked();
  const retryPreparation = availability().getByRole("button", {
    name: "Retry preparation request",
  });
  expect(retryPreparation).toHaveProperty("disabled", false);
  act(() => {
    fireEvent.click(retryPreparation);
    fireEvent.click(retryPreparation);
  });
  await waitFor(() => expect(preparationWriteCount).toBe(2));
  const preparationRecovery = requests(preparationPath, "POST")[1][1];
  expectOwnedRequest(preparationRecovery);
  expect(preparationRecovery?.body).toBe(originalPreparation?.body);
  expect(preparationRecovery?.signal).not.toBe(originalPreparation?.signal);
  expect(originalChoice?.signal?.aborted).toBe(false);
  expectDependentActionsBlocked();
  await act(async () => lostChoice.reject(new TypeError(privateFailure)));
  expect(preparationRecovery?.signal?.aborted).toBe(false);
  expect(choice().getByRole("alert").textContent).toBe(
    "Hito could not confirm the source choice. Retry the same request to recover its result.",
  );
  expectDependentActionsBlocked();
  expect(
    await choice().findByRole("button", { name: "Retry source choice" }),
  ).toHaveProperty("disabled", false);
  expect(preparationReceipt).toBeDefined();
  await act(async () =>
    repeatedPreparation.resolve(
      Response.json(preparationReceipt, { status: 202 }),
    ),
  );
  await availability().findByText("Audio preparation failed.");
  expect(preparationReadCount).toBe(2);
  expectDependentActionsBlocked();
  const retryChoice = choice().getByRole("button", {
    name: "Retry source choice",
  });
  act(() => {
    fireEvent.click(retryChoice);
    fireEvent.click(retryChoice);
  });
  await waitFor(() => expect(choiceWriteCount).toBe(2));
  const choiceRecovery = requests(choicePath, "POST")[1][1];
  expectOwnedRequest(choiceRecovery);
  expect(choiceRecovery?.body).toBe(originalChoice?.body);
  expect(choiceRecovery?.signal).not.toBe(originalChoice?.signal);
  expectDependentActionsBlocked();
  const receipt: SourceChoiceReceipt = {
    request_id: choiceBody.request_id,
    recording_id: savedRecording.id,
    source_id: preparableSourceB.id,
    revision: 1,
  };
  await act(async () => repeatedChoice.resolve(Response.json(receipt)));
  await waitFor(() => expect(choiceReadCount).toBe(2));
  expectDependentActionsBlocked();
  await act(async () =>
    failedChoiceRead.resolve(
      Response.json({ error: privateFailure }, { status: 503 }),
    ),
  );
  const checkChoice = await choice().findByRole("button", {
    name: "Check source choice",
  });
  expect(choice().getByRole("alert").textContent).toBe(
    "Hito could not check the current source choice. Try again.",
  );
  expectDependentActionsBlocked();
  fireEvent.click(checkChoice);
  await waitFor(() => expect(choiceReadCount).toBe(3));
  expectDependentActionsBlocked();
  await act(async () => {
    currentChoiceConfirmed = true;
    recoveredChoiceRead.resolve(Response.json(chosenSourceB));
  });
  await choice().findByText("Manual source selection");
  await waitFor(() => {
    expect(verifiedSourceReads).toBe(1);
    expect(verifiedDownloadReads).toBe(1);
  });
  expectDependentActionsBlocked();
  await act(async () =>
    freshAvailability.resolve(Response.json(chosenEligibility)),
  );
  await availability().findByText("No audio is ready yet.");
  expect(
    availability().queryByRole("button", { name: "Play track" }),
  ).toBeNull();
  expect(
    availability().getByRole("button", { name: "Prepare audio" }),
  ).toHaveProperty("disabled", false);
  for (const button of download().queryAllByRole("button")) {
    expect(button).toHaveProperty("disabled", true);
  }
  await act(async () =>
    freshDownload.resolve(Response.json(previousDownloadA)),
  );
  await download().findByText(/previous source/i);
  expect(
    download().getByRole("button", { name: "Download selected source" }),
  ).toHaveProperty("disabled", false);
  expect(
    download().getByRole("button", { name: "Remove download" }),
  ).toHaveProperty("disabled", false);
  expect(download().queryByText("Saved on this server")).toBeNull();
  expect(title().value).toBe(draft);
  expect(refreshCount).toBe(1);
  expect(preparationWriteCount).toBe(2);
  expect(choiceWriteCount).toBe(2);
  expect(
    fetch.mock.calls
      .filter(([, options]) => options?.method !== "GET")
      .map(([url, options]) => [url, options?.method]),
  ).toEqual([
    [refreshPath, "POST"],
    [preparationPath, "POST"],
    [choicePath, "POST"],
    [preparationPath, "POST"],
    [choicePath, "POST"],
  ]);
  expect(play).not.toHaveBeenCalled();
  expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();
  expect(router.reload).not.toHaveBeenCalled();
  expect(router.visit).not.toHaveBeenCalled();
  expect(document.body.textContent).not.toContain(privateCanary);
  for (const [, options] of fetch.mock.calls) {
    const headers = new Headers(options?.headers);
    expect(headers.get("content-type")).toBe("application/json");
    expect(headers.get("x-hito-account")).toBe(attachedPage.user.id);
    expect(headers.get("x-hito-request")).toBe("1");
    expect(options?.credentials).toBe("same-origin");
    expect(options?.signal).toBeInstanceOf(AbortSignal);
  }
  observingCleanup = true;
  await act(async () => {
    window.dispatchEvent(new Event("focus"));
  });
  const cleanupRequest = requests(downloadPath, "GET").at(-1)![1];
  expectOwnedRequest(cleanupRequest);
  const beforeUnmount = fetch.mock.calls.length;
  view.unmount();
  expect(cleanupRequest?.signal?.aborted).toBe(true);
  expect(document.querySelectorAll("audio")).toHaveLength(0);
  await act(async () => {
    cleanupDownload.resolve(Response.json(previousDownloadA));
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
  expect(logged).not.toContain(privateCanary);
  expect(logged).not.toContain(draft);
});
