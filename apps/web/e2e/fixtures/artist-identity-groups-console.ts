import { createHash } from "node:crypto";
import type { BrowserContext } from "@playwright/test";
import { captureConsolePrivacy } from "./console-privacy";

const uuid = /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/;
const digest = (text: string) =>
  createHash("sha256").update(text).digest("hex");
export type ArtistGroupCommands = Map<
  string,
  { action: string; accepted: number; failed: number }
>;
type Counts = {
  consoleBytes: number;
  bounds: number;
  malformedLogs: number;
  reviewStarts: number;
  reviewFinishes: number;
  acceptedReads: number;
  acceptedChoices: number;
  acceptedResets: number;
  failedChoices: number;
  privateConsole: number;
};

export async function artistGroupsConsole(
  context: BrowserContext,
  origin: string,
  privateValues: string[],
  pending: Set<Promise<void>>,
  counts: Counts,
  commands: ArtistGroupCommands,
) {
  const texts = new Map<string, string>();
  const reviewStarts = new Map<string, string>();
  const reviewFinishes = new Set<string>();
  await captureConsolePrivacy(
    context,
    (text, capture) => {
      counts.consoleBytes += Buffer.byteLength(text);
      if (counts.consoleBytes > 524_288) {
        counts.bounds++;
        return;
      }
      texts.set(digest(text), text);
      if (capture !== "snapshot") return;
      try {
        const message: unknown = JSON.parse(text);
        if (!Array.isArray(message) || message[0] !== "[hito]") return;
        const event = message[1] as Record<string, unknown> | null;
        if (
          !event ||
          typeof event !== "object" ||
          typeof event.event !== "string"
        )
          return;
        if (
          !/^(artist_review\.(started|finished)|artist_identity\.finished)$/.test(
            event.event,
          )
        )
          return;
        if (
          Object.keys(event).some(
            (key) =>
              ![
                "event",
                "action",
                "request_id",
                "reason",
                "duration_ms",
              ].includes(key),
          ) ||
          typeof event.request_id !== "string" ||
          !uuid.test(event.request_id)
        )
          throw new Error("Invalid owned workflow log");
        if (event.event === "artist_review.started") {
          if (
            !["load", "search", "preview", "confirm", "profile"].includes(
              String(event.action),
            ) ||
            reviewStarts.has(event.request_id) ||
            reviewStarts.size >= 64
          )
            throw new Error("Invalid owned review start");
          reviewStarts.set(event.request_id, String(event.action));
          counts.reviewStarts++;
          return;
        }
        if (
          typeof event.duration_ms !== "number" ||
          !Number.isFinite(event.duration_ms) ||
          event.duration_ms < 0
        )
          throw new Error("Invalid owned workflow duration");
        if (event.event === "artist_review.finished") {
          if (
            reviewStarts.get(event.request_id) !== event.action ||
            reviewFinishes.has(event.request_id) ||
            event.reason !== "accepted"
          )
            throw new Error("Invalid owned review finish");
          reviewFinishes.add(event.request_id);
          counts.reviewFinishes++;
          return;
        }
        if (event.action === "read" && event.reason === "accepted") {
          counts.acceptedReads++;
          return;
        }
        const command = commands.get(event.request_id);
        if (!command || command.action !== event.action)
          throw new Error("Unknown owned command finish");
        if (event.reason === "accepted" && command.accepted++ === 0) {
          if (command.action === "choose") counts.acceptedChoices++;
          else counts.acceptedResets++;
        } else if (
          event.reason === "failed" &&
          command.action === "choose" &&
          command.failed++ === 0
        )
          counts.failedChoices++;
        else throw new Error("Invalid owned command result");
      } catch {
        counts.malformedLogs++;
      }
    },
    () => counts.malformedLogs++,
    pending,
  );
  await context.addInitScript((expected) => {
    if (location.origin === expected)
      localStorage.setItem("hito-log-level", "debug");
  }, origin);
  return {
    complete: () =>
      reviewStarts.size >= 20 && reviewStarts.size === reviewFinishes.size,
    snapshot() {
      counts.privateConsole = [...texts.values()].filter((text) =>
        privateValues.some((value) => value.length > 0 && text.includes(value)),
      ).length;
      const safe = {
        schemaVersion: 1,
        counts,
        consoleCaptures: texts.size,
        consoleCaptureSha256: digest(JSON.stringify([...texts.keys()].sort())),
        privateCanaries: [...new Set(privateValues.filter(Boolean))].map(
          (value) => ({
            bytes: Buffer.byteLength(value),
            sha256: digest(value),
          }),
        ),
      };
      return safe;
    },
    clear() {
      texts.clear();
    },
  };
}
