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
import { monitorAudio } from "./audio-monitor.mjs";
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
const alsa = join(temporary, "asound.conf");
await writeFile(
  alsa,
  "pcm.!default { type pulse }\nctl.!default { type pulse }\n",
);
const sound = spawn(
  "pulseaudio",
  [
    "-n",
    "--daemonize=no",
    "--exit-idle-time=-1",
    "--use-pid-file=no",
    "--log-target=stderr",
    "--load=module-null-sink sink_name=hito-test rate=48000 channels=2",
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
  ALSA_CONFIG_PATH: alsa,
});
let monitor;
const playerState = () =>
  execute('return fetch("/api/v1/player").then(r=>r.json())');
const uiPosition = () =>
  execute(
    'return Number(document.querySelector("input[aria-label=Seek]")?.value ?? 0)',
  );
const noBrowserAudio = async () =>
  assert.equal(
    await execute('return document.querySelectorAll("audio").length'),
    0,
  );
const audible = () => monitor.energy() > 0.001;
const silent = () => {
  const energy = monitor.energy();
  return energy !== null && energy < 0.000001;
};
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
  monitor = monitorAudio(socket);
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
  await noBrowserAudio();
  await click('button[aria-label="Play Fixture 440 Hz"]');
  await wait(
    async () => (await uiPosition()) > 0.2 && audible(),
    "native UI position and actual CPAL output",
  );
  const initial = await playerState();
  assert.equal(initial.native, true);
  assert.equal(initial.state.playing, true);
  const position = await uiPosition();
  await click('nav[aria-label="Main navigation"] a[href="/albums"]');
  await wait(
    () =>
      execute('return document.querySelector("h1")?.textContent === "Albums"'),
    "album navigation",
  );
  assert.equal(
    (await playerState()).state.current.id,
    initial.state.current.id,
  );
  assert.equal((await playerState()).state.source_id, initial.state.source_id);
  await wait(
    async () => (await uiPosition()) > position + 0.2 && audible(),
    "uninterrupted playback during navigation",
  );
  const beforeReload = await uiPosition();
  await execute("location.reload(); return true;");
  await wait(
    async () => (await uiPosition()) > beforeReload + 0.2 && audible(),
    "native player reattachment after WebView reload",
  );
  assert.equal(
    (await playerState()).state.current.id,
    initial.state.current.id,
  );
  assert.equal((await playerState()).state.source_id, initial.state.source_id);
  await noBrowserAudio();
  await click('button[aria-label="Pause"]');
  await wait(async () => {
    const state = (await playerState()).state;
    return !state.desired_playing && !state.playing && silent();
  }, "native pause and silent output");
  await wait(
    () =>
      execute(
        'return fetch("/api/v1/activity").then(r=>r.json()).then(a=>a.history.some(h=>h.position_ms > 200))',
      ),
    "Rust-owned listening progress persistence",
  );
  const paused = (await playerState()).state;
  await click('button[aria-label="Shuffle"]');
  await click('button[aria-label="Repeat: off"]');
  await wait(async () => {
    const state = (await playerState()).state;
    return (
      state.shuffle &&
      state.repeat === "all" &&
      !state.desired_playing &&
      silent()
    );
  }, "native shuffle and repeat without unpausing");
  await execute("location.reload(); return true;");
  await wait(async () => {
    const state = (await playerState()).state;
    return (
      state.shuffle &&
      state.repeat === "all" &&
      !state.desired_playing &&
      silent() &&
      (await exists('button[aria-label="Repeat: all"]'))
    );
  }, "native mode reattachment after reload");
  for (const [before, after] of [
    ["all", "one"],
    ["one", "off"],
  ]) {
    await click(`button[aria-label="Repeat: ${before}"]`);
    await wait(
      async () => (await playerState()).state.repeat === after,
      `native repeat ${after}`,
    );
  }
  await click('button[aria-label="Previous track"]');
  await wait(async () => {
    const state = (await playerState()).state;
    return (
      !state.playing &&
      !state.desired_playing &&
      state.phase === "ready" &&
      (await uiPosition()) < 0.05 &&
      silent()
    );
  }, "native Previous restarts without unpausing");
  const returned = (await playerState()).state;
  assert.equal(returned.current.id, paused.current.id);
  assert.equal(returned.revision, paused.revision);
  assert.equal(returned.selection, paused.selection);
  await click('.player button[aria-label="Play"]');
  await wait(
    async () =>
      (await playerState()).state.playing &&
      (await uiPosition()) > 0.3 &&
      audible(),
    "native resume and actual output",
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
  await noBrowserAudio();
  const origin = await execute("return location.origin");
  await close(origin);
  await wait(silent, "silent output after joined native shutdown");
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
  await noBrowserAudio();
  assert.equal((await playerState()).state.queue_len, 0);
  assert.equal(
    await execute(
      'return fetch("/api/v1/activity").then(r=>r.json()).then(a=>a.history.some(h=>h.position_ms > 200))',
    ),
    true,
  );
  await close(restartedOrigin);
  assert.equal(
    logs().split('"event":"desktop_stopped"').length,
    previousStops + 1,
  );
  assert.equal(logs().includes("desktop fixture account password"), false);
  assert.equal(logs().includes("ticket="), false);
  console.log(
    "Standalone Tauri setup, scan, native output, navigation, reload, Previous, shuffle/repeat, pause/resume, progress, persistence and joined shutdown passed.",
  );
} catch (error) {
  try {
    const state = await execute(
      'return fetch("/api/v1/player").then(r=>r.json()).then(p=>({native:p.native,phase:p.state?.phase,playing:p.state?.playing,desired:p.state?.desired_playing,queue:p.state?.queue_len,position:Number(document.querySelector("input[aria-label=Seek]")?.value??0),audio:document.querySelectorAll("audio").length,connection:document.querySelector(".playback-status")?.textContent}))',
    );
    console.error("Native player diagnostic", state, {
      output: monitor?.energy(),
    });
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
  await monitor?.close();
  manager.kill("SIGTERM");
  sound.kill("SIGTERM");
  await writeFile("/workspace/test-results/desktop/driver.log", logs(), {
    mode: 0o600,
  });
  await rm(temporary, { recursive: true, force: true });
}
