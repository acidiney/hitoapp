import { createElement } from "react";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import CommunitySettings from "./components/CommunitySettings";
import type { components } from "./generated";

type Schema = components["schemas"];
type Preferences = Schema["CommunityPreferences"];

test("community consent requires an explicit independent save and retires cancelled or foreign account work", async () => {
  const disabled: Preferences = {
    revision: 0,
    playback_reporting: false,
    metadata_corrections: false,
  };
  let releaseRead!: (response: Response) => void;
  const retiredRead = new Promise<Response>((resolve) => {
    releaseRead = resolve;
  });
  let releaseSave!: (response: Response) => void;
  const retiredSave = new Promise<Response>((resolve) => {
    releaseSave = resolve;
  });
  const fetch = vi.fn<typeof globalThis.fetch>();
  fetch
    .mockReturnValueOnce(retiredRead)
    .mockResolvedValueOnce(Response.json(disabled));
  fetch.mockImplementation(() => {
    throw new Error(
      "Unexpected request outside the community consent workflow",
    );
  });
  vi.stubGlobal("fetch", fetch);
  const view = render(createElement(CommunitySettings, { account: "alice" }));
  try {
    await waitFor(() => expect(fetch).toHaveBeenCalledTimes(1));
    const oldRead = fetch.mock.calls[0][1]?.signal;
    view.rerender(createElement(CommunitySettings, { account: "bob" }));
    expect(oldRead?.aborted).toBe(true);
    await screen.findByRole("checkbox", { name: "Playback reporting" });
    await act(async () => {
      releaseRead(
        Response.json({
          ...disabled,
          revision: 8,
          playback_reporting: true,
          metadata_corrections: true,
        } satisfies Preferences),
      );
      await retiredRead;
    });
    expect(playback().checked).toBe(false);
    expect(corrections().checked).toBe(false);
    expect(save().disabled).toBe(true);
    expect(screen.queryByRole("textbox")).toBeNull();
    expect(document.querySelector('input[type="password"]')).toBeNull();

    fireEvent.click(playback());
    expect(playback().checked).toBe(true);
    expect(corrections().checked).toBe(false);
    expect(fetch).toHaveBeenCalledTimes(2);
    fireEvent.click(screen.getByRole("button", { name: "Cancel changes" }));
    expect(playback().checked).toBe(false);
    expect(corrections().checked).toBe(false);
    expect(save().disabled).toBe(true);
    expect(fetch).toHaveBeenCalledTimes(2);

    const reporting: Preferences = {
      ...disabled,
      revision: 1,
      playback_reporting: true,
    };
    fetch.mockResolvedValueOnce(Response.json(reporting));
    fireEvent.click(playback());
    const form = save().closest("form");
    expect(form).not.toBeNull();
    act(() => {
      fireEvent.submit(form!);
      fireEvent.submit(form!);
    });
    await screen.findByText("Community preferences saved.", { exact: true });
    expect(fetch).toHaveBeenCalledTimes(3);
    expect(fetch.mock.calls[2][0]).toBe("/api/v1/community/preferences");
    expect(fetch.mock.calls[2][1]).toMatchObject({
      method: "PUT",
      credentials: "same-origin",
      headers: { "x-hito-account": "bob", "x-hito-request": "1" },
      signal: expect.any(AbortSignal),
    });
    const request: Schema["UpdateCommunityPreferences"] = JSON.parse(
      String(fetch.mock.calls[2][1]?.body),
    );
    expect(request).toEqual({ ...disabled, playback_reporting: true });
    expect(playback().checked).toBe(true);
    expect(corrections().checked).toBe(false);
    expect(save().disabled).toBe(true);

    fetch.mockResolvedValueOnce(
      Response.json(
        { error: "Community preferences changed. Review them again." },
        { status: 409 },
      ),
    );
    fireEvent.click(corrections());
    expect(fetch).toHaveBeenCalledTimes(3);
    fireEvent.click(save());
    expect((await screen.findByRole("alert")).textContent).toContain(
      "Community preferences changed. Review them again.",
    );
    expect(playback().checked).toBe(true);
    expect(corrections().checked).toBe(true);
    expect(fetch).toHaveBeenCalledTimes(4);
    expect(
      screen.queryByText("Community preferences saved.", { exact: true }),
    ).toBeNull();

    const changed: Preferences = {
      ...disabled,
      revision: 2,
      metadata_corrections: true,
    };
    fetch.mockResolvedValueOnce(Response.json(changed));
    fireEvent.click(
      screen.getByRole("button", { name: "Reload community preferences" }),
    );
    await waitFor(() => {
      expect(fetch).toHaveBeenCalledTimes(5);
      expect(playback().checked).toBe(false);
    });
    expect(corrections().checked).toBe(true);
    expect(save().disabled).toBe(true);
    expect(screen.queryByRole("alert")).toBeNull();

    fetch
      .mockReturnValueOnce(retiredSave)
      .mockResolvedValueOnce(Response.json(disabled));
    fireEvent.click(playback());
    fireEvent.click(save());
    await waitFor(() => expect(fetch).toHaveBeenCalledTimes(6));
    const oldSave = fetch.mock.calls[5][1]?.signal;
    expect(JSON.parse(String(fetch.mock.calls[5][1]?.body))).toEqual({
      ...changed,
      playback_reporting: true,
    });
    view.rerender(createElement(CommunitySettings, { account: "carol" }));
    expect(oldSave?.aborted).toBe(true);
    await screen.findByRole("checkbox", { name: "Playback reporting" });
    await act(async () => {
      releaseSave(
        Response.json({
          ...changed,
          revision: 3,
          playback_reporting: true,
        } satisfies Preferences),
      );
      await retiredSave;
    });
    expect(playback().checked).toBe(false);
    expect(corrections().checked).toBe(false);
    expect(save().disabled).toBe(true);
    expect(
      screen.queryByText("Community preferences saved.", { exact: true }),
    ).toBeNull();
    expect(fetch).toHaveBeenCalledTimes(7);
    expect(
      fetch.mock.calls.every(
        ([path]) => path === "/api/v1/community/preferences",
      ),
    ).toBe(true);
    expect(
      fetch.mock.calls.map(([, options]) =>
        new Headers(options?.headers).get("x-hito-account"),
      ),
    ).toEqual(["alice", "bob", "bob", "bob", "bob", "bob", "carol"]);
    const currentRead = fetch.mock.calls[6][1]?.signal;
    view.unmount();
    expect(currentRead?.aborted).toBe(true);
  } finally {
    view.unmount();
    releaseRead(Response.json(disabled));
    releaseSave(Response.json(disabled));
    await act(async () => {
      await Promise.all([retiredRead, retiredSave]);
    });
  }
});

function playback() {
  return screen.getByRole("checkbox", {
    name: "Playback reporting",
  }) as HTMLInputElement;
}

function corrections() {
  return screen.getByRole("checkbox", {
    name: "Metadata corrections",
  }) as HTMLInputElement;
}

function save() {
  return screen.getByRole("button", {
    name: "Save community preferences",
  }) as HTMLButtonElement;
}

test("community credential recovery is explicit, preserves consent drafts, and retires account work", async () => {
  const preferences: Preferences = {
    revision: 3,
    playback_reporting: true,
    metadata_corrections: false,
  };
  let releaseReset!: (response: Response) => void;
  const retiredReset = new Promise<Response>((resolve) => {
    releaseReset = resolve;
  });
  const fetch = vi.fn<typeof globalThis.fetch>();
  fetch.mockResolvedValueOnce(Response.json(preferences));
  fetch.mockImplementation(() => {
    throw new Error("Unexpected request outside explicit community recovery");
  });
  vi.stubGlobal("fetch", fetch);
  const view = render(createElement(CommunitySettings, { account: "alice" }));
  try {
    await screen.findByRole("checkbox", { name: "Playback reporting" });
    const reset = () =>
      screen.getByRole("button", {
        name: "Reset community credentials",
      }) as HTMLButtonElement;
    expect(reset().disabled).toBe(false);
    expect(screen.getByText(/cancel unsent reports/i).textContent).toContain(
      "accepted contributions",
    );
    expect(fetch).toHaveBeenCalledTimes(1);
    fireEvent.click(corrections());
    expect(fetch).toHaveBeenCalledTimes(1);

    fetch.mockResolvedValueOnce(
      Response.json(
        { error: "Community reset could not finish. Try again." },
        { status: 502 },
      ),
    );
    fireEvent.click(reset());
    expect((await screen.findByRole("alert")).textContent).toContain(
      "Community reset could not finish. Try again.",
    );
    expect(playback().checked).toBe(true);
    expect(corrections().checked).toBe(true);
    expect(save().disabled).toBe(false);
    expect(fetch).toHaveBeenCalledTimes(2);
    expect(fetch.mock.calls[1][0]).toBe("/api/v1/community/credentials");
    expect(fetch.mock.calls[1][1]).toMatchObject({
      method: "DELETE",
      credentials: "same-origin",
      headers: { "x-hito-account": "alice", "x-hito-request": "1" },
      signal: expect.any(AbortSignal),
    });
    expect(fetch.mock.calls[1][1]?.body).toBeUndefined();

    fetch.mockResolvedValueOnce(
      Response.json({ reporters_removed: 2, reports_cancelled: 3 }),
    );
    act(() => {
      fireEvent.click(reset());
      fireEvent.click(reset());
    });
    await screen.findByText(
      "Community credentials reset. Privacy choices and accepted contributions are kept. Unsent reports canceled: 3.",
    );
    expect(screen.queryByRole("alert")).toBeNull();
    expect(playback().checked).toBe(true);
    expect(corrections().checked).toBe(true);
    expect(save().disabled).toBe(false);
    expect(fetch).toHaveBeenCalledTimes(3);

    fetch
      .mockReturnValueOnce(retiredReset)
      .mockResolvedValueOnce(
        Response.json({ ...preferences, playback_reporting: false }),
      );
    fireEvent.click(reset());
    await waitFor(() => expect(fetch).toHaveBeenCalledTimes(4));
    const oldReset = fetch.mock.calls[3][1]?.signal;
    view.rerender(createElement(CommunitySettings, { account: "bob" }));
    expect(oldReset?.aborted).toBe(true);
    await screen.findByRole("checkbox", { name: "Playback reporting" });
    await act(async () => {
      releaseReset(
        Response.json({ reporters_removed: 9, reports_cancelled: 99 }),
      );
      await retiredReset;
    });
    expect(playback().checked).toBe(false);
    expect(corrections().checked).toBe(false);
    expect(screen.queryByText(/Community credentials reset/)).toBeNull();
    expect(fetch).toHaveBeenCalledTimes(5);
    expect(
      fetch.mock.calls.map(([, options]) =>
        new Headers(options?.headers).get("x-hito-account"),
      ),
    ).toEqual(["alice", "alice", "alice", "alice", "bob"]);
    fetch.mockReturnValueOnce(new Promise<Response>(() => {}));
    fireEvent.click(reset());
    await waitFor(() => expect(fetch).toHaveBeenCalledTimes(6));
    const currentReset = fetch.mock.calls[5][1]?.signal;
    view.unmount();
    expect(currentReset?.aborted).toBe(true);
  } finally {
    view.unmount();
    releaseReset(Response.json({ reporters_removed: 0, reports_cancelled: 0 }));
    await act(async () => {
      await retiredReset;
    });
  }
});
