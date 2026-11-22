import type { Schema } from "../../../shared/schema";
import { HttpError } from "../../../shared/http";
import { log } from "../../../shared/log";
import { upcomingApi } from "../api/upcomingApi";

export type Upcoming = { current: string | null; next: string[] };

type Scope = { signal: AbortSignal; active(): boolean };
type Wanted = {
  key: string;
  body: Schema<"UpcomingRecordings">;
  since: number;
};

const QUIET_MS = 1000;
const LONGEST_WAIT_MS = 3000;
const SPACING_MS = 2000;
const DEFAULT_RETRY_AFTER_SECONDS = 60;
const MAX_UPCOMING = 2;
const TAG_KEY = "hito:playback-client";
const TAG_PATTERN = /^web-[A-Za-z0-9]{12}$/;
const NOTHING = JSON.stringify([null, []]);

let tabTag: string | null = null;

export function upcomingClientTag() {
  try {
    const stored = sessionStorage.getItem(TAG_KEY);
    if (stored && TAG_PATTERN.test(stored)) return stored;
  } catch {
    log.debug("playback.upcoming_tag_unstored");
  }
  tabTag ??= randomTag();
  try {
    sessionStorage.setItem(TAG_KEY, tabTag);
  } catch {
    log.debug("playback.upcoming_tag_unstored");
  }
  return tabTag;
}

function randomTag() {
  const alphabet =
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
  const bytes = crypto.getRandomValues(new Uint8Array(12));
  return `web-${Array.from(bytes, (byte) => alphabet[byte % alphabet.length]).join("")}`;
}

export class BrowserUpcoming {
  private latest = NOTHING;
  private sent = NOTHING;
  private wanted: Wanted | null = null;
  private sending = false;
  private lastSentAt = Number.NEGATIVE_INFINITY;
  private blockedUntil = 0;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private closed = new AbortController();

  constructor(
    private account: string,
    private capture: () => Scope,
  ) {}

  activate() {
    if (this.closed.signal.aborted) this.closed = new AbortController();
  }

  announce(upcoming: Upcoming) {
    if (this.closed.signal.aborted) return;
    const next = upcoming.next.slice(0, MAX_UPCOMING);
    const key = JSON.stringify([upcoming.current, next]);
    if (key === this.latest) return;
    this.latest = key;
    const now = Date.now();
    this.wanted = {
      key,
      body: {
        client: upcomingClientTag(),
        current: upcoming.current,
        recording_ids: next,
      },
      since: this.wanted?.since ?? now,
    };
    this.schedule(now);
  }

  close() {
    this.closed.abort();
    this.clearTimer();
    this.wanted = null;
    this.latest = this.sent;
  }

  private clearTimer() {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
  }

  private schedule(now = Date.now()) {
    this.clearTimer();
    const wanted = this.wanted;
    if (!wanted || this.sending || this.closed.signal.aborted) return;
    if (wanted.key === this.sent) {
      this.wanted = null;
      return;
    }
    const settled = Math.min(now + QUIET_MS, wanted.since + LONGEST_WAIT_MS);
    const at = Math.max(
      settled,
      this.lastSentAt + SPACING_MS,
      this.blockedUntil,
    );
    this.timer = setTimeout(() => void this.send(), Math.max(0, at - now));
  }

  private async send() {
    this.timer = null;
    const wanted = this.wanted;
    const scope = this.capture();
    if (!wanted || !scope.active() || this.closed.signal.aborted) return;
    this.sending = true;
    this.lastSentAt = Date.now();
    const started = performance.now();
    const signal = AbortSignal.any([scope.signal, this.closed.signal]);
    let reason = "accepted";
    try {
      await upcomingApi.announce(this.account, wanted.body, signal);
      this.sent = wanted.key;
      if (this.wanted === wanted) this.wanted = null;
    } catch (error) {
      if (error instanceof HttpError && error.status === 429) {
        reason = "limited";
        this.blockedUntil =
          Date.now() + (error.retryAfter || DEFAULT_RETRY_AFTER_SECONDS) * 1000;
      } else {
        reason = signal.aborted ? "cancelled" : "failed";
        if (this.wanted === wanted) this.wanted = null;
      }
    } finally {
      this.sending = false;
      log.debug("playback.upcoming_announced", {
        reason,
        count: wanted.body.recording_ids.length,
        mode: wanted.body.current === null ? "stopped" : "playing",
        duration_ms: Math.max(0, Math.round(performance.now() - started)),
      });
      if (scope.active()) this.schedule();
    }
  }
}
