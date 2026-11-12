import { createElement } from "react";
import {
  act,
  cleanup,
  render,
  renderHook,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { HttpError } from "../../../shared/http";
import { invalidate } from "../../../shared/invalidation";
import type { Schema } from "../../../shared/schema";
import { forgetAutomaticLookups } from "../../lyrics/api/automaticLyrics";
import {
  connectionNeedsCredentials,
  resetCredentialRequirements,
  useCredentialRequirements,
} from "../../plugins/model/credentialRequirements";
import { LyricsPanel } from "../components/NowPlayingLyrics";

const apis = vi.hoisted(() => ({
  identities: vi.fn(),
  read: vi.fn(),
  services: vi.fn(),
  automatic: vi.fn(),
}));
vi.mock("@inertiajs/react", () => ({ router: { visit: vi.fn() } }));
vi.mock("../../lyrics/api/lyricsApi", () => ({
  lyricsApi: {
    identities: apis.identities,
    readSelected: apis.read,
    services: apis.services,
    automatic: apis.automatic,
  },
}));

const message = "This music service connection needs its credentials updated.";
const stamp = {
  installation_id: "inst-a",
  installation_revision: 4,
} satisfies Schema<"PluginInstallationStamp">;
const body = {
  error: message,
  code: "plugin_credentials_required",
  plugin_connection: stamp,
} satisfies Schema<"ErrorResponse">;
const credentialError = () =>
  new HttpError(409, body.error, body.code, undefined, undefined, body);

const track = {
  id: "r1",
  title: "Song",
  artist: "Shore",
  album: null,
  duration_ms: 1000,
  available: true,
};
const identity = {
  selectable: true,
  selector: { kind: "observation", observation_id: "obs-a" },
  installation_id: stamp.installation_id,
  installation_revision: stamp.installation_revision,
  label: "Lyrics A",
  provider_recording: { id: "rec-a", revision: null },
} satisfies Schema<"RecordingLyricIdentity">;
const ready = {
  status: "ready_v1",
  recording_id: "r1",
  selector: identity.selector,
  provider_recording: identity.provider_recording,
  document_digest: "d",
  document: {
    recording: identity.provider_recording,
    lyrics: {
      id: "l",
      rendition_id: "x",
      rendition_label: null,
      lines: [{ start_ms: 0, end_ms: 1, text: "Back again" }],
      attribution: null,
    },
  },
};
const open = () =>
  render(
    createElement(LyricsPanel, {
      account: "alice",
      track: track as never,
      leave: vi.fn(),
    }),
  );

beforeEach(() => {
  vi.clearAllMocks();
  forgetAutomaticLookups();
  resetCredentialRequirements();
  apis.identities.mockResolvedValue({ recording_id: "r1", items: [identity] });
});
afterEach(cleanup);

test("a plugin_credentials_required 409 shows the credential message and the settings link, marks the stamped connection and never asks to sign in", async () => {
  apis.read.mockRejectedValue(credentialError());
  const flags = renderHook(() => useCredentialRequirements("alice"));
  open();
  expect(await screen.findByText(message)).toBeTruthy();
  expect(
    screen
      .getByRole("link", { name: "Open plugin settings" })
      .getAttribute("href"),
  ).toBe("/settings?section=plugins");
  expect(screen.getByRole("button", { name: "Retry" })).toBeTruthy();
  expect(screen.queryByText(/Sign in/i)).toBeNull();
  expect(
    connectionNeedsCredentials(flags.result.current, {
      id: stamp.installation_id,
      revision: stamp.installation_revision,
      health: undefined,
    }),
  ).toBe(true);
  expect(apis.read).toHaveBeenCalledTimes(1);
});

test("after a credential update the failed read is retried once and no loop follows", async () => {
  apis.read
    .mockRejectedValueOnce(credentialError())
    .mockResolvedValue(ready);
  open();
  await screen.findByText(message);
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 30));
  });
  expect(apis.read).toHaveBeenCalledTimes(1);
  act(() => invalidate("plugins", stamp.installation_id));
  expect(await screen.findByText("Back again")).toBeTruthy();
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 30));
  });
  expect(apis.read).toHaveBeenCalledTimes(2);
  expect(screen.queryByText(message)).toBeNull();
});

test("a 401 keeps the generic failure, marks nothing, and the selected provider is still requested", async () => {
  apis.read.mockRejectedValue(
    new HttpError(401, "Sign in", "plugin_credentials_required"),
  );
  const flags = renderHook(() => useCredentialRequirements("alice"));
  open();
  expect(await screen.findByText("Lyrics could not be loaded.")).toBeTruthy();
  expect(screen.queryByText(message)).toBeNull();
  expect(screen.queryByRole("link", { name: "Open plugin settings" })).toBeNull();
  expect(flags.result.current.size).toBe(0);
  await waitFor(() => expect(apis.read).toHaveBeenCalledTimes(1));
  expect(apis.read.mock.calls[0][2]).toEqual({
    selector: identity.selector,
    installation_revision: stamp.installation_revision,
  });
});

test("a credential error without a stamp shows the guidance and marks no connection", async () => {
  apis.read.mockRejectedValue(
    new HttpError(409, message, body.code, undefined, undefined, {
      error: message,
      code: body.code,
    } satisfies Schema<"ErrorResponse">),
  );
  const flags = renderHook(() => useCredentialRequirements("alice"));
  open();
  expect(await screen.findByText(message)).toBeTruthy();
  expect(flags.result.current.size).toBe(0);
});
