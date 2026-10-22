import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import RecordingDetailsPage from "../../../pages/RecordingDetailsPage";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { PlaybackProvider } from "../../../player/PlaybackProvider";
import { attachedPage } from "../../../fixtures/sourceAttachment";
import { recordingA, recordingB } from "../../../fixtures/playlistPages";
import type {
  DownloadReceipt,
  DownloadRequest,
  DownloadStatus,
  RecordingDetailsPageProps,
} from "../../../shared/types";
import { heldResponse } from "../../../test/async";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

function props(
  account: string,
  recording: string,
  title: string,
): RecordingDetailsPageProps {
  const observation = { ...attachedPage.detail.selected_observation!, title };
  return {
    ...attachedPage,
    user: { id: account, username: account, admin: false },
    detail: {
      ...attachedPage.detail,
      recording_id: recording,
      title: {
        ...attachedPage.detail.title,
        value: title,
        inherited: title,
        personal: title,
      },
      selected_observation: observation,
      observations: { items: [observation], next_before: null },
    },
  };
}

function page(props: RecordingDetailsPageProps) {
  return createElement(
    NoticeProvider,
    null,
    createElement(PlaybackProvider, {
      account: props.user.id,
      children: createElement(RecordingDetailsPage, props),
    }),
  );
}

test.each(["account", "recording"] as const)(
  "held download requests cannot cross a %s change and only the current owner polls",
  async (firstChange) => {
    vi.useFakeTimers();
    vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
    vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
    const play = vi
      .spyOn(HTMLMediaElement.prototype, "play")
      .mockResolvedValue();
    const first = props("alice", recordingA, "First owned recording");
    const middle =
      firstChange === "account"
        ? props("bob", recordingA, "Second owned recording")
        : props("alice", recordingB, "Second owned recording");
    const current = props("bob", recordingB, "Current owned recording");
    const sourceChoicePaths = [recordingA, recordingB].map(
      (recording) => `/api/v1/recordings/${recording}/source-choice`,
    );
    const downloadPaths = [recordingA, recordingB].map(
      (recording) => `/api/v1/recordings/${recording}/download`,
    );
    const oldRead = heldResponse();
    const oldWrite = heldResponse();
    const currentRead = heldResponse();
    const running: DownloadStatus = {
      status: "running",
      intent_id: "0179f572-d7b4-4cdb-9b20-640598546657",
      recording_id: current.detail.recording_id,
      source_id: null,
      host_device_id: "owned-download-host",
      choice_revision: 0,
      matches_source_choice: true,
      bytes_received: 128,
      total_bytes: 256,
      error: null,
    };
    const owned = (
      page: RecordingDetailsPageProps,
      account: string | null,
      url: string,
    ) =>
      account === page.user.id &&
      url === `/api/v1/recordings/${page.detail.recording_id}/download`;
    let currentReads = 0;
    const fetch = vi.fn((url: string, options?: RequestInit) => {
      const account = new Headers(options?.headers).get("x-hito-account");
      if (options?.method === "GET") {
        if (sourceChoicePaths.includes(url))
          return Promise.resolve(
            Response.json({ error: "Not found" }, { status: 404 }),
          );
        if (owned(first, account, url)) return oldRead.response;
        if (owned(middle, account, url))
          return Promise.resolve(
            Response.json({ error: "Not found" }, { status: 404 }),
          );
        if (owned(current, account, url)) {
          currentReads++;
          return currentReads === 1
            ? Promise.resolve(Response.json(running))
            : currentRead.response;
        }
      }
      if (options?.method === "POST" && owned(middle, account, url))
        return oldWrite.response;
      throw new Error("Unexpected request outside download ownership checks");
    });
    const choiceReads = () =>
      fetch.mock.calls.filter(
        ([url, options]) =>
          sourceChoicePaths.includes(url) && options?.method === "GET",
      );
    const downloadRequests = () =>
      fetch.mock.calls.filter(([url]) => downloadPaths.includes(url));
    vi.stubGlobal("fetch", fetch);
    const view = render(page(first));
    expect(downloadRequests()).toHaveLength(1);
    expect(choiceReads()).toHaveLength(1);
    expect(choiceReads()[0][1]?.headers).toMatchObject({
      "x-hito-account": first.user.id,
    });
    expect(choiceReads()[0][1]?.signal?.aborted).toBe(false);
    const oldReadSignal = downloadRequests()[0][1]?.signal;
    expect(oldReadSignal?.aborted).toBe(false);
    view.rerender(page(middle));
    expect(oldReadSignal?.aborted).toBe(true);
    expect(choiceReads()[0][1]?.signal?.aborted).toBe(true);
    expect(choiceReads()).toHaveLength(2);
    expect(choiceReads()[1][1]?.headers).toMatchObject({
      "x-hito-account": middle.user.id,
    });
    expect(choiceReads()[1][1]?.signal?.aborted).toBe(false);
    await act(async () => {});
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Download" }));
    });
    const posted = fetch.mock.calls.find(
      ([, options]) => options?.method === "POST",
    )!;
    const request: DownloadRequest = JSON.parse(String(posted[1]?.body));
    const oldWriteSignal = posted[1]?.signal;
    expect(oldWriteSignal?.aborted).toBe(false);
    view.rerender(page(current));
    expect(oldWriteSignal?.aborted).toBe(true);
    expect(choiceReads()[1][1]?.signal?.aborted).toBe(true);
    expect(choiceReads()).toHaveLength(3);
    expect(choiceReads()[2][1]?.headers).toMatchObject({
      "x-hito-account": current.user.id,
    });
    expect(choiceReads()[2][1]?.signal?.aborted).toBe(false);
    await act(async () => {});
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe(
      current.detail.title.value,
    );
    const progress = screen.getByRole("progressbar", {
      name: "Download progress",
    });
    expect(progress.getAttribute("value")).toBe("128");
    expect(screen.getByText("Downloading audio…")).toBeTruthy();
    const beforeLate = fetch.mock.calls.length;
    await act(async () => {
      oldRead.resolve(
        Response.json({
          ...running,
          recording_id: first.detail.recording_id,
          intent_id: "00450af1-f567-42c3-9a1f-3b6ae469ba51",
          source_id: "6e6b0cee-92a3-4198-b4cf-9473e91c5ca2",
          status: "ready",
          bytes_received: 256,
        } satisfies DownloadStatus),
      );
      oldWrite.resolve(
        Response.json(
          {
            request_id: request.request_id,
            recording_id: middle.detail.recording_id,
            intent_id: "1e9a88a0-ed61-48c2-82ed-31f96329c8f0",
          } satisfies DownloadReceipt,
          { status: 202 },
        ),
      );
    });
    expect(fetch).toHaveBeenCalledTimes(beforeLate);
    expect(screen.queryByText("Saved on this server")).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Retry download request" }),
    ).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByRole("progressbar", { name: "Download progress" })).toBe(
      progress,
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(currentReads).toBe(2);
    const poll = downloadRequests()[downloadRequests().length - 1];
    expect(poll[0]).toBe(
      `/api/v1/recordings/${current.detail.recording_id}/download`,
    );
    expect(poll[1]?.headers).toMatchObject({
      "x-hito-account": current.user.id,
    });
    expect(poll[1]?.signal?.aborted).toBe(false);
    expect(screen.getByRole("progressbar", { name: "Download progress" })).toBe(
      progress,
    );
    const beforeFocus = fetch.mock.calls.length;
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
    });
    expect(fetch).toHaveBeenCalledTimes(beforeFocus);
    expect(
      fetch.mock.calls.filter(([, options]) => options?.method === "POST"),
    ).toHaveLength(1);
    expect(play).not.toHaveBeenCalled();
    expect(
      choiceReads().map(([url, options]) => [
        url,
        new Headers(options?.headers).get("x-hito-account"),
      ]),
    ).toEqual(
      [first, middle, current].map((owner) => [
        `/api/v1/recordings/${owner.detail.recording_id}/source-choice`,
        owner.user.id,
      ]),
    );
    for (const [, options] of choiceReads()) {
      expect(options?.credentials).toBe("same-origin");
      expect(options?.signal).toBeInstanceOf(AbortSignal);
      expect(options?.body).toBeUndefined();
    }
    view.unmount();
    expect(choiceReads().every(([, options]) => options?.signal?.aborted)).toBe(
      true,
    );
    expect(poll[1]?.signal?.aborted).toBe(true);
    expect(vi.getTimerCount()).toBe(0);
    await act(async () => {
      currentRead.resolve(
        Response.json({
          ...running,
          bytes_received: 256,
          status: "ready",
          source_id: "4d39da6f-4c7c-4b10-9851-ccfd5f85d69d",
        } satisfies DownloadStatus),
      );
      await vi.advanceTimersByTimeAsync(60_000);
      window.dispatchEvent(new Event("focus"));
    });
    expect(fetch).toHaveBeenCalledTimes(beforeFocus);
    expect(vi.getTimerCount()).toBe(0);
  },
);
