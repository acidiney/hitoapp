import { spawn } from "node:child_process";
import { setTimeout as delay } from "node:timers/promises";

let output = "";
let session;
let driver;
export function start(environment) {
  driver = spawn("tauri-driver", ["--port", "4444", "--native-port", "4445"], {
    env: { ...process.env, ...environment },
    detached: true,
    stdio: ["ignore", "pipe", "pipe"],
  });
  driver.stdout.on("data", (chunk) => (output += chunk.toString()));
  driver.stderr.on("data", (chunk) => (output += chunk.toString()));
}
export const logs = () => output;
export async function request(method, path, body, timeout = 30000) {
  const response = await fetch(`http://127.0.0.1:4444${path}`, {
    method,
    headers: { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(timeout),
  });
  const value = await response.json();
  if (!response.ok)
    throw new Error(
      `WebDriver failed: ${value.value?.error ?? response.status}`,
    );
  return value.value;
}
export async function wait(check, label) {
  const deadline = Date.now() + 15000;
  while (Date.now() < deadline) {
    if (await check()) return;
    await delay(100);
  }
  throw new Error(`Timed out waiting for ${label}`);
}
export const exists = (selector) =>
  execute(`return !!document.querySelector(${JSON.stringify(selector)})`);
export const execute = (script) =>
  request("POST", `/session/${session}/execute/sync`, { script, args: [] });
async function find(selector, using = "css selector") {
  const value = await request("POST", `/session/${session}/element`, {
    using,
    value: selector,
  });
  return value["element-6066-11e4-a52e-4f735466cecf"];
}
export async function type(selector, text) {
  const element = await find(selector);
  await request("POST", `/session/${session}/element/${element}/value`, {
    text,
  });
}
export async function click(selector) {
  const element = await find(selector);
  await request("POST", `/session/${session}/element/${element}/click`, {});
}
export async function clickText(text) {
  const element = await find(`//button[normalize-space()='${text}']`, "xpath");
  await request("POST", `/session/${session}/element/${element}/click`, {});
}
export async function open() {
  const created = await request("POST", "/session", {
    capabilities: {
      alwaysMatch: {
        "tauri:options": {
          application: "/build/desktop-target/debug/hito-desktop",
        },
      },
    },
  });
  session = created.sessionId;
}
export async function endSession() {
  if (!session) return;
  try {
    await request("DELETE", `/session/${session}`, undefined, 3000);
  } finally {
    session = undefined;
  }
}
export async function stop() {
  try {
    await endSession();
  } catch {}
  if (driver) {
    try {
      process.kill(-driver.pid, "SIGTERM");
    } catch {}
    await Promise.race([
      new Promise((resolve) => driver.once("exit", resolve)),
      new Promise((resolve) => {
        const timer = setTimeout(resolve, 3000);
        timer.unref();
      }),
    ]);
    try {
      process.kill(-driver.pid, "SIGKILL");
    } catch {}
  }
}
