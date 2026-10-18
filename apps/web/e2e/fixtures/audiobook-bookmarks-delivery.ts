import { isDeepStrictEqual } from "node:util";
import type { BrowserContext, Request, Route } from "@playwright/test";
import type { components } from "../../src/generated";

type Schema = components["schemas"];
type Policy = {
  allowed: (request: Request) => boolean;
  armed: (request: Request) => boolean;
  lose: (request: Request) => void;
};
const uuid = /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/;

export async function bookmarkDelivery(
  context: BrowserContext,
  origin: string,
  policy: Policy,
) {
  const counts = { create: 0, replay: 0, remove: 0, held_reads: 0, invalid: 0 };
  const pending = new Set<Promise<void>>();
  const releases = new Map<number, () => void>();
  let closed = false;
  let target: {
    account: string;
    detail: Schema["AudiobookDetail"];
    entry: Schema["PlaylistEntry"];
    label: string;
  } | null = null;
  let original: {
    text: string;
    batch: Schema["SyncBatch"];
    receipt: Buffer;
  } | null = null;
  let removal: Schema["SyncBatch"] | null = null;
  async function deliver(route: Route) {
    if (closed) {
      await route.abort("aborted");
      return;
    }
    const request = route.request();
    if (!policy.allowed(request)) {
      await route.abort("blockedbyclient");
      return;
    }
    const url = new URL(request.url());
    if (!policy.armed(request) || !target) {
      await route.continue();
      return;
    }
    const sync = url.pathname === "/api/v1/sync" && request.method() === "POST";
    const read =
      url.pathname === `/api/v1/audiobooks/${target.detail.playlist.id}` &&
      request.method() === "GET";
    if (!sync && !read) {
      await route.continue();
      return;
    }
    if (
      url.origin !== origin ||
      url.search ||
      request.headers()["x-hito-account"] !== target.account ||
      request.headers()["x-hito-request"] !== "1"
    )
      throw new Error("The bookmark request has the wrong authority");
    let batch: Schema["SyncBatch"] | null = null;
    let text = "";
    let first = false;
    if (sync) {
      text = request.postData() ?? "";
      if (!text || Buffer.byteLength(text) > 4096)
        throw new Error("The bookmark mutation exceeded its bound");
      batch = JSON.parse(text) as Schema["SyncBatch"];
      const operation = batch.operations?.[0];
      if (
        batch.operations?.length !== 1 ||
        !operation ||
        !uuid.test(operation.id) ||
        typeof batch.device_id !== "string" ||
        !uuid.test(batch.device_id) ||
        Object.keys(batch).sort().join(",") !== "device_id,operations" ||
        Object.keys(operation).sort().join(",") !== "action,id"
      )
        throw new Error("The bookmark mutation is not one owned operation");
      const action = operation.action;
      if (!original) {
        if (
          action.kind !== "audiobook_bookmark_create" ||
          !uuid.test(action.bookmark_id) ||
          action.bookmark_id === "00000000-0000-0000-0000-000000000000" ||
          !isDeepStrictEqual(action, {
            kind: "audiobook_bookmark_create",
            audiobook_id: target.detail.playlist.id,
            bookmark_id: action.bookmark_id,
            entry_id: target.entry.entry_id,
            recording_id: target.entry.recording_id,
            position_ms: 35_500,
            label: target.label,
          })
        )
          throw new Error(
            "The bookmark create lost its exact chapter occurrence",
          );
        first = true;
        policy.lose(request);
      } else if (counts.replay === 0) {
        if (text !== original.text)
          throw new Error("The bookmark retry changed its original operation");
      } else {
        const created = original.batch.operations[0].action;
        if (
          counts.remove !== 0 ||
          created.kind !== "audiobook_bookmark_create" ||
          operation.id === original.batch.operations[0].id ||
          batch.device_id !== original.batch.device_id ||
          !isDeepStrictEqual(action, {
            kind: "audiobook_bookmark_remove",
            audiobook_id: target.detail.playlist.id,
            bookmark_id: created.bookmark_id,
          })
        )
          throw new Error("The bookmark remove changed another marker");
      }
    } else if (request.postData() !== null || counts.held_reads >= 2) {
      throw new Error("The bookmark read exceeded its exact workflow");
    }
    let response: Awaited<ReturnType<typeof route.fetch>> | null = null;
    try {
      response = await route.fetch({
        timeout: 10_000,
        maxRetries: 0,
        maxRedirects: 0,
      });
      if (closed) {
        await route.abort("aborted");
        return;
      }
      const bytes = await response.body();
      if (closed) {
        await route.abort("aborted");
        return;
      }
      if (
        response.status() !== 200 ||
        bytes.length > 524_288 ||
        !response.headers()["content-type"]?.startsWith("application/json") ||
        !response.headers()["cache-control"]?.includes("no-store")
      )
        throw new Error(
          "The bookmark server response is not bounded and private",
        );
      if (batch) {
        if (
          bytes.length > 4096 ||
          !isDeepStrictEqual(JSON.parse(bytes.toString()), {
            acknowledged: [batch.operations[0].id],
          } satisfies Schema["SyncReceipt"])
        )
          throw new Error(
            "The bookmark acknowledgement did not match its operation",
          );
        if (first) {
          original = { text, batch, receipt: bytes };
          counts.create++;
          await route.abort("failed");
          return;
        }
        if (counts.replay === 0) {
          if (!original || !bytes.equals(original.receipt))
            throw new Error("The bookmark replay acknowledgement changed");
          counts.replay++;
        } else {
          removal = batch;
          counts.remove++;
        }
      } else {
        if (!original)
          throw new Error("The bookmark read preceded its accepted operation");
        const detail = JSON.parse(
          bytes.toString(),
        ) as Schema["AudiobookDetail"];
        const action = original.batch.operations[0].action;
        if (action.kind !== "audiobook_bookmark_create")
          throw new Error("The accepted bookmark identity is missing");
        const bookmarks =
          counts.remove === 0
            ? [
                {
                  id: action.bookmark_id,
                  audiobook_id: action.audiobook_id,
                  entry_id: action.entry_id,
                  recording_id: action.recording_id,
                  position_ms: action.position_ms,
                  label: action.label,
                } satisfies Schema["AudiobookBookmark"],
              ]
            : [];
        if (!isDeepStrictEqual(detail, { ...target.detail, bookmarks }))
          throw new Error("The bookmark read changed its owned book or marker");
        const number = ++counts.held_reads;
        await new Promise<void>((resolve) => releases.set(number, resolve));
        releases.delete(number);
      }
      if (closed) await route.abort("aborted");
      else await route.fulfill({ response, body: bytes });
    } finally {
      if (response) await response.dispose();
    }
  }
  const handler = async (route: Route) => {
    const running = deliver(route)
      .catch(async () => {
        counts.invalid++;
        await route.abort("failed").catch(() => counts.invalid++);
      })
      .finally(() => pending.delete(running));
    pending.add(running);
    await running;
  };
  await context.route("**/*", handler);
  return {
    counts,
    arm(
      account: string,
      detail: Schema["AudiobookDetail"],
      entry: Schema["PlaylistEntry"],
      label: string,
    ) {
      if (
        target ||
        detail.bookmarks.length !== 0 ||
        !entry.recording_id ||
        !entry.available
      )
        throw new Error(
          "The bookmark delivery target is not a fresh owned chapter",
        );
      target = { account, detail, entry, label };
    },
    creation() {
      if (!original) throw new Error("The accepted bookmark create is missing");
      return original.batch;
    },
    removal() {
      if (!removal) throw new Error("The accepted bookmark remove is missing");
      return removal;
    },
    release(number: number) {
      const release = releases.get(number);
      if (!release) throw new Error("The owned bookmark read is not held");
      release();
    },
    async close() {
      closed = true;
      for (const release of releases.values()) release();
      while (pending.size) await Promise.allSettled([...pending]);
      await context.unroute("**/*", handler);
    },
  };
}
