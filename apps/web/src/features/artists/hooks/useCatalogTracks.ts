import { useEffect, useMemo } from "react";
import { router } from "@inertiajs/react";
import type { Schema } from "../../../shared/schema";
import type { Mutate } from "../../../shared/hooks/useMutation";
import { albumEditionApi } from "../api/albumEditionApi";
import type { ArtistEditionContext } from "./useArtistEditions";
import { useCatalogAlbum } from "./useCatalogAlbum";
import { useCatalogPlayback } from "./useCatalogPlayback";
import { useDiscographyLibrary } from "./useDiscographyLibrary";

export type CatalogTrack = {
  id: string;
  number: string;
  title: string;
  credit: string | null;
  durationMs: number | null;
  status: "idle" | "finding" | "not_found" | "failed";
  recording: Schema<"Recording"> | null;
};

export type CatalogTrackControls = {
  play(index: number): void;
  find(index: number): void;
  open(index: number): void;
  like(index: number): void;
};

export function useCatalogTracks(
  context: ArtistEditionContext,
  withLibrary: boolean,
  mutate?: Mutate,
) {
  const { account, recording, association, group } = context;
  const album = useCatalogAlbum(context);
  const edition = album.editions.chosen?.id ?? null;
  const { linked: remembered } = album.tracks;
  const rememberedByRow = useMemo(() => {
    const known = new Set(album.rows.map((row) => row.id));
    return new Map(
      remembered
        .filter((match) => known.has(match.occurrence_id))
        .map((match) => [match.occurrence_id, match.recording]),
    );
  }, [remembered, album.rows]);
  const playback = useCatalogPlayback(
    context,
    edition,
    album.rows.map((row) => row.id),
    rememberedByRow,
  );
  const library = useDiscographyLibrary(
    account,
    recording,
    association,
    [group],
    withLibrary,
  );
  const linked = library.albums[group.id]?.[0]?.release_id ?? null;
  const { seed } = playback;
  const ready = album.rows.length > 0;

  useEffect(() => {
    if (!linked || edition === null || !ready) return;
    const controller = new AbortController();
    albumEditionApi
      .tracks(
        account,
        linked,
        {
          recording_id: recording,
          association_id: association.id,
          release_group_id: group.id,
          edition_id: edition,
        },
        controller.signal,
      )
      .then((result) => {
        if (result.status !== "available" || controller.signal.aborted) return;
        seed(
          new Map(
            result.matches.map((match) => [
              match.occurrence_id,
              match.recording,
            ]),
          ),
        );
      })
      .catch(() => undefined);
    return () => controller.abort();
  }, [account, recording, association.id, group.id, linked, edition, ready]);

  const rows: CatalogTrack[] = album.rows.map((row) => {
    const found = playback.recordings.get(row.id) ?? null;
    const state = playback.rows.get(row.id)?.status ?? "idle";
    return {
      ...row,
      status: found || state === "found" ? "idle" : state,
      recording: found,
    };
  });

  const controls: CatalogTrackControls = {
    play: (index) => void playback.start(index),
    find: (index) => void playback.find(index),
    open: async (index) => {
      const found = await playback.find(index);
      if (found) router.get(`/recordings/${encodeURIComponent(found.id)}`);
    },
    like: async (index) => {
      const found = await playback.find(index);
      if (!found || !mutate) return;
      await mutate({ kind: "like_set", recording_id: found.id, liked: true });
    },
  };

  return {
    ...album,
    occurrences: album.rows,
    playback,
    library,
    linked,
    rows,
    controls,
  };
}
