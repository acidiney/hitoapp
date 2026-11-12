import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { forgetAutomaticLookups } from "../api/automaticLyrics";
import { useRecordingLyrics } from "../hooks/useRecordingLyrics";
import { lyricRecordingPage } from "../fixtures/recordingLyrics";
import {
  automaticReadyV2,
  identities,
  plainDocument,
  serviceA,
  services,
} from "../fixtures/lyricIdentityWire";
import { held, json } from "../fixtures/lyricServer";

afterEach(() => {
  forgetAutomaticLookups();
  vi.restoreAllMocks();
});

const detail = lyricRecordingPage.detail;
const otherRecording = "77777777-7777-4777-8777-777777777777";

function stubServer(reply: (recording: string) => Promise<Response>) {
  const automatic: { recording: string; signal: AbortSignal | null }[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, options?: RequestInit) => {
      const match = /recordings\/([^/]+)\/lyrics\/(.+)$/.exec(path);
      if (!match) throw new Error(`Unexpected request ${path}`);
      const [, recording, route] = match;
      if (route === "services")
        return json({ ...services(serviceA), recording_id: recording });
      if (route === "identities")
        return json({ ...identities(), recording_id: recording });
      if (route === "automatic") {
        automatic.push({ recording, signal: options?.signal ?? null });
        return reply(recording);
      }
      throw new Error(`Unexpected route ${route}`);
    }),
  );
  return automatic;
}

function mount() {
  return renderHook(
    (props: { recording: typeof detail }) =>
      useRecordingLyrics("account-1", props.recording, true, true),
    { initialProps: { recording: detail } },
  );
}

test("a held automatic reply is dropped when the recording changes and the new recording still looks up", async () => {
  const hold = held();
  const calls = stubServer((recording) =>
    recording === detail.recording_id
      ? hold.reply
      : Promise.resolve(
          json({
            ...automaticReadyV2(plainDocument(["Fresh text"])),
            recording_id: recording,
          }),
        ),
  );
  const { result, rerender } = mount();
  await waitFor(() => expect(result.current.lookup.pending).toBe(true));
  await waitFor(() => expect(calls).toHaveLength(1));
  rerender({ recording: { ...detail, recording_id: otherRecording } });
  hold.release(json(automaticReadyV2(plainDocument(["Stale text"]))));
  await act(async () => {
    await Promise.resolve();
  });
  expect(JSON.stringify(result.current.lookup.outcome)).not.toContain("Stale");
  await waitFor(() =>
    expect(JSON.stringify(result.current.lookup.outcome)).toContain(
      "Fresh text",
    ),
  );
  expect(calls[0].signal?.aborted).toBe(true);
  expect(calls.map((call) => call.recording)).toEqual([
    detail.recording_id,
    otherRecording,
  ]);
});

test("a held automatic reply is dropped when the account changes", async () => {
  const hold = held();
  stubServer(() => hold.reply);
  const { result, rerender } = renderHook(
    (props: { account: string }) =>
      useRecordingLyrics(props.account, detail, true, true),
    { initialProps: { account: "account-1" } },
  );
  await waitFor(() => expect(result.current.lookup.pending).toBe(true));
  rerender({ account: "account-2" });
  hold.release(json(automaticReadyV2(plainDocument(["Account one text"]))));
  await act(async () => {
    await Promise.resolve();
  });
  expect(JSON.stringify(result.current.lookup)).not.toContain(
    "Account one text",
  );
});
