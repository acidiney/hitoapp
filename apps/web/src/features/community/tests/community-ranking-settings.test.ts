import { createElement } from "react";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import SettingsPage from "../../../pages/SettingsPage";
import type { components } from "../../../generated";

type Schema = components["schemas"];

vi.mock("@inertiajs/react", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@inertiajs/react")>();
  return { ...actual, usePage: () => ({ url: "/settings?section=privacy" }) };
});

test("Privacy saves source rankings independently and requires review after a stale revision", async () => {
  const errors = vi.spyOn(console, "error");
  const initial: Schema["CommunityPreferences"] = {
    revision: 3,
    playback_reporting: true,
    metadata_corrections: false,
  };
  let legacy = { ...initial };
  let rankings: Schema["CommunityRankingPreferences"] = {
    revision: 0,
    use_source_rankings: false,
  };
  const requests: {
    path: string;
    method: string;
    body: unknown;
    signal: AbortSignal;
  }[] = [];
  const rankWrites: Schema["UpdateCommunityRankingPreferences"][] = [];
  const legacyWrites: Schema["UpdateCommunityPreferences"][] = [];
  let release!: (response: Response) => void;
  const held = new Promise<Response>((resolve) => {
    release = resolve;
  });
  let holdSave = true;
  const fetch = vi.fn<typeof globalThis.fetch>(async (input, options) => {
    const path = String(input);
    const method = options?.method ?? "GET";
    const headers = new Headers(options?.headers);
    expect(headers.get("x-hito-account")).toBe("alice");
    expect(headers.get("x-hito-request")).toBe("1");
    expect(options?.credentials).toBe("same-origin");
    expect(options?.signal).toBeInstanceOf(AbortSignal);
    const body: unknown =
      options?.body === undefined
        ? undefined
        : JSON.parse(String(options.body));
    requests.push({
      path,
      method,
      body,
      signal: options!.signal as AbortSignal,
    });
    if (path === "/api/v1/community/preferences") {
      if (method === "GET") return Response.json(legacy);
      expect(method).toBe("PUT");
      const value = body as Schema["UpdateCommunityPreferences"];
      expect(value).toEqual({ ...initial, playback_reporting: false });
      legacyWrites.push(value);
      legacy = { ...value, revision: legacy.revision + 1 };
      return Response.json(legacy);
    }
    expect(path).toBe("/api/v1/community/ranking-preferences");
    if (method === "GET") return Response.json(rankings);
    expect(method).toBe("PUT");
    const value = body as Schema["UpdateCommunityRankingPreferences"];
    expect(Object.keys(value).sort()).toEqual([
      "revision",
      "use_source_rankings",
    ]);
    rankWrites.push(value);
    if (value.revision !== rankings.revision)
      return Response.json(
        {
          error: "Source ranking preference changed. Reload it before saving.",
        },
        { status: 409 },
      );
    if (holdSave) return held;
    rankings = { ...value, revision: rankings.revision + 1 };
    return Response.json(rankings);
  });
  vi.stubGlobal("fetch", fetch);
  const props: Schema["SettingsPageProps"] = {
    user: { id: "alice", username: "Alice", admin: true },
    errors: {},
    libraries: [],
    playlists: [],
    native_playback: false,
  };
  const view = render(createElement(SettingsPage, props));
  try {
    const panel = screen.getByRole("tabpanel", { name: "Privacy" });
    let ranking = (await within(panel).findByRole("checkbox", {
      name: "Use community source rankings",
    })) as HTMLInputElement;
    const reporting = within(panel).getByRole("checkbox", {
      name: "Playback reporting",
    }) as HTMLInputElement;
    const corrections = within(panel).getByRole("checkbox", {
      name: "Metadata corrections",
    }) as HTMLInputElement;
    const save = () =>
      within(panel).getByRole("button", {
        name: "Save source ranking preference",
      }) as HTMLButtonElement;
    const controls = () => within(save().closest("form")!);
    expect(ranking.checked).toBe(false);
    expect(reporting.checked).toBe(true);
    expect(corrections.checked).toBe(false);
    expect(save().disabled).toBe(true);
    expect(rankWrites).toHaveLength(0);
    expect(legacyWrites).toHaveLength(0);
    fireEvent.click(ranking);
    expect(ranking.checked).toBe(true);
    expect(rankings).toEqual({ revision: 0, use_source_rankings: false });
    expect(rankWrites).toHaveLength(0);
    fireEvent.click(controls().getByRole("button", { name: "Cancel changes" }));
    expect(ranking.checked).toBe(false);
    expect(save().disabled).toBe(true);

    fireEvent.click(ranking);
    act(() => {
      fireEvent.submit(save().closest("form")!);
      fireEvent.submit(save().closest("form")!);
    });
    await vi.waitFor(() =>
      expect(rankWrites).toEqual([{ revision: 0, use_source_rankings: true }]),
    );
    expect(ranking.disabled).toBe(true);
    expect(save().disabled).toBe(true);
    expect(rankings).toEqual({ revision: 0, use_source_rankings: false });
    expect(legacy).toEqual(initial);
    expect(legacyWrites).toHaveLength(0);
    rankings = { revision: 1, use_source_rankings: true };
    await act(async () => {
      release(Response.json(rankings));
      await held;
    });
    await screen.findByText("Source ranking preference saved.", {
      exact: true,
    });
    expect(ranking.disabled).toBe(false);
    expect(ranking.checked).toBe(true);
    expect(save().disabled).toBe(true);
    expect(legacy).toEqual(initial);

    holdSave = false;
    rankings = { revision: 2, use_source_rankings: true };
    fireEvent.click(ranking);
    fireEvent.click(save());
    await screen.findByText(
      "Source ranking preference changed. Reload it before saving.",
      { exact: true },
    );
    expect(ranking.checked).toBe(false);
    expect(rankWrites).toEqual([
      { revision: 0, use_source_rankings: true },
      { revision: 1, use_source_rankings: false },
    ]);
    expect(rankings).toEqual({ revision: 2, use_source_rankings: true });
    expect(
      screen.queryByText("Source ranking preference saved.", { exact: true }),
    ).toBeNull();
    fireEvent.click(
      within(panel).getByRole("button", {
        name: "Reload source ranking preference",
      }),
    );
    await vi.waitFor(() =>
      expect(
        (
          within(panel).getByRole("checkbox", {
            name: "Use community source rankings",
          }) as HTMLInputElement
        ).checked,
      ).toBe(true),
    );
    expect(ranking.isConnected).toBe(false);
    ranking = within(panel).getByRole("checkbox", {
      name: "Use community source rankings",
    }) as HTMLInputElement;
    expect(save().disabled).toBe(true);
    expect(
      screen.queryByText(
        "Source ranking preference changed. Reload it before saving.",
        { exact: true },
      ),
    ).toBeNull();
    fireEvent.click(ranking);
    fireEvent.click(save());
    await screen.findByText("Source ranking preference saved.", {
      exact: true,
    });
    expect(rankWrites[2]).toEqual({ revision: 2, use_source_rankings: false });
    expect(rankings).toEqual({ revision: 3, use_source_rankings: false });
    expect(legacy).toEqual(initial);
    expect(legacyWrites).toHaveLength(0);

    fireEvent.click(reporting);
    fireEvent.click(
      within(panel).getByRole("button", {
        name: "Save community preferences",
      }),
    );
    await screen.findByText("Community preferences saved.", { exact: true });
    expect(legacyWrites).toEqual([{ ...initial, playback_reporting: false }]);
    expect(rankings).toEqual({ revision: 3, use_source_rankings: false });
    expect(corrections.checked).toBe(false);
    expect(
      requests.every((request) =>
        [
          "/api/v1/community/preferences",
          "/api/v1/community/ranking-preferences",
        ].includes(request.path),
      ),
    ).toBe(true);
    view.unmount();
    expect(requests.every((request) => request.signal.aborted)).toBe(true);
    expect(errors).not.toHaveBeenCalled();
  } finally {
    view.unmount();
    release(Response.json(rankings));
  }
});
