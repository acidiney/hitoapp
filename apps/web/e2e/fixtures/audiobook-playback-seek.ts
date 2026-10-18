import type { Page } from "@playwright/test";

export type AudiobookSeekEvent = {
  kind: number;
  owner: number;
  source: string;
  time: number;
  position: number;
  trusted: boolean;
  connected: boolean;
  active: boolean;
  unique: boolean;
};
export type AudiobookSeekWitness = {
  source: string;
  position: number;
  changed: boolean;
  overflow: boolean;
  observedUntil: number;
  events: AudiobookSeekEvent[];
};

export async function audiobookSeekCheckpoint(
  page: Page,
  path: string,
  position: number,
) {
  const handle = await page.evaluateHandle(
    ({ path, position }) => {
      const source = new URL(path, location.href).href;
      const events: AudiobookSeekEvent[] = [];
      const owners = new Map<HTMLAudioElement, number>();
      let retained: HTMLAudioElement | null = null;
      let completed: AudiobookSeekEvent | null = null;
      let changed = false;
      let overflow = false;
      const sourceOf = (audio: HTMLAudioElement) => {
        const attribute = audio.getAttribute("src");
        return attribute
          ? new URL(attribute, location.href).href
          : audio.currentSrc;
      };
      const sample = (
        audio: HTMLAudioElement | null,
        kind: number,
        trusted: boolean,
      ): AudiobookSeekEvent => {
        if (audio && !owners.has(audio)) {
          if (owners.size >= 8) overflow = true;
          else owners.set(audio, owners.size + 1);
        }
        const selected = document.querySelectorAll("audio[data-hito-player]");
        const sameSource = [
          ...document.querySelectorAll<HTMLAudioElement>("audio"),
        ].filter((element) => sourceOf(element) === source);
        return {
          kind,
          owner: audio ? (owners.get(audio) ?? 0) : 0,
          source: audio?.currentSrc ?? "",
          time: performance.timeOrigin + performance.now(),
          position: audio ? Math.round(audio.currentTime * 1000) : -1,
          trusted,
          connected: audio?.isConnected ?? false,
          active: selected.length === 1 && selected[0] === audio,
          unique: sameSource.length === 1 && sameSource[0] === audio,
        };
      };
      const inspect = (records: MutationRecord[]) => {
        if (!retained) return;
        for (const record of records) {
          if (
            (record.type === "attributes" && record.target === retained) ||
            [...record.removedNodes].some(
              (node) => node === retained || node.contains(retained),
            )
          )
            changed = true;
          if (
            record.target instanceof HTMLAudioElement &&
            record.target !== retained &&
            (record.attributeName === "data-hito-player" ||
              record.target.hasAttribute("data-hito-player") ||
              sourceOf(record.target) === source ||
              record.target.currentSrc === source ||
              (record.attributeName === "src" &&
                record.oldValue &&
                new URL(record.oldValue, location.href).href === source))
          )
            changed = true;
          for (const node of record.addedNodes) {
            const added =
              node instanceof HTMLAudioElement
                ? [node]
                : node instanceof Element
                  ? [...node.querySelectorAll<HTMLAudioElement>("audio")]
                  : [];
            if (
              added.some(
                (audio) =>
                  audio !== retained &&
                  (audio.hasAttribute("data-hito-player") ||
                    sourceOf(audio) === source ||
                    audio.currentSrc === source),
              )
            )
              changed = true;
          }
        }
      };
      const mutations = new MutationObserver(inspect);
      mutations.observe(document.body, {
        subtree: true,
        childList: true,
        attributes: true,
        attributeOldValue: true,
        attributeFilter: ["src", "data-hito-player"],
      });
      const observed = [
        "loadstart",
        "loadedmetadata",
        "seeking",
        "seeked",
        "pause",
        "emptied",
        "abort",
        "error",
        "ended",
        "timeupdate",
      ];
      const record = (event: Event) => {
        if (!(event.target instanceof HTMLAudioElement)) return;
        const audio = event.target;
        const kind = observed.indexOf(event.type);
        if (!retained) {
          if (
            kind !== 0 ||
            sourceOf(audio) !== source ||
            !audio.hasAttribute("data-hito-player")
          )
            return;
          mutations.takeRecords();
          retained = audio;
        }
        if (audio !== retained) {
          if (sourceOf(audio) === source && audio.isConnected) changed = true;
          return;
        }
        if (event.type === "timeupdate") {
          if (
            completed ||
            changed ||
            overflow ||
            !event.isTrusted ||
            events.length !== 4 ||
            !events.every(
              (event, index) => event.kind === index && event.trusted,
            )
          )
            return;
          const advancement = sample(audio, 4, true);
          if (
            advancement.position > position + 10 &&
            advancement.position < position + 2000 &&
            advancement.connected &&
            advancement.active &&
            advancement.unique &&
            advancement.source === source &&
            !audio.paused &&
            !audio.ended &&
            audio.error === null
          )
            completed = advancement;
          return;
        }
        if (kind > 3 || (kind === 0 && events.length)) changed = true;
        if (events.length >= 8) {
          overflow = true;
          return;
        }
        events.push(sample(audio, kind, event.isTrusted));
      };
      observed.forEach((event) =>
        document.addEventListener(event, record, true),
      );
      return {
        read(): AudiobookSeekWitness {
          inspect(mutations.takeRecords());
          const selected = document.querySelectorAll("audio[data-hito-player]");
          const sameSource = [
            ...document.querySelectorAll<HTMLAudioElement>("audio"),
          ].filter((audio) => sourceOf(audio) === source);
          if (
            retained &&
            (!retained.isConnected ||
              selected.length !== 1 ||
              selected[0] !== retained ||
              sameSource.length !== 1 ||
              sameSource[0] !== retained ||
              retained.error !== null ||
              retained.paused ||
              retained.ended ||
              retained.currentSrc !== source ||
              retained.playbackRate !== 1)
          )
            changed = true;
          return {
            source,
            position,
            changed,
            overflow,
            observedUntil: performance.timeOrigin + performance.now(),
            events: completed ? [...events, completed] : events.slice(),
          };
        },
        close() {
          mutations.disconnect();
          observed.forEach((event) =>
            document.removeEventListener(event, record, true),
          );
        },
      };
    },
    { path, position },
  );
  return {
    read: () => handle.evaluate((checkpoint) => checkpoint.read()),
    async dispose() {
      try {
        await handle.evaluate((checkpoint) => checkpoint.close());
      } finally {
        await handle.dispose();
      }
    },
  };
}
