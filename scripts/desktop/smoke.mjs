import assert from "node:assert/strict";
import { spawn, execFileSync } from "node:child_process";
import {
  mkdtemp,
  mkdir,
  writeFile,
  rm,
  copyFile,
  access,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  start,
  open,
  endSession,
  stop,
  logs,
  wait,
  request,
  execute,
  exists,
  type,
  click,
  clickText,
} from "./webdriver.mjs";

const temporary = await mkdtemp(join(tmpdir(), "hito-desktop-"));
const config = join(temporary, "config");
await mkdir(config);
await writeFile(
  join(config, "user-dirs.dirs"),
  `XDG_MUSIC_DIR="${temporary}/music"\n`,
);
await mkdir(join(temporary, "music"));
await copyFile(
  "/workspace/fixtures/e2e-generated/Music/Fixture 440 Hz.wav",
  join(temporary, "music", "fixture.wav"),
);
const runtime = join(temporary, "runtime");
await mkdir(runtime, { mode: 0o700 });
const socket = join(temporary, "pulse.sock");
const sound = spawn(
  "pulseaudio",
  [
    "-n",
    "--daemonize=no",
    "--exit-idle-time=-1",
    "--use-pid-file=no",
    "--log-target=stderr",
    "--load=module-null-sink sink_name=hito-test",
    `--load=module-native-protocol-unix socket=${socket} auth-anonymous=1`,
  ],
  { env: { ...process.env, XDG_RUNTIME_DIR: runtime }, stdio: "ignore" },
);
const manager = spawn("openbox", ["--sm-disable"], { stdio: "ignore" });
start({
  XDG_CONFIG_HOME: config,
  XDG_DATA_HOME: join(temporary, "data"),
  XDG_RUNTIME_DIR: runtime,
  PULSE_SERVER: `unix:${socket}`,
});
async function close(origin) {
  const window = execFileSync(
    "xdotool",
    ["search", "--onlyvisible", "--name", "^Hito$"],
    { encoding: "utf8", timeout: 5000 },
  );
  assert.match(window.trim(), /^\d+$/);
  execFileSync("wmctrl", ["-ic", window.trim()], {
    timeout: 5000,
  });
  await wait(async () => {
    try {
      await fetch(`${origin}/health`, { signal: AbortSignal.timeout(1000) });
      return false;
    } catch {
      return true;
    }
  }, "local backend shutdown after closing the window");
  await wait(
    async () => logs().includes('"event":"desktop_stopped"'),
    "the joined desktop shutdown event",
  );
  await endSession();
}
try {
  await wait(async () => {
    try {
      await access(socket);
      return true;
    } catch {
      return false;
    }
  }, "the clocked test audio sink");
  await wait(async () => {
    try {
      return (await request("GET", "/status")).ready;
    } catch (error) {
      if (error instanceof TypeError) return false;
      throw error;
    }
  }, "the WebDriver service");
  await open();
  await wait(
    () =>
      execute(
        'return document.querySelector("input[autocomplete=username]") !== null',
      ),
    "the local setup form",
  );
  assert.equal(await execute("return location.hostname"), "127.0.0.1");
  assert.equal(await execute("return location.search"), "");
  assert.equal(
    await execute('return document.cookie.includes("hito_desktop")'),
    false,
  );
  await type('input[autocomplete="username"]', "owner");
  await type('input[type="password"]', "desktop fixture account password");
  await click('form button[type="submit"],form button:not([type])');
  await wait(
    () =>
      execute(
        'return document.querySelector("h1")?.textContent === "Your library"',
      ),
    "the standalone library",
  );
  await click('nav[aria-label="Main navigation"] a[href="/settings"]');
  await wait(
    () =>
      execute(
        'return !!document.querySelector(".music-folders-heading button")',
      ),
    "music folder settings",
  );
  await clickText("Add music folder");
  await wait(
    () => exists('button[aria-label="Open Music folder"]'),
    "the owned Music directory",
  );
  await click('button[aria-label="Open Music folder"]');
  await wait(
    () =>
      execute(
        'return [...document.querySelectorAll("dialog button")].some(button => button.textContent === "Use this folder" && !button.disabled)',
      ),
    "the folder selection",
  );
  await clickText("Use this folder");
  await wait(
    () =>
      execute(
        'return document.querySelector(".music-folder-row")?.textContent.includes("1 file read")',
      ),
    "the local scan",
  );
  await click('nav[aria-label="Main navigation"] a[href="/library"]');
  await wait(
    () => exists('button[aria-label="Play Fixture 440 Hz"]'),
    "the indexed local track",
  );
  await execute(
    'window.__desktopEvents = []; for(const name of ["playing","ended","error","emptied"]) document.querySelector("audio").addEventListener(name,()=>window.__desktopEvents.push(name));',
  );
  await click('button[aria-label="Play Fixture 440 Hz"]');
  await wait(
    () => execute('return document.querySelector("audio")?.currentTime > 0.2'),
    "decoded WebKit playback with the clocked test sink",
  );
  const source = await execute('return document.querySelector("audio").src');
  const position = await execute(
    'return document.querySelector("audio").currentTime',
  );
  await click('nav[aria-label="Main navigation"] a[href="/albums"]');
  await wait(
    () =>
      execute('return document.querySelector("h1")?.textContent === "Albums"'),
    "album navigation",
  );
  assert.equal(
    await execute('return document.querySelector("audio").src'),
    source,
  );
  await wait(
    () =>
      execute(
        `return document.querySelector("audio").currentTime > ${position + 0.2}`,
      ),
    "uninterrupted playback during navigation",
  );
  execFileSync(
    "import",
    [
      "-window",
      "root",
      "/workspace/test-results/desktop/standalone-library.png",
    ],
    { timeout: 5000 },
  );
  assert.equal(
    await execute(
      'return (async () => {const invoke=window.__TAURI_INTERNALS__?.invoke; if(!invoke) return true; try {await invoke("plugin:app|version");return false;} catch {return true;}})()',
    ),
    true,
  );
  assert.equal(
    await execute('return document.querySelectorAll("audio").length'),
    1,
  );
  const origin = await execute("return location.origin");
  await close(origin);
  const previousStops = logs().split('"event":"desktop_stopped"').length;
  await open();
  await wait(
    () =>
      execute(
        'return document.querySelector("h1")?.textContent === "Your library"',
      ),
    "the saved account after restart",
  );
  await wait(
    () => exists('button[aria-label="Play Fixture 440 Hz"]'),
    "the persisted local library",
  );
  const restartedOrigin = await execute("return location.origin");
  assert.notEqual(restartedOrigin, origin);
  await close(restartedOrigin);
  assert.equal(
    logs().split('"event":"desktop_stopped"').length,
    previousStops + 1,
  );
  assert.equal(logs().includes("desktop fixture account password"), false);
  assert.equal(logs().includes("ticket="), false);
  console.log(
    "Standalone Tauri setup, scan, WebKit playback, navigation, persistence and joined shutdown passed.",
  );
} catch (error) {
  try {
    const state = await execute(
      'return (()=>{const a=document.querySelector("audio");return a ? {paused:a.paused,ended:a.ended,time:a.currentTime,duration:a.duration,ready:a.readyState,error:a.error?.code,events:window.__desktopEvents} : null})()',
    );
    console.error("WebKit audio diagnostic", state);
  } catch {}
  {
    try {
      execFileSync(
        "import",
        ["-window", "root", "/workspace/test-results/desktop/failure.png"],
        { timeout: 5000 },
      );
    } catch {}
  }
  throw error;
} finally {
  await stop();
  manager.kill("SIGTERM");
  sound.kill("SIGTERM");
  await writeFile("/workspace/test-results/desktop/driver.log", logs(), {
    mode: 0o600,
  });
  await rm(temporary, { recursive: true, force: true });
}
