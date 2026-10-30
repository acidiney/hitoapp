import type { Schema } from "../../../shared/schema";
import { router } from "@inertiajs/react";
import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import RecordingDetailsPage from "../../recordings/pages/RecordingDetailsPage";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { PlaybackProvider } from "../../playback/components/PlaybackProvider";
import { attachedPage, savedRecording } from "../fixtures/sourceAttachment";
import {
  automaticChoice,
  choiceSourceA,
  choiceSourceB,
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

test("choice changes recover failed requested availability while leaving untouched availability unrequested", async () => {
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
  const resetPath = `${choicePath}/reset`;
  const downloadPath = `/api/v1/recordings/${savedRecording.id}/download`;
  const sourceB: Schema<"Source"> = {
    ...choiceSourceB,
    ready: false,
    can_prepare: false,
  };
  const readySources: Schema<"Source">[] = [choiceSourceA, sourceB];
  const firstAvailability = heldResponse();
  const recoveredAvailability = heldResponse();
  const finalDownload = heldResponse();
  const privateCanary = "availability-recovery-private-canary";
  const busyFailure = `private busy diagnostics https://owned.invalid/audio?secret=${privateCanary}`;
  const unavailableFailure = `private read diagnostics https://owned.invalid/stream?secret=${privateCanary}`;
  const draft = "Unsent failed availability recovery title";
  const uuid =
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
  let revision = 0;
  let choiceReadCount = 0;
  let chooseCount = 0;
  let resetCount = 0;
  let sourceReadCount = 0;
  let refreshCount = 0;
  let downloadReadCount = 0;
  const unexpected: { url: string; method: string }[] = [];
  const fetch = vi.fn(
    async (url: string, options?: RequestInit): Promise<Response> => {
      const method = options?.method ?? "GET";
      if (url === downloadPath && method === "GET") {
        downloadReadCount++;
        if (downloadReadCount < 5) return new Response(null, { status: 404 });
        if (downloadReadCount === 5) return finalDownload.response;
      }
      if (url === choicePath && method === "GET" && ++choiceReadCount <= 5) {
        const manual = revision % 2 === 1;
        const sources: Schema<"Source">[] = manual
          ? [{ ...choiceSourceA, ready: false }, sourceB]
          : readySources;
        const current: Schema<"SourceChoice"> = {
          ...automaticChoice,
          revision,
          mode: manual ? "manual" : "automatic",
          sources: sources.map((source) => ({
            source,
            selected: manual && source.id === sourceB.id,
            selectable: true,
          })),
        };
        return Response.json(current);
      }
      if (
        url === choicePath &&
        method === "POST" &&
        [0, 2].includes(revision)
      ) {
        const request = JSON.parse(
          String(options?.body),
        ) as Schema<"ChooseSource">;
        if (request.source_id === sourceB.id) {
          chooseCount++;
          revision++;
          const receipt: Schema<"SourceChoiceReceipt"> = {
            request_id: request.request_id,
            recording_id: savedRecording.id,
            source_id: sourceB.id,
            revision,
          };
          return Response.json(receipt);
        }
      }
      if (url === resetPath && method === "POST" && [1, 3].includes(revision)) {
        const request = JSON.parse(
          String(options?.body),
        ) as Schema<"ResetSourceChoice">;
        if (request.expected_revision === revision) {
          resetCount++;
          revision++;
          const receipt: Schema<"SourceChoiceResetReceipt"> = {
            request_id: request.request_id,
            recording_id: savedRecording.id,
            revision,
          };
          return Response.json(receipt);
        }
      }
      if (url === refreshPath && method === "POST" && ++refreshCount === 1)
        return Response.json({ error: busyFailure }, { status: 409 });
      if (url === sourcesPath && method === "GET") {
        sourceReadCount++;
        if (sourceReadCount === 1) return firstAvailability.response;
        if (sourceReadCount === 2) return recoveredAvailability.response;
      }
      unexpected.push({ url, method });
      return Response.json(
        { error: "Unexpected availability recovery request" },
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
  expect(choiceReadCount).toBe(1);
  expect(downloadReadCount).toBe(1);
  expect(sourceReadCount).toBe(0);
  expect(refreshCount).toBe(0);
  expect(unexpected).toEqual([]);
  fireEvent.click(choice().getByRole("radio", { name: sourceB.label }));
  const firstUse = choice().getByRole("button", { name: "Use source" });
  await act(async () => {
    fireEvent.click(firstUse);
    fireEvent.click(firstUse);
  });
  expect(choice().getByText("Manual source selection")).toBeTruthy();
  expect(revision).toBe(1);
  expect(chooseCount).toBe(1);
  expect(
    sourceReadCount,
    "choosing a source must not turn untouched availability into a requested check",
  ).toBe(0);
  expect(refreshCount).toBe(0);
  const firstReset = choice().getByRole("button", {
    name: "Return to automatic",
  });
  await act(async () => {
    fireEvent.click(firstReset);
    fireEvent.click(firstReset);
  });
  expect(choice().getByText("Automatic source selection")).toBeTruthy();
  expect(revision).toBe(2);
  expect(resetCount).toBe(1);
  expect(choiceReadCount).toBe(3);
  expect(
    sourceReadCount,
    "resetting a choice must leave untouched availability unrequested",
  ).toBe(0);
  expect(refreshCount).toBe(0);
  expect(
    availability().queryByRole("button", { name: "Play track" }),
  ).toBeNull();
  expect(
    availability().queryByRole("button", { name: "Prepare audio" }),
  ).toBeNull();
  const check = availability().getByRole("button", {
    name: "Check availability",
  });
  await act(async () => {
    fireEvent.click(check);
    fireEvent.click(check);
  });
  expect(refreshCount).toBe(1);
  expect(sourceReadCount).toBe(0);
  const refreshRequest = requests(refreshPath, "POST")[0][1];
  expectOwnedRequest(refreshRequest);
  expect(refreshRequest?.body).toBeUndefined();
  expect(availability().getByRole("alert").textContent).toBe(
    "Service temporarily busy.",
  );
  expect(
    availability().queryByRole("button", { name: "Play track" }),
  ).toBeNull();
  expect(
    availability().queryByRole("button", { name: "Prepare audio" }),
  ).toBeNull();
  expect(document.body.textContent).not.toContain(privateCanary);
  fireEvent.click(choice().getByRole("radio", { name: sourceB.label }));
  const secondUse = choice().getByRole("button", { name: "Use source" });
  await act(async () => {
    fireEvent.click(secondUse);
    fireEvent.click(secondUse);
  });
  expect(choice().getByText("Manual source selection")).toBeTruthy();
  expect(revision).toBe(3);
  expect(chooseCount).toBe(2);
  expect(choiceReadCount).toBe(4);
  expect(
    sourceReadCount,
    "a choice change after an explicit failed check must read current availability again",
  ).toBe(1);
  const firstRead = requests(sourcesPath, "GET")[0][1];
  expectOwnedRequest(firstRead);
  expect(firstRead?.body).toBeUndefined();
  expect(availability().getByText("Checking audio…")).toBeTruthy();
  expect(availability().queryByRole("alert")).toBeNull();
  expect(
    availability().queryByRole("button", { name: "Play track" }),
  ).toBeNull();
  expect(
    availability().queryByRole("button", { name: "Prepare audio" }),
  ).toBeNull();
  await act(async () =>
    firstAvailability.resolve(
      Response.json({ error: unavailableFailure }, { status: 503 }),
    ),
  );
  expect(availability().getByRole("alert").textContent).toBe(
    "Hito could not check audio availability. Try again.",
  );
  expect(
    availability().queryByRole("button", { name: "Play track" }),
  ).toBeNull();
  expect(
    availability().queryByRole("button", { name: "Prepare audio" }),
  ).toBeNull();
  expect(document.body.textContent).not.toContain(privateCanary);
  const afterFailure = fetch.mock.calls.length;
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
  });
  expect(fetch).toHaveBeenCalledTimes(afterFailure);
  const lastReset = choice().getByRole("button", {
    name: "Return to automatic",
  });
  await act(async () => {
    fireEvent.click(lastReset);
    fireEvent.click(lastReset);
  });
  expect(choice().getByText("Automatic source selection")).toBeTruthy();
  expect(revision).toBe(4);
  expect(resetCount).toBe(2);
  expect(choiceReadCount).toBe(5);
  expect(
    sourceReadCount,
    "failed choice-driven reads must remain requested for a later reset",
  ).toBe(2);
  const recoveredRead = requests(sourcesPath, "GET")[1][1];
  expectOwnedRequest(recoveredRead);
  expect(recoveredRead?.signal).not.toBe(firstRead?.signal);
  expect(recoveredRead?.body).toBeUndefined();
  expect(availability().getByText("Checking audio…")).toBeTruthy();
  expect(availability().queryByRole("alert")).toBeNull();
  expect(
    availability().queryByRole("button", { name: "Play track" }),
  ).toBeNull();
  expect(
    availability().queryByRole("button", { name: "Prepare audio" }),
  ).toBeNull();
  expect(refreshCount).toBe(1);
  await act(async () =>
    recoveredAvailability.resolve(Response.json(readySources)),
  );
  expect(availability().getByText("Audio is ready to play.")).toBeTruthy();
  expect(
    availability().getByRole("button", { name: "Play track" }),
  ).toHaveProperty("disabled", false);
  expect(
    availability().queryByRole("button", { name: "Prepare audio" }),
  ).toBeNull();
  expect(availability().queryByRole("alert")).toBeNull();
  expect(title).toBe(screen.getByRole("textbox", { name: "Track title" }));
  expect(title.value).toBe(draft);
  expect(downloadReadCount).toBe(5);
  expect(unexpected).toEqual([]);
  const choices = requests(choicePath, "POST");
  const resets = requests(resetPath, "POST");
  const operationIds: string[] = [];
  for (const [, options] of choices) {
    expectOwnedRequest(options);
    const body = JSON.parse(String(options?.body)) as Schema<"ChooseSource">;
    expect(body).toEqual({
      request_id: expect.stringMatching(uuid),
      source_id: sourceB.id,
    });
    operationIds.push(body.request_id);
  }
  for (const [index, [, options]] of resets.entries()) {
    expectOwnedRequest(options);
    const body = JSON.parse(
      String(options?.body),
    ) as Schema<"ResetSourceChoice">;
    expect(body).toEqual({
      request_id: expect.stringMatching(uuid),
      expected_revision: index === 0 ? 1 : 3,
    });
    operationIds.push(body.request_id);
  }
  expect(new Set(operationIds).size).toBe(4);
  expect(
    fetch.mock.calls
      .filter(([, options]) => options?.method === "POST")
      .map(([url]) => url),
  ).toEqual([choicePath, resetPath, refreshPath, choicePath, resetPath]);
  expect(
    fetch.mock.calls.every(
      ([url]) => !url.includes("/preparation") && !url.includes("/audio"),
    ),
  ).toBe(true);
  expect(requests(downloadPath, "POST")).toHaveLength(0);
  for (const [, options] of fetch.mock.calls) {
    expectOwnedRequest(options);
  }
  expect(play).not.toHaveBeenCalled();
  expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();
  expect(router.reload).not.toHaveBeenCalled();
  expect(router.visit).not.toHaveBeenCalled();
  const lastDownload = requests(downloadPath, "GET")[4][1];
  const beforeUnmount = fetch.mock.calls.length;
  view.unmount();
  expect(lastDownload?.signal?.aborted).toBe(true);
  expect(recoveredRead?.signal?.aborted).toBe(true);
  expect(firstRead?.signal?.aborted).toBe(true);
  expect(document.querySelectorAll("audio")).toHaveLength(0);
  await act(async () => {
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
