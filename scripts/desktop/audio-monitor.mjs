import { spawn } from "node:child_process";
import { once } from "node:events";

export function monitorAudio(socket) {
  const child = spawn(
    "parec",
    [
      "--raw",
      "--format=float32le",
      "--rate=48000",
      "--channels=2",
      "--device=hito-test.monitor",
    ],
    {
      env: { ...process.env, PULSE_SERVER: `unix:${socket}` },
      stdio: ["ignore", "pipe", "ignore"],
    },
  );
  let samples = Buffer.alloc(0);
  let received = 0;
  let updated = 0;
  let failed = false;
  const exited = once(child, "exit").catch(() => {});
  child.on("error", () => {
    failed = true;
  });
  child.stdout.on("data", (chunk) => {
    samples = Buffer.concat([samples, chunk]);
    received += chunk.length;
    updated = Date.now();
    if (samples.length > 38400)
      samples = samples.subarray(samples.length - 38400);
  });
  return {
    energy() {
      if (failed) throw new Error("The test audio monitor failed.");
      if (Date.now() - updated > 1000 || samples.length < 38400 || received % 4)
        return null;
      let sum = 0;
      for (let offset = 0; offset < samples.length; offset += 4) {
        const value = samples.readFloatLE(offset);
        sum += value * value;
      }
      return sum / (samples.length / 4);
    },
    async close() {
      if (child.exitCode !== null || child.signalCode !== null) return;
      child.kill("SIGTERM");
      const timer = setTimeout(() => child.kill("SIGKILL"), 2000);
      try {
        await exited;
      } finally {
        clearTimeout(timer);
      }
    },
  };
}
