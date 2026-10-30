import type { BrowserContext, Request } from "@playwright/test";
import type { Schema } from "../../src/shared/schema";
import { artistDiscographyLogs } from "./artist-discography-logs";
import { captureConsolePrivacy } from "./console-privacy";

type CatalogRead = "discography" | "editions" | "tracks";
const uuid = /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/;

export async function artistDiscographyEvidence(
  context: BrowserContext,
  origin: string,
  privateValues: string[],
  editions = false,
) {
  const pending = new Set<Promise<void>>();
  const texts = new Set<string>();
  const statusRequests = new WeakSet<Request>();
  const catalogRequests = new WeakMap<Request, CatalogRead>();
  let target: { account: string; recording: string } | null = null;
  let associationPath: string | null = null;
  let selection: Schema<"ReadEditionTrackList"> | undefined;
  const logs = artistDiscographyLogs(editions);
  const counts = {
    requests: 0,
    armedRequests: 0,
    preparatoryWrites: 0,
    unexpectedWrites: 0,
    foreign: 0,
    audio: 0,
    download: 0,
    downloadStatusReads: 0,
    downloadStatusAbsentReplies: 0,
    invalidDownloadStatus: 0,
    sourceWrites: 0,
    pageErrors: 0,
    consoleBytes: 0,
    privateConsole: 0,
    malformedLogs: 0,
    discographyRequests: 0,
    discographyReplies: 0,
    editionRequests: 0,
    editionReplies: 0,
    trackListRequests: 0,
    trackListReplies: 0,
    invalidCatalog: 0,
    catalogBytes: 0,
    bounds: 0,
    drainTimeout: 0,
  };
  const privateText = (text: string) =>
    privateValues.some((value) => {
      if (!value.length) return false;
      const escaped = JSON.stringify(value).slice(1, -1);
      const ascii = escaped.replace(
        /[\u007f-\uffff]/g,
        (character) =>
          `\\u${character.charCodeAt(0).toString(16).padStart(4, "0")}`,
      );
      return [value, escaped, ascii].some((variant) => text.includes(variant));
    });
  const observeError = () => {
    counts.pageErrors++;
  };
  context.on("page", (page) => page.on("pageerror", observeError));
  for (const page of context.pages()) page.on("pageerror", observeError);
  context.on("request", (request) => {
    if (++counts.requests > 300) counts.bounds++;
    const url = new URL(request.url());
    const method = request.method();
    if (["http:", "https:"].includes(url.protocol) && url.origin !== origin)
      counts.foreign++;
    if (
      request.resourceType() === "media" ||
      /\/audio(?:\/|$)/.test(url.pathname)
    )
      counts.audio++;
    if (/\/(?:download|downloads)(?:\/|$)/.test(url.pathname)) {
      if (
        target &&
        url.origin === origin &&
        !url.search &&
        !url.hash &&
        method === "GET" &&
        url.pathname === `/api/v1/recordings/${target.recording}/download` &&
        request.postData() === null &&
        request.headers()["x-hito-request"] === "1" &&
        request.headers()["x-hito-account"] === target.account
      ) {
        counts.downloadStatusReads++;
        statusRequests.add(request);
      } else counts.download++;
    }
    const writing = !["GET", "HEAD", "OPTIONS"].includes(method);
    if (
      writing &&
      /\/(?:sources|source-choice|preparations)(?:\/|$)/.test(url.pathname)
    )
      counts.sourceWrites++;
    if (!associationPath) {
      if (writing) counts.preparatoryWrites++;
      return;
    }
    counts.armedRequests++;
    if (!writing) return;
    const kind: CatalogRead | null =
      url.pathname === `${associationPath}/discography`
        ? "discography"
        : editions &&
            url.pathname === `${associationPath}/release-group-editions`
          ? "editions"
          : editions && url.pathname === `${associationPath}/edition-track-list`
            ? "tracks"
            : null;
    if (
      url.origin !== origin ||
      !kind ||
      url.search ||
      url.hash ||
      method !== "POST"
    ) {
      counts.unexpectedWrites++;
      return;
    }
    if (kind === "discography") counts.discographyRequests++;
    else if (kind === "editions") counts.editionRequests++;
    else counts.trackListRequests++;
    catalogRequests.set(request, kind);
    try {
      const raw = request.postData();
      const maximum =
        kind === "tracks" ? 2048 : kind === "editions" ? 8_768 : 8_256;
      if (!raw || Buffer.byteLength(raw) > maximum)
        throw new Error("Invalid owned catalog envelope");
      const value: unknown = JSON.parse(raw);
      const keys =
        kind === "discography"
          ? "continuation,limit"
          : kind === "editions"
            ? "continuation,limit,release_group_id"
            : "edition_id,release_group_id";
      if (
        value === null ||
        typeof value !== "object" ||
        Object.keys(value).sort().join(",") !== keys ||
        request.headers()["x-hito-account"] !== target?.account ||
        request.headers()["x-hito-request"] !== "1" ||
        !request.headers()["content-type"]?.startsWith("application/json") ||
        (kind !== "discography" &&
          (!selection ||
            !("release_group_id" in value) ||
            value.release_group_id !== selection.release_group_id)) ||
        (kind === "tracks" &&
          (!("edition_id" in value) ||
            value.edition_id !== selection?.edition_id)) ||
        (kind !== "tracks" &&
          (!("limit" in value) ||
            value.limit !== 20 ||
            !("continuation" in value) ||
            !(
              value.continuation === null ||
              (typeof value.continuation === "string" &&
                value.continuation.length > 0 &&
                value.continuation.length <= 8192 &&
                /^[\x20-\x7e]+$/.test(value.continuation))
            )))
      )
        counts.invalidCatalog++;
    } catch {
      counts.invalidCatalog++;
    }
  });
  context.on("response", (response) => {
    const status = statusRequests.has(response.request());
    const catalog = catalogRequests.get(response.request());
    if (!status && !catalog) return;
    if (pending.size >= 32) {
      counts.bounds++;
      return;
    }
    const checking = (async () => {
      try {
        const maximum = status ? 4096 : 294_912;
        const declared = Number(response.headers()["content-length"] ?? "0");
        if (!Number.isFinite(declared) || declared < 0 || declared > maximum)
          throw new Error("Owned response exceeds its limit");
        const bytes = await response.body();
        if (
          bytes.length > maximum ||
          !response.headers()["content-type"]?.startsWith("application/json")
        )
          throw new Error("Invalid owned response envelope");
        const value: unknown = JSON.parse(bytes.toString());
        if (status) {
          if (
            response.status() === 404 &&
            typeof value === "object" &&
            value !== null &&
            Object.keys(value).length === 1 &&
            "error" in value &&
            value.error === "Resource not found"
          )
            counts.downloadStatusAbsentReplies++;
          else counts.invalidDownloadStatus++;
        } else {
          counts.catalogBytes += bytes.length;
          if (counts.catalogBytes > (editions ? 6 : 3) * 294_912)
            counts.bounds++;
          if (
            response.status() !== 200 ||
            response.headers()["cache-control"] !== "private, no-store"
          )
            counts.invalidCatalog++;
          else if (catalog === "discography") counts.discographyReplies++;
          else if (catalog === "editions") counts.editionReplies++;
          else counts.trackListReplies++;
        }
      } catch {
        if (status) counts.invalidDownloadStatus++;
        else counts.invalidCatalog++;
      }
    })().finally(() => pending.delete(checking));
    pending.add(checking);
  });
  await captureConsolePrivacy(
    context,
    (text, capture) => {
      counts.consoleBytes += Buffer.byteLength(text);
      if (counts.consoleBytes <= 524_288) texts.add(text);
      else counts.bounds++;
      if (privateText(text)) counts.privateConsole++;
      if (capture === "snapshot" && !logs.observe(text)) counts.malformedLogs++;
    },
    () => {
      counts.malformedLogs++;
    },
    pending,
  );
  await context.addInitScript((expected) => {
    if (location.origin === expected)
      localStorage.setItem("hito-log-level", "debug");
  }, origin);
  return {
    counts,
    acceptedReview: logs.acceptedReview,
    acceptedCatalog: logs.acceptedCatalog,
    watchDownloadStatus(account: string, recording: string) {
      if (target || !uuid.test(account) || !uuid.test(recording))
        throw new Error("Invalid owned observer target");
      target = { account, recording };
    },
    arm(association: string, selected?: Schema<"ReadEditionTrackList">) {
      if (
        !target ||
        associationPath ||
        !uuid.test(association) ||
        editions !== Boolean(selected)
      )
        throw new Error("Invalid owned catalog observation phase");
      associationPath = `/api/v1/recordings/${target.recording}/artist-associations/${association}`;
      selection = selected;
    },
    complete: logs.complete,
    async finish(releasePrivate = false) {
      let timer: ReturnType<typeof setTimeout> | undefined;
      try {
        await Promise.race([
          (async () => {
            while (pending.size) await Promise.allSettled([...pending]);
          })(),
          new Promise<void>((resolve) => {
            timer = setTimeout(() => {
              counts.drainTimeout++;
              resolve();
            }, 3000);
          }),
        ]);
      } finally {
        clearTimeout(timer);
      }
      for (const text of texts) if (privateText(text)) counts.privateConsole++;
      if (releasePrivate) texts.clear();
    },
  };
}
