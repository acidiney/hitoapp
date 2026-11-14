export const ACQUISITION_PASSES = 3;
export const ACQUISITION_DEADLINE_MS = 15_000;
const ORIGINAL_RESERVE_MS = 2_000;
const PASS_BACKOFF_MS = [500, 1000];

export type AcquisitionPass = { attempt: number; of: number };

export class BrowserAcquisition {
  private pass = 1;
  private final = false;
  private readonly started = performance.now();
  private deadline: ReturnType<typeof setTimeout> | null;
  private backoff: ReturnType<typeof setTimeout> | null = null;
  private resume: ((continued: boolean) => void) | null = null;
  private expired = false;

  constructor(
    readonly entry: string,
    private readonly expire: () => void,
  ) {
    this.deadline = setTimeout(() => this.expireNow(), ACQUISITION_DEADLINE_MS);
  }

  get overdue() {
    return performance.now() - this.started >= ACQUISITION_DEADLINE_MS;
  }

  expireNow() {
    if (this.expired) return;
    this.expired = true;
    this.stop();
    this.expire();
  }

  get attempt() {
    return this.pass;
  }

  get elapsedMs() {
    return Math.round(performance.now() - this.started);
  }

  negotiationBudgetMs() {
    const remaining =
      ACQUISITION_DEADLINE_MS - (performance.now() - this.started);
    return Math.max(0, remaining - ORIGINAL_RESERVE_MS);
  }

  finalPass() {
    this.final = true;
  }

  nextPass(): AcquisitionPass | null {
    if (this.deadline === null || this.final || this.pass >= ACQUISITION_PASSES)
      return null;
    this.pass++;
    return { attempt: this.pass, of: ACQUISITION_PASSES };
  }

  waitForPass(): Promise<boolean> {
    if (this.deadline === null) return Promise.resolve(false);
    const delay = PASS_BACKOFF_MS[this.pass - 2] ?? 0;
    return new Promise((resolve) => {
      this.resume = resolve;
      this.backoff = setTimeout(() => {
        this.backoff = null;
        this.resume = null;
        resolve(true);
      }, delay);
    });
  }

  stop() {
    if (this.deadline !== null) clearTimeout(this.deadline);
    this.deadline = null;
    if (this.backoff !== null) clearTimeout(this.backoff);
    this.backoff = null;
    const resume = this.resume;
    this.resume = null;
    resume?.(false);
  }
}
