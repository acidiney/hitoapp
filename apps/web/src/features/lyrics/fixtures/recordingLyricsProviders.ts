import { createElement } from "react";
import { ArtworkProvider } from "../../artwork/components/ArtworkProvider";
import { NoticeProvider } from "../../../shared/ui/Notices";
import type { components } from "../../../generated";
import RecordingDetailsPage from "../../recordings/pages/RecordingDetailsPage";
import { PlaybackProvider } from "../../playback/components/PlaybackProvider";
import type { RecordingDetailsPageProps } from "../../../shared/types";
import { lyricObservation, readyLyrics } from "./recordingLyrics";

type Schema = components["schemas"];

export const attachedLyricProvider = {
  observation_id: "8d49c593-d800-4c6d-b398-727d2f0f809f",
  installation_id: "cf7bac50-aaf2-42ed-9070-70c67531c91c",
  installation_revision: 17,
  provider_recording: {
    id: 'attached:lyrics/private?edition="3"',
    revision: 'opaque:"attached-version"',
  },
  label: "Attached lyric provider",
} satisfies Schema["RecordingLyricsProvider"];

export const primaryLyricProvider = {
  observation_id: lyricObservation.id,
  installation_id: lyricObservation.plugin_installation_id,
  installation_revision: 9,
  provider_recording: {
    id: lyricObservation.provider_recording_id,
    revision: lyricObservation.provider_revision,
  },
  label: "Primary metadata provider",
} satisfies Schema["RecordingLyricsProvider"];

export const attachedLyrics = {
  ...readyLyrics,
  observation_id: attachedLyricProvider.observation_id,
  document: {
    ...readyLyrics.document,
    recording: attachedLyricProvider.provider_recording,
  },
} satisfies Schema["LyricsLookup"];

export function lyricProviderPage(props: RecordingDetailsPageProps) {
  return createElement(
    NoticeProvider,
    null,
    createElement(ArtworkProvider, {
      account: props.user.id,
      children: createElement(PlaybackProvider, {
        account: props.user.id,
        children: createElement(RecordingDetailsPage, props),
      }),
    }),
  );
}

export function heldLyricJson(value: unknown) {
  let body!: ReadableStreamDefaultController<Uint8Array>;
  let released = false;
  const response = new Response(
    new ReadableStream<Uint8Array>({
      start(controller) {
        body = controller;
      },
    }),
    { headers: { "content-type": "application/json" } },
  );
  return {
    response,
    release() {
      if (released) return;
      released = true;
      body.enqueue(new TextEncoder().encode(JSON.stringify(value)));
      body.close();
    },
  };
}
