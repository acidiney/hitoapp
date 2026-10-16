import { execute } from "./webdriver.mjs";

function install({ positionMs, forceZero }) {
  const original = window.fetch;
  const controller = new AbortController();
  const tasks = [];
  const proof = { armed: false, playingPosition: null, error: null };
  let entry;
  let selection;
  let playing = false;
  let position = null;
  const current = (frame) =>
    frame.selection === selection &&
    (frame.entry ?? frame.current?.id) === entry;
  function check() {
    if (!playing || position === null || position === 0) return;
    proof.playingPosition ??= position;
    if (position < positionMs)
      proof.error =
        "Saved playback presented a position below the saved cursor";
  }
  function observe(kind) {
    let ready;
    const initial = new Promise((resolve) => {
      ready = resolve;
    });
    const task = (async () => {
      const response = await original(`/api/v1/player/${kind}/events`, {
        method: "POST",
        headers: observerHeaders,
        body: JSON.stringify({ lease: observerLease }),
        signal: controller.signal,
      });
      if (!response.ok || !response.body)
        throw new Error("Observation unavailable");
      const reader = response.body.getReader();
      const decoder = new TextDecoder();
      let pending = "";
      try {
        while (true) {
          const chunk = await reader.read();
          if (chunk.done) throw new Error("Observation closed");
          pending += decoder.decode(chunk.value, { stream: true });
          let boundary;
          while ((boundary = pending.indexOf("\n\n")) >= 0) {
            const event = pending.slice(0, boundary);
            pending = pending.slice(boundary + 2);
            const data = event
              .split("\n")
              .filter((line) => line.startsWith("data:"))
              .map((line) => line.slice(5).trimStart())
              .join("\n");
            if (!data) continue;
            const frame = JSON.parse(data);
            if (current(frame)) {
              if (kind === "state") playing = frame.playing;
              else position = frame.position_ms;
              check();
            }
            ready();
          }
        }
      } finally {
        reader.releaseLock();
      }
    })().catch(() => {
      if (!controller.signal.aborted)
        proof.error ??= "Saved playback observation failed";
      ready();
    });
    tasks.push(task);
    return initial;
  }
  let observerHeaders;
  let observerLease;
  const intercept = async (url, options) => {
    if (url === "/api/v1/player/commands") {
      const command = JSON.parse(options.body);
      if (command.action.kind === "resume_saved") {
        window.fetch = original;
        const inspected = await original("/api/v1/player").then((response) =>
          response.json(),
        );
        entry = inspected.state.current.id;
        selection = inspected.state.selection;
        observerLease = command.lease;
        observerHeaders = options.headers;
        await Promise.all([observe("state"), observe("position")]);
        proof.armed = true;
        if (forceZero) command.action = { kind: "resume" };
        return original(
          url,
          forceZero ? { ...options, body: JSON.stringify(command) } : options,
        );
      }
    }
    return original(url, options);
  };
  window.fetch = intercept;
  window.__hitoSavedStart = {
    snapshot: () => ({ ...proof }),
    async stop() {
      if (window.fetch === intercept) window.fetch = original;
      controller.abort();
      await Promise.all(tasks);
      delete window.__hitoSavedStart;
    },
  };
}

export async function armSavedStart(positionMs, forceZero = false) {
  await execute(
    `return (${install.toString()})(${JSON.stringify({ positionMs, forceZero })});`,
  );
}
export const savedStartObservation = () =>
  execute("return window.__hitoSavedStart.snapshot();");
export const stopSavedStartObservation = () =>
  execute("return window.__hitoSavedStart.stop();");
