export const PROGRESS_WAITING_MESSAGE =
  "Listening progress is waiting to be saved. Playback paused.";

type ProgressSource = {
  persist(): boolean;
  observe(): boolean;
  retry(): boolean;
};

type Transport = { isPaused(): boolean; pause(): void };

export class BrowserProgressGuard {
  private blocking = false;

  constructor(
    private source: ProgressSource,
    private transport: Transport,
    private fail: (message: string) => void,
  ) {}

  get stopping() {
    return this.blocking;
  }

  persist() {
    if (this.source.persist()) return true;
    this.block();
    return false;
  }

  observe() {
    if (!this.source.observe()) this.block();
  }

  retry() {
    return this.source.retry();
  }

  private block() {
    if (this.blocking) return;
    this.blocking = true;
    try {
      if (!this.transport.isPaused()) this.transport.pause();
      this.fail(PROGRESS_WAITING_MESSAGE);
    } finally {
      this.blocking = false;
    }
  }
}
