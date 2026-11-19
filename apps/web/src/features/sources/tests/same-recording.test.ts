import { createElement, type ReactNode } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import type { Schema } from "../../../shared/schema";
import SourceChoiceControls from "../components/SourceChoiceControls";
import { communityApi } from "../../community/api/communityApi";
import { sameRecordingApi } from "../api/sameRecordingApi";

vi.mock("@inertiajs/react", () => ({
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));
vi.mock("../../community/api/communityApi", () => ({
  communityApi: { preferences: vi.fn() },
}));
vi.mock("../api/sameRecordingApi", () => ({
  sameRecordingApi: { suggest: vi.fn() },
}));

function source(
  id: string,
  service: string,
  community_public: boolean,
): Schema<"Source"> {
  return {
    id,
    recording_id: "recording-1",
    provider: "plugin",
    label: `${service} audio`,
    provider_label: service,
    community_public,
    device_id: null,
    ready: true,
    can_prepare: false,
    match_kind: "verified",
  };
}

function controls(sources: Schema<"Source">[]) {
  return createElement(SourceChoiceControls, {
    account: "account-1",
    choice: {
      mode: "automatic",
      recording_id: "recording-1",
      revision: 1,
      sources: sources.map((entry) => ({
        source: entry,
        selected: false,
        selectable: true,
      })),
    },
    stagedSource: null,
    checking: false,
    pending: false,
    retrying: false,
    unavailable: false,
    error: "",
    statusError: "",
    blocked: false,
    canChoose: false,
    canReset: false,
    stage: vi.fn(),
    choose: vi.fn(async () => {}),
    reset: vi.fn(async () => {}),
    retry: vi.fn(async () => {}),
    check: vi.fn(async () => {}),
    fixSource: vi.fn(),
  });
}

const youtube = source("source-youtube", "YouTube", true);
const jamendo = source("source-jamendo", "Jamendo", true);
const privateService = source("source-private", "Private service", false);

function consent(on: boolean) {
  vi.mocked(communityApi.preferences).mockResolvedValue({
    revision: 1,
    playback_reporting: false,
    metadata_corrections: on,
  });
}

beforeEach(() => {
  vi.mocked(communityApi.preferences).mockReset();
  vi.mocked(sameRecordingApi.suggest).mockReset();
});

async function openSuggestion(service: string) {
  fireEvent.click(
    await screen.findByRole("button", { name: `More for ${service}` }),
  );
  fireEvent.click(
    await screen.findByRole("menuitem", { name: "Suggest as same recording" }),
  );
}

test("the action is offered only when two or more public sources are listed", async () => {
  consent(true);
  const { unmount } = render(controls([youtube, privateService]));
  expect(screen.queryByRole("button", { name: /^More for/ })).toBeNull();
  expect(communityApi.preferences).not.toHaveBeenCalled();
  unmount();
  render(controls([youtube, jamendo, privateService]));
  expect(
    await screen.findByRole("button", { name: "More for YouTube" }),
  ).toBeTruthy();
  expect(screen.getByRole("button", { name: "More for Jamendo" })).toBeTruthy();
  expect(
    screen.queryByRole("button", { name: "More for Private service" }),
  ).toBeNull();
});

test("without metadata corrections the action is disabled and points to Community settings", async () => {
  consent(false);
  render(controls([youtube, jamendo]));
  const link = await screen.findByRole("link", { name: "Community settings" });
  expect(link.getAttribute("href")).toBe("/settings?section=privacy");
  fireEvent.click(screen.getByRole("button", { name: "More for YouTube" }));
  const item = await screen.findByRole("menuitem", {
    name: "Suggest as same recording",
  });
  expect(
    item.hasAttribute("disabled") ||
      item.getAttribute("aria-disabled") === "true",
  ).toBe(true);
  expect(sameRecordingApi.suggest).not.toHaveBeenCalled();
});

test("confirming sends one v4 submission for the chosen pair and marks the row", async () => {
  consent(true);
  vi.mocked(sameRecordingApi.suggest).mockResolvedValue("queued");
  render(controls([youtube, jamendo]));
  await openSuggestion("YouTube");
  const dialog = await screen.findByRole("dialog", {
    name: "Suggest as same recording",
  });
  expect(dialog.textContent).toContain(
    "The suggestion goes to the shared community service for review.",
  );
  fireEvent.click(screen.getByRole("button", { name: "Send suggestion" }));
  expect(await screen.findByText("Suggestion sent for review")).toBeTruthy();
  expect(sameRecordingApi.suggest).toHaveBeenCalledTimes(1);
  const [account, from, body, signal] = vi.mocked(sameRecordingApi.suggest).mock
    .calls[0];
  expect(account).toBe("account-1");
  expect(from).toBe("source-youtube");
  expect(body.other_source_id).toBe("source-jamendo");
  expect(body.submission_id).toMatch(
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
  );
  expect(signal).toBeInstanceOf(AbortSignal);
});

test("a failure shows the error and a retry sends a new submission", async () => {
  consent(true);
  vi.mocked(sameRecordingApi.suggest)
    .mockRejectedValueOnce(new Error("The community service is unavailable"))
    .mockResolvedValueOnce("queued");
  render(controls([youtube, jamendo]));
  await openSuggestion("YouTube");
  fireEvent.click(
    await screen.findByRole("button", { name: "Send suggestion" }),
  );
  expect((await screen.findByRole("alert")).textContent).toBe(
    "The community service is unavailable",
  );
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  expect(await screen.findByText("Suggestion sent for review")).toBeTruthy();
  const calls = vi.mocked(sameRecordingApi.suggest).mock.calls;
  expect(calls).toHaveLength(2);
  expect(calls[1][2].other_source_id).toBe("source-jamendo");
  expect(calls[1][2].submission_id).not.toBe(calls[0][2].submission_id);
});

test("cancelling a suggestion in flight aborts the request and leaves no status", async () => {
  consent(true);
  let signal: AbortSignal | undefined;
  vi.mocked(sameRecordingApi.suggest).mockImplementation(
    (_account, _from, _body, given) => {
      signal = given;
      return new Promise(() => {});
    },
  );
  render(controls([youtube, jamendo]));
  await openSuggestion("Jamendo");
  fireEvent.click(
    await screen.findByRole("button", { name: "Send suggestion" }),
  );
  expect(await screen.findByText(/Sending suggestion…/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(signal?.aborted).toBe(true));
  expect(screen.queryByText(/Sending suggestion…/)).toBeNull();
  expect(screen.queryByText("Suggestion sent for review")).toBeNull();
});
