import { useMemo, useState } from "react";
import { pluginsApi } from "../../plugins/api/pluginsApi";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { errorMessage } from "../../../shared/format";
import { log } from "../../../shared/log";
import type {
  PluginInstallation,
  ProviderRecording,
  Recording,
} from "../../../shared/types";
import type { Player } from "../../playback/model";

type Work = {
  generation: number;
  search: AbortController | null;
  adoption: AbortController | null;
  saved: ReadonlyMap<string, Recording>;
};
export type ProviderSelection = {
  id: string;
  intent: "play" | "save";
  pending: boolean;
  error: string;
};
type Results = {
  owner: Work;
  generation: number;
  query: string;
  items: ProviderRecording[];
  next: string | null;
  cursors: (string | null)[];
  page: number;
  pending: boolean;
  error: string;
  saved: ReadonlyMap<string, Recording>;
  selection: ProviderSelection | null;
};

export function useDiscovery(
  account: string,
  connection: PluginInstallation | null,
  player: Pick<Player, "prepareSelection">,
) {
  const scope = useRequestScope(
    JSON.stringify([account, connection?.id, connection?.revision]),
  );
  const owner = useMemo<Work>(
    () => ({ generation: 0, search: null, adoption: null, saved: new Map() }),
    [scope],
  );
  const [snapshot, setSnapshot] = useState<Results | null>(null);
  const current = snapshot?.owner === owner ? snapshot : null;
  async function loadPage(
    query: string,
    cursors: (string | null)[],
    page: number,
    previous: Results | null,
  ) {
    const request = scope.capture();
    if (
      !request.active() ||
      !connection?.manifest.capabilities.includes("search")
    )
      return false;
    owner.search?.abort();
    owner.adoption?.abort();
    owner.adoption = null;
    const controller = new AbortController();
    owner.search = controller;
    const generation = ++owner.generation;
    query = query.trim();
    const initial: Results = {
      owner,
      generation,
      query,
      items: previous?.items ?? [],
      next: previous?.next ?? null,
      cursors: previous?.cursors ?? [null],
      page: previous?.page ?? 0,
      pending: Boolean(query),
      error: "",
      saved: owner.saved,
      selection: null,
    };
    setSnapshot(initial);
    if (!query) {
      owner.search = null;
      return true;
    }
    const signal = AbortSignal.any([request.signal, controller.signal]);
    const active = () =>
      request.active() && owner.generation === generation && !signal.aborted;
    try {
      const result = await pluginsApi.search(
        account,
        connection.id,
        {
          installation_revision: connection.revision,
          query,
          cursor: cursors[page],
          limit: 20,
        },
        signal,
      );
      if (!active()) return false;
      const next = result.next_cursor ?? null;
      const repeated =
        next !== null && cursors.slice(0, page + 1).includes(next);
      setSnapshot({
        ...initial,
        items: result.items,
        cursors,
        page,
        next: repeated ? null : next,
        pending: false,
        error: repeated
          ? "This service repeated a results page. Search again to continue."
          : "",
      });
      log.info("discovery.search_completed", {
        account_id: account,
        count: result.items.length,
        index: page,
      });
      return true;
    } catch (error) {
      if (active())
        setSnapshot({ ...initial, pending: false, error: errorMessage(error) });
      return false;
    } finally {
      if (owner.search === controller) owner.search = null;
    }
  }
  async function turn(direction: 1 | -1) {
    if (!current || current.generation !== owner.generation || owner.search)
      return false;
    const page = current.page + direction;
    if (page < 0 || (direction === 1 && current.next === null)) return false;
    const cursors =
      direction === 1
        ? [...current.cursors.slice(0, current.page + 1), current.next]
        : current.cursors;
    return loadPage(current.query, cursors, page, current);
  }
  async function select(hit: ProviderRecording, intent: "play" | "save") {
    const request = scope.capture();
    if (
      !request.active() ||
      !current ||
      current.generation !== owner.generation ||
      owner.search ||
      owner.adoption ||
      (intent === "play" &&
        !connection?.manifest.capabilities.includes("sources")) ||
      !connection?.manifest.capabilities.includes("search") ||
      !current.items.some((item) => item.id === hit.id)
    )
      return null;
    const generation = owner.generation;
    const controller = new AbortController();
    owner.adoption = controller;
    const signal = AbortSignal.any([request.signal, controller.signal]);
    const active = () =>
      request.active() && owner.generation === generation && !signal.aborted;
    const update = (change: (value: Results) => Results) => {
      if (active())
        setSnapshot((value) =>
          value?.owner === owner && value.generation === generation
            ? change(value)
            : value,
        );
    };
    update((value) => ({
      ...value,
      selection: { id: hit.id, intent, pending: true, error: "" },
    }));
    try {
      const play = intent === "play" ? player.prepareSelection() : null;
      const recording = await pluginsApi.adopt(
        account,
        connection.id,
        {
          installation_revision: connection.revision,
          provider_recording_id: hit.id,
        },
        signal,
      );
      if (!active()) return null;
      owner.saved = new Map(owner.saved).set(hit.id, recording);
      update((value) => ({ ...value, saved: owner.saved }));
      if (play && recording.available) await play(recording);
      else if (play)
        update((value) => ({
          ...value,
          selection: {
            id: hit.id,
            intent,
            pending: false,
            error:
              "No source is ready to play. This track was added to your library.",
          },
        }));
      if (!active()) return null;
      log.info("discovery.selection_completed", {
        account_id: account,
        action: intent,
        recording_id: recording.id,
      });
      return recording;
    } catch (error) {
      update((value) => ({
        ...value,
        selection: {
          id: hit.id,
          intent,
          pending: false,
          error: errorMessage(error),
        },
      }));
      return null;
    } finally {
      if (owner.adoption === controller) owner.adoption = null;
      update((value) => ({
        ...value,
        selection: value.selection
          ? { ...value.selection, pending: false }
          : null,
      }));
    }
  }
  return {
    items: current?.items ?? [],
    query: current?.query ?? "",
    page: current?.page ?? 0,
    hasNext: Boolean(current?.next),
    hasPrevious: (current?.page ?? 0) > 0,
    pending: current?.pending ?? false,
    error: current?.error ?? "",
    saved: current?.saved ?? new Map<string, Recording>(),
    selection: current?.selection ?? null,
    select,
    search: (query: string) => loadPage(query, [null], 0, null),
    next: () => turn(1),
    previous: () => turn(-1),
  };
}
