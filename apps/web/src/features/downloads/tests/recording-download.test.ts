import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import RecordingDetailsPage from "../../../pages/RecordingDetailsPage";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { PlaybackProvider } from "../../../player/PlaybackProvider";
import { attachedPage, savedRecording } from "../../../fixtures/sourceAttachment";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

test("a track downloads only on request and removes its saved server copy by intent without starting playback", async () => {
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const path = `/api/v1/recordings/${savedRecording.id}/download`;
  const sourceChoice = `/api/v1/recordings/${savedRecording.id}/source-choice`;
  const intent = "70baaa3a-6ec3-47ad-9f93-bda782f31fb7";
  const removePath = `/api/v1/downloads/${intent}`;
  let saved = false;
  let finishCreate!: (response: Response) => void;
  let finishRemove!: (response: Response) => void;
  const fetch = vi.fn(async (url: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (url === sourceChoice && method === "GET")
      return Response.json({ error: "Not found" }, { status: 404 });
    if (url === path && method === "GET")
      return saved
        ? Response.json({
            status: "ready",
            intent_id: intent,
            recording_id: savedRecording.id,
            source_id: "ad034b36-2274-4e56-bef5-9b63e3bb2055",
            host_device_id: "owned-server-host",
            choice_revision: 0,
            matches_source_choice: true,
            bytes_received: 16_044,
            total_bytes: 16_044,
            error: null,
          })
        : Response.json({ error: "Not found" }, { status: 404 });
    if (url === path && method === "POST")
      return new Promise<Response>((resolve) => {
        finishCreate = (response) => {
          saved = true;
          resolve(response);
        };
      });
    if (url === removePath && method === "DELETE")
      return new Promise<Response>((resolve) => {
        finishRemove = (response) => {
          saved = false;
          resolve(response);
        };
      });
    throw new Error(
      "Unexpected request outside the explicit download workflow",
    );
  });
  vi.stubGlobal("fetch", fetch);
  const { unmount } = render(
    createElement(
      NoticeProvider,
      null,
      createElement(PlaybackProvider, {
        account: attachedPage.user.id,
        children: createElement(RecordingDetailsPage, attachedPage),
      }),
    ),
  );
  const download = screen.getByRole("button", {
    name: "Download",
  });
  const title = screen.getByRole("textbox", {
    name: "Track title",
  }) as HTMLInputElement;
  fireEvent.change(title, { target: { value: "Unsent personal title" } });
  await waitFor(() =>
    expect(
      fetch.mock.calls.some(
        ([url, options]) => url === path && options?.method === "GET",
      ),
    ).toBe(true),
  );
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
  expect(
    fetch.mock.calls.every(([, options]) => options?.method === "GET"),
  ).toBe(true);
  expect(play).not.toHaveBeenCalled();
  expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();

  act(() => {
    fireEvent.click(download);
    fireEvent.click(download);
  });
  await waitFor(() =>
    expect(
      fetch.mock.calls.filter(([, options]) => options?.method === "POST"),
    ).toHaveLength(1),
  );
  const creation = fetch.mock.calls.find(
    ([, options]) => options?.method === "POST",
  )!;
  expect(creation[0]).toBe(path);
  expect(creation[1]).toMatchObject({
    credentials: "same-origin",
    headers: { "x-hito-account": attachedPage.user.id, "x-hito-request": "1" },
    signal: expect.any(AbortSignal),
  });
  const request = JSON.parse(String(creation[1]?.body));
  expect(request).toEqual({
    request_id: expect.stringMatching(
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
    ),
  });
  expect(creation[1]?.signal?.aborted).toBe(false);
  expect((download as HTMLButtonElement).disabled).toBe(true);
  expect(screen.queryByText("Saved on this server")).toBeNull();
  await act(async () =>
    finishCreate(
      Response.json(
        {
          request_id: request.request_id,
          intent_id: intent,
          recording_id: savedRecording.id,
        },
        { status: 202 },
      ),
    ),
  );
  await screen.findByText("Saved on this server");
  expect(title.value).toBe("Unsent personal title");
  expect(play).not.toHaveBeenCalled();
  expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();

  const remove = screen.getByRole("button", {
    name: "Remove download",
  });
  act(() => {
    fireEvent.click(remove);
    fireEvent.click(remove);
  });
  await waitFor(() =>
    expect(
      fetch.mock.calls.filter(([, options]) => options?.method === "DELETE"),
    ).toHaveLength(1),
  );
  const removal = fetch.mock.calls.find(
    ([, options]) => options?.method === "DELETE",
  )!;
  expect(removal[0]).toBe(removePath);
  expect(removal[1]).toMatchObject({
    credentials: "same-origin",
    headers: { "x-hito-account": attachedPage.user.id, "x-hito-request": "1" },
    signal: expect.any(AbortSignal),
  });
  expect(removal[1]?.body).toBeUndefined();
  expect(removal[1]?.signal?.aborted).toBe(false);
  expect((remove as HTMLButtonElement).disabled).toBe(true);
  await act(async () => finishRemove(new Response(null, { status: 204 })));
  await screen.findByRole("button", { name: "Download" });
  expect(screen.queryByText("Saved on this server")).toBeNull();
  expect(title.value).toBe("Unsent personal title");
  expect(
    fetch.mock.calls.filter(([, options]) => options?.method !== "GET"),
  ).toHaveLength(2);
  expect(play).not.toHaveBeenCalled();
  expect(
    fetch.mock.calls.filter(
      ([url, options]) => url === sourceChoice && options?.method === "GET",
    ),
  ).toHaveLength(1);
  unmount();
  expect(choiceReads[0][1]?.signal?.aborted).toBe(true);
  expect(document.querySelectorAll("audio")).toHaveLength(0);
});
