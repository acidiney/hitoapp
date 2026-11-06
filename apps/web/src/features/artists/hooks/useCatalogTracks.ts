import { useEffect, useState } from "react";
import { router } from "@inertiajs/react";
import type { Schema } from "../../../shared/schema";
import type {
  CatalogControls,
  TrackRow,
  TrackTableRow,
} from "../../playback/components/TrackTable";
import type { Mutate } from "../../../shared/hooks/useMutation";
import { albumEditionApi } from "../api/albumEditionApi";
import type { ArtistEditionContext } from "./useArtistEditions";
import { useCatalogAlbum } from "./useCatalogAlbum";
import { useCatalogPlayback } from "./useCatalogPlayback";
import { useDiscographyLibrary } from "./useDiscographyLibrary";

function trackRow(recording: Schema<"Recording">, liked: boolean): TrackRow {
  return { ...recording, liked, row_id: recording.id };
}

export function useCatalogTracks(
  context: ArtistEditionContext,
  withLibrary: boolean,
  mutate?: Mutate,
) {
  const { account, recording, association, group } = context;
  const album = useCatalogAlbum(context);
  const edition = album.editions.chosen?.id ?? null;
  const playback = useCatalogPlayback(
    context,
    edition,
    album.rows.map((row) => row.id),
  );
  const library = useDiscographyLibrary(
    account,
    recording,
    association,
    [group],
    withLibrary,
  );
  const linked = library.albums[group.id]?.[0]?.release_id ?? null;
  const [liked, setLiked] = useState<ReadonlyMap<string, boolean>>(new Map());
  const { seed } = playback;
  const ready = album.rows.length > 0;

  useEffect(() => setLiked(new Map()), [account, group.id, edition]);

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

  const rows: TrackTableRow[] = album.rows.map((row) => {
    const found = playback.recordings.get(row.id);
    const state = playback.rows.get(row.id)?.status ?? "idle";
    return {
      kind: "catalog",
      row: {
        id: row.id,
        number: row.number,
        title: row.title,
        artist: row.credit ?? album.credit ?? "",
        album: group.title,
        durationMs: row.durationMs,
        status: found ? "found" : state === "found" ? "idle" : state,
        recording: found
          ? trackRow(found, liked.get(found.id) ?? found.liked)
          : null,
      },
    };
  });

  const controls: CatalogControls = {
    play: (index) => void playback.start(index),
    find: (index) => void playback.find(index),
    open: async (index) => {
      const found = await playback.find(index);
      if (found) router.get(`/recordings/${encodeURIComponent(found.id)}`);
    },
    like: async (index) => {
      const found = await playback.find(index);
      if (!found || !mutate) return;
      const action = {
        kind: "like_set",
        recording_id: found.id,
        liked: true,
      } as const;
      if ((await mutate(action)).ok) controls.mutated(action);
    },
    mutated: (action) => {
      if (action.kind === "like_set")
        setLiked((value) =>
          new Map(value).set(action.recording_id, action.liked),
        );
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
