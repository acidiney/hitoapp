import type { Schema } from "../../../shared/schema";
import type { ReactNode } from "react";

import MetadataObservations from "../../metadata/components/MetadataObservations";
import RecordingLyrics from "../../lyrics/components/RecordingLyrics";
import { useRecordingLyrics } from "../../lyrics/hooks/useRecordingLyrics";
import { useAccountSwitch } from "../../../shared/hooks/useAccountSwitch";
import { usePlayer } from "../../playback/components/PlaybackProvider";
import { useRecordingDownload } from "../../downloads/hooks/useRecordingDownload";
import { useSourceChoice } from "../../sources/hooks/useSourceChoice";
import TrackHeader from "../components/TrackHeader";
import TrackMetadata from "../components/TrackMetadata";
import TrackOverview from "../components/TrackOverview";
import TrackSources from "../components/TrackSources";
import TrackTabBar, { panelId, tabId } from "../components/TrackTabBar";
import { useTrackTab } from "../hooks/useTrackTab";
import { hasLocalFile, trackStatus } from "../model/trackStatus";
import { trackTabs, type TrackTab } from "../model/trackTabs";

function Panel({
  tab,
  current,
  children,
}: {
  tab: TrackTab;
  current: TrackTab;
  children: ReactNode;
}) {
  return (
    <div
      className="track-panel"
      role="tabpanel"
      id={panelId(tab)}
      aria-labelledby={tabId(tab)}
      data-active={tab === current}
    >
      {children}
    </div>
  );
}

function TrackTabs({
  user,
  detail,
  copies,
  liked,
  playlists,
  native_playback,
}: Schema<"RecordingDetailsPageProps">) {
  const player = usePlayer();
  const { tab, select, reveal } = useTrackTab(detail.recording_id);
  const download = useRecordingDownload(user.id, detail.recording_id);
  const [automaticLyrics] = useAccountSwitch("lyrics-auto", user.id);
  const lyrics = useRecordingLyrics(
    user.id,
    detail,
    automaticLyrics,
    tab === "lyrics",
  );
  const choice = useSourceChoice(
    user.id,
    detail.recording_id,
    download.invalidateRead,
  );
  const source = detail.selected_observation;
  const localFile = hasLocalFile(detail);
  const base = trackStatus(choice.choice, choice.unavailable, localFile);
  const status = {
    ...base,
    playable: choice.eligibilityPending && base.playable ? null : base.playable,
  };
  const edited =
    detail.title.personal !== null || detail.artist.personal !== null;
  const readySources = choice.choice?.sources.filter(
    (entry) => entry.source.ready,
  ).length;
  const sourceCount =
    readySources === undefined
      ? localFile
        ? 1
        : undefined
      : Math.max(readySources, localFile && readySources === 0 ? 1 : 0);
  const badges: Partial<Record<TrackTab, string>> = {
    ...(sourceCount === undefined ? {} : { sources: String(sourceCount) }),
    ...(edited ? { metadata: "Edited" } : {}),
  };
  const target = { kind: "recording", id: detail.recording_id } as const;
  return (
    <div className="track-page" data-tab={tab}>
      <TrackHeader
        account={user.id}
        detail={detail}
        liked={liked}
        playlists={playlists}
        status={status}
        onPlay={() =>
          void player.playQueue([
            {
              id: detail.recording_id,
              title: detail.title.value,
              artist: detail.artist.value,
              album: source?.album ?? null,
              duration_ms: source?.duration_ms ?? 0,
              available: true,
            },
          ])
        }
        onFindAudio={() => reveal("find-audio")}
      />
      <TrackTabBar tab={tab} badges={badges} onSelect={select} />
      {trackTabs.map((entry) => (
        <Panel key={entry} tab={entry} current={tab}>
          {entry === "overview" && (
            <TrackOverview
              detail={detail}
              copies={copies}
              status={status}
              localFile={localFile}
              choice={choice.choice}
              lyricPreview={lyrics.preview.filter((line) => line.trim() !== "")}
              onSelect={select}
              onReveal={reveal}
            />
          )}
          {entry === "lyrics" && (
            <RecordingLyrics
              lyrics={lyrics}
              saved={{
                title: detail.title.value,
                artist: detail.artist.value,
                album: source?.album,
                durationMs: source?.duration_ms,
              }}
            />
          )}
          {entry === "sources" && (
            <TrackSources
              account={user.id}
              detail={detail}
              choice={choice}
              localFile={localFile}
              active={tab === "sources"}
              download={download}
              nativePlayback={native_playback}
              onFixSource={() => reveal("find-audio")}
            />
          )}
          {entry === "metadata" && (
            <TrackMetadata account={user.id} detail={detail} copies={copies} />
          )}
          {entry === "history" && (
            <MetadataObservations
              key={`${user.id}/${detail.recording_id}/${source?.id ?? "none"}/${detail.title.operation_id ?? "inherited"}/${detail.artist.operation_id ?? "inherited"}`}
              account={user.id}
              target={target}
              initial={detail.observations}
              selected={source?.id}
            />
          )}
        </Panel>
      ))}
    </div>
  );
}

export default function RecordingDetailsPage(
  props: Schema<"RecordingDetailsPageProps">,
) {
  return <TrackTabs key={props.detail.recording_id} {...props} />;
}
