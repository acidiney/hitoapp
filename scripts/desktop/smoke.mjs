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
import { readdirSync, readFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { monitorAudio } from "./audio-monitor.mjs";
import {
  armSavedStart,
  savedStartObservation,
  stopSavedStartObservation,
} from "./saved-start-observer.mjs";
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
const stubs = join(temporary, "stubs");
const openerLog = join(temporary, "opener.log");
const followMarker = join(temporary, "follow-provider");
const followLog = join(temporary, "follow.log");
await mkdir(stubs);
await writeFile(
  join(stubs, "follow.mjs"),
  `import { appendFileSync } from "node:fs";
const response = await fetch(process.argv[2]);
const page = await response.text();
appendFileSync(${JSON.stringify(followLog)}, JSON.stringify({ status: response.status, origin: new URL(response.url).origin, path: new URL(response.url).pathname, authorized: page.includes("Provider authorized"), noReturnLink: !page.includes("href=") }) + "\\n");
`,
);
await writeFile(
  join(stubs, "xdg-open"),
  `#!/bin/sh\nprintf 'argc=%s\\n' "$#" >> "${openerLog}"\nfor argument in "$@"; do printf 'url=%s\\n' "$argument" >> "${openerLog}"; done\nif [ -f "${followMarker}" ]; then node "${join(stubs, "follow.mjs")}" "$1" >/dev/null 2>&1 & fi\n`,
  { mode: 0o755 },
);
const manager = spawn("openbox", ["--sm-disable"], { stdio: "ignore" });
start({
  PATH: `${stubs}:${process.env.PATH}`,
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
const connectionNoticeShown = () =>
  execute('return document.querySelector(".connection-notice") !== null');
async function verifyWithoutNetwork() {
  assert.deepEqual(readdirSync("/sys/class/net"), ["lo"]);
  await assert.rejects(
    fetch("http://1.1.1.1/", { signal: AbortSignal.timeout(3000) }),
  );
  assert.equal(await connectionNoticeShown(), false);
  const outcome = await execute(`
    return (async () => {
      const account = await fetch('/api/v1/me').then(r => r.json());
      const headers = {
        'content-type': 'application/json',
        'x-hito-request': '1',
        'x-hito-account': account.id
      };
      const started = performance.now();
      const offered = await fetch('/api/v1/plugins/services', {
        method: 'POST',
        headers,
        body: JSON.stringify({ name: 'Unreachable service', base_url: 'http://127.0.0.1:9' })
      });
      let previewed = null;
      if (offered.status === 201) {
        const service = await offered.json();
        const response = await fetch('/api/v1/plugins/preview', {
          method: 'POST',
          headers,
          body: JSON.stringify({ service_id: service.id, service_revision: service.revision })
        });
        previewed = { status: response.status, error: typeof (await response.json()).error };
      }
      const plugins = await fetch('/api/v1/plugins').then(r => r.status);
      return {
        offered: offered.status,
        previewed,
        plugins,
        elapsed: performance.now() - started
      };
    })();
  `);
  assert.ok(outcome.elapsed < 20000, "provider failure must not hang");
  assert.equal(outcome.plugins, 200);
  if (outcome.previewed) {
    assert.ok(outcome.previewed.status >= 400);
    assert.equal(outcome.previewed.error, "string");
  } else assert.ok(outcome.offered >= 400 && outcome.offered < 500);
  console.log("providers unavailable without a network", outcome);
  await click('nav[aria-label="Main navigation"] a[href="/discover"]');
  await wait(
    () => execute('return document.querySelector("h1") !== null'),
    "Discover without providers",
  );
  assert.equal(await connectionNoticeShown(), false);
  await click('nav[aria-label="Main navigation"] a[href="/library"]');
  await wait(
    () => exists('button[aria-label="Play Fixture 440 Hz"]'),
    "the local library after the provider checks",
  );
}
const openerCalls = () => {
  try {
    return readFileSync(openerLog, "utf8").split("\n").filter(Boolean);
  } catch {
    return [];
  }
};
const followed = () => {
  try {
    return readFileSync(followLog, "utf8")
      .split("\n")
      .filter(Boolean)
      .map((line) => JSON.parse(line));
  } catch {
    return [];
  }
};
async function verifyBrowserLauncher() {
  let appRequests = 0;
  const authorizations = [];
  const exchanges = [];
  const provider = createServer((incoming, outgoing) => {
    const url = new URL(incoming.url ?? "/", "http://provider.test");
    const port = provider.address().port;
    if (url.pathname === "/authorize") {
      if (/HitoDesktop/.test(incoming.headers["user-agent"] ?? "")) appRequests += 1;
      authorizations.push(url);
      const back = new URL(url.searchParams.get("redirect_uri"));
      back.searchParams.set("code", "desktop-smoke-code");
      back.searchParams.set("state", url.searchParams.get("state"));
      outgoing.writeHead(302, { location: back.toString() });
      outgoing.end();
      return;
    }
    if (url.pathname === "/token" && incoming.method === "POST") {
      let body = "";
      incoming.on("data", (chunk) => (body += chunk));
      incoming.on("end", () => {
        exchanges.push(new URLSearchParams(body));
        outgoing.setHeader("content-type", "application/json");
        outgoing.end(
          JSON.stringify({
            access_token: "desktop-smoke-access",
            token_type: "Bearer",
            expires_in: 3600,
            refresh_token: "desktop-smoke-refresh",
            scope: "playlist-read-private",
          }),
        );
      });
      return;
    }
    outgoing.setHeader("content-type", "application/json");
    outgoing.end(
      JSON.stringify({
        id: "org.hito.desktop-launcher-fixture",
        name: "Launcher fixture",
        protocol_version: 1,
        capabilities: ["playlists"],
        authorization: {
          kind: "oauth2_pkce",
          client_id: "launcher-client",
          authorization_endpoint: `http://127.0.0.1:${port}/authorize`,
          token_endpoint: `http://127.0.0.1:${port}/token`,
          scopes: ["playlist-read-private"],
        },
      }),
    );
  });
  await new Promise((resolve) => provider.listen(0, "127.0.0.1", resolve));
  const endpoint = `http://127.0.0.1:${provider.address().port}/authorize`;
  try {
    const installed = await execute(`
      return (async () => {
        const account = await fetch('/api/v1/me').then(r => r.json());
        const headers = {
          'content-type': 'application/json',
          'x-hito-request': '1',
          'x-hito-account': account.id
        };
        const service = await fetch('/api/v1/plugins/services', {
          method: 'POST', headers,
          body: JSON.stringify({ name: 'Launcher fixture', base_url: ${JSON.stringify(`http://127.0.0.1:${provider.address().port}`)} })
        }).then(r => r.json());
        const installation = await fetch('/api/v1/plugins', {
          method: 'POST', headers,
          body: JSON.stringify({ service_id: service.id, service_revision: service.revision })
        });
        return { status: installation.status, ...(await installation.json()) };
      })();
    `);
    assert.equal(installed.status, 201);
    await execute("location.assign('/settings?section=plugins'); return true;");
    await wait(
      () =>
        execute(
          'return [...document.querySelectorAll("button")].some(b => b.textContent === "Connect")',
        ),
      "the provider Connect button",
    );
    const settingsUrl = await execute("return location.href");
    await clickText("Connect");
    await wait(
      async () => openerCalls().length >= 2,
      "the system opener to receive the sign-in page",
    );
    const calls = openerCalls();
    assert.deepEqual(calls.slice(0, 1), ["argc=1"]);
    assert.equal(calls.length, 2);
    const received = new URL(calls[1].slice("url=".length));
    const origin = await execute("return location.origin");
    assert.equal(`${received.origin}${received.pathname}`, endpoint);
    const fixedReturn =
      "http://127.0.0.1:47321/api/v1/plugin-authorization/callback";
    assert.equal(received.searchParams.get("redirect_uri"), fixedReturn);
    assert.notEqual(new URL(fixedReturn).origin, origin);
    assert.equal(received.searchParams.get("code_challenge_method"), "S256");
    assert.equal(received.searchParams.get("client_id"), "launcher-client");
    assert.equal(appRequests, 0);
    assert.equal(await execute("return location.href"), settingsUrl);
    assert.equal(
      await execute(
        'return document.querySelector("[aria-label=\\"Sign-in status\\"]")?.textContent',
      ),
      "Waiting for Launcher fixture…",
    );
    const redacted = new URL(received);
    for (const name of ["state", "code_challenge"])
      redacted.searchParams.set(name, "REDACTED");
    console.log(
      "desktop launcher: system opener invoked once with one argument:",
      redacted.toString(),
    );
    const forged = await execute(`
      return (async () => {
        const account = await fetch('/api/v1/me').then(r => r.json());
        const installations = await fetch('/api/v1/plugins').then(r => r.json());
        const id = installations.installations[0].id;
        const headers = {
          'content-type': 'application/json',
          'x-hito-request': '1',
          'x-hito-account': account.id
        };
        const first = ${JSON.stringify(received.toString())};
        const begun = await fetch('/api/v1/plugins/' + id + '/authorization', {
          method: 'POST', headers,
          body: JSON.stringify({ revision: installations.installations[0].revision })
        }).then(r => r.json());
        const targets = [
          'https://evil.example/authorize',
          begun.authorization_url.replace('/authorize?', '/other?'),
          first,
          'file:///etc/passwd'
        ];
        const statuses = [];
        for (const authorization_url of targets) {
          const response = await fetch('/api/v1/plugins/' + id + '/authorization/' + begun.id + '/launch', {
            method: 'POST', headers, body: JSON.stringify({ authorization_url })
          });
          statuses.push(response.status);
        }
        return statuses;
      })();
    `);
    assert.deepEqual(forged, [422, 422, 422, 422]);
    assert.equal(openerCalls().length, 2);
    console.log("desktop launcher: forged launches for a live attempt refused", forged);
    const callback = await fetch(
      `${origin}/api/v1/plugin-authorization/callback?code=probe&state=probe`,
      { redirect: "manual" },
    );
    console.log(
      "desktop launcher: callback from a browser without the desktop cookie answered",
      callback.status,
    );
    assert.equal(callback.status, 403);
    await clickText("Stop waiting");
    await writeFile(followMarker, "1");
    await clickText("Connect");
    await wait(
      async () => openerCalls().length >= 4 && followed().length >= 1,
      "the system browser stand-in to follow the provider page back to the loopback address",
    );
    const returned = followed()[0];
    console.log("desktop sign-in: system browser stand-in ended on", returned);
    assert.equal(returned.status, 200);
    assert.equal(returned.origin, "http://127.0.0.1:47321");
    assert.equal(returned.path, "/api/v1/plugin-authorization/callback");
    assert.equal(returned.authorized, true);
    assert.equal(returned.noReturnLink, true);
    assert.equal(appRequests, 0);
    await wait(
      () =>
        execute(
          'return document.querySelector("[aria-label=\\"Sign-in status\\"]")?.textContent === "Connected"',
        ),
      "the Settings row to show Connected",
    );
    assert.equal(exchanges.length, 1);
    const exchange = exchanges[0];
    assert.equal(exchange.get("grant_type"), "authorization_code");
    assert.equal(exchange.get("code"), "desktop-smoke-code");
    assert.equal(exchange.get("redirect_uri"), fixedReturn);
    assert.equal(exchange.get("client_id"), "launcher-client");
    const secondAuthorization = authorizations.at(-1);
    const challenge = createHash("sha256")
      .update(exchange.get("code_verifier"))
      .digest("base64url");
    assert.equal(
      challenge,
      secondAuthorization.searchParams.get("code_challenge"),
    );
    assert.equal(await execute("return location.href"), settingsUrl);
    console.log(
      "desktop sign-in: Settings row shows Connected; token exchange used the fixed loopback redirect and a matching PKCE verifier",
    );
    execFileSync(
      "import",
      [
        "-window",
        "root",
        "/workspace/test-results/desktop/desktop-oauth-connected.png",
      ],
      { timeout: 5000 },
    );
    const pageText = await execute("return document.body.innerText");
    for (const secret of ["desktop-smoke-access", "desktop-smoke-refresh", "desktop-smoke-code"])
      assert.equal(pageText.includes(secret), false);
  } finally {
    provider.close();
  }
  await click('nav[aria-label="Main navigation"] a[href="/library"]');
  await wait(
    () => exists('button[aria-label="Play Fixture 440 Hz"]'),
    "the local library after the launcher check",
  );
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
  assert.match(
    await execute("return navigator.userAgent"),
    /^Mozilla\/5\.0 .* HitoDesktop\/\d+\.\d+\.\d+/,
  );
  assert.deepEqual(
    await execute(`
    return (async () => {
      const account = await fetch('/api/v1/me').then(r => r.json());
      const sessions = await fetch('/api/v1/account/sessions', {
        headers: { 'x-hito-request': '1', 'x-hito-account': account.id }
      }).then(r => r.json());
      return sessions.map(session => session.client);
    })();
  `),
    ["Hito desktop"],
  );
  console.log("desktop user agent and session label verified");
  await click('a[href="/settings"]');
  await wait(
    () =>
      execute(
        'return [...document.querySelectorAll("button")].some(b => b.textContent.includes("Add music folder"))',
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
        'return document.body.textContent.includes("1 file read")',
      ),
    "the local scan",
  );
  await click('nav[aria-label="Main navigation"] a[href="/library"]');
  await wait(
    () => exists('button[aria-label="Play Fixture 440 Hz"]'),
    "the indexed local track",
  );
  await noBrowserAudio();
  if (process.env.HITO_TEST_NO_NETWORK === "1") await verifyWithoutNetwork();
  await verifyBrowserLauncher();
  await click('button[aria-label="Play Fixture 440 Hz"]');
  await wait(
    async () => (await uiPosition()) > 0.2 && audible(),
    "native UI position and actual CPAL output",
  );
  const initial = await playerState();
  assert.equal(initial.native, true);
  assert.equal(initial.state.playing, true);
  const position = await uiPosition();
  await click('nav[aria-label="Library sections"] a[href="/albums"]');
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
  await execute(`
    const control = document.querySelector('select[aria-label="Playback speed"]');
    control.value = '1.5';
    control.dispatchEvent(new Event('change', { bubbles: true }));
    return true;
  `);
  await wait(async () => {
    const state = (await playerState()).state;
    return state.speed === 1500 && !state.desired_playing && silent();
  }, "native speed without unpausing");
  await click('button[aria-label="Shuffle"]');
  await click('button[aria-label="Repeat: off"]');
  await wait(async () => {
    const state = (await playerState()).state;
    return (
      state.shuffle &&
      state.repeat === "all" &&
      state.speed === 1500 &&
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
      state.speed === 1500 &&
      !state.desired_playing &&
      silent() &&
      (await execute(
        "return document.querySelector('button[aria-label=\"Repeat: all\"]')?.disabled === false",
      ))
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
  await wait(
    () =>
      execute(
        'return fetch("/api/v1/activity").then(r=>r.json()).then(a=>a.history.some(h=>h.position_ms > 200 && h.speed === 1.5))',
      ),
    "Rust-owned native speed and progress persistence",
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
  for (const command of ["plugin:app|version", "plugin:opener|open_url"])
    assert.equal(
      await execute(
        `return (async () => {const invoke=window.__TAURI_INTERNALS__?.invoke; if(!invoke) return true; try {await invoke(${JSON.stringify(command)}, {url: "https://evil.example/"});return false;} catch {return true;}})()`,
      ),
      true,
    );
  assert.equal(openerCalls().length, 4);
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
      'return fetch("/api/v1/activity").then(r=>r.json()).then(a=>a.history.some(h=>h.position_ms > 200 && h.speed === 1.5))',
    ),
    true,
  );
  await click('nav[aria-label="Library sections"] a[href="/history"]');
  await wait(
    () => exists('button[aria-label="Play Fixture 440 Hz"]'),
    "the saved History row",
  );
  assert.equal(
    await execute(`
    return (async () => {
      const account = await fetch('/api/v1/me').then(r => r.json());
      const activity = await fetch('/api/v1/activity').then(r => r.json());
      const response = await fetch('/api/v1/sync', {
        method: 'POST',
        headers: { 'content-type': 'application/json', 'x-hito-request': '1', 'x-hito-account': account.id },
        body: JSON.stringify({ device_id: 'desktop-saved-fixture', operations: [{
          id: crypto.randomUUID(),
          action: { kind: 'progress_set', recording_id: activity.history[0].id, position_ms: 6000, speed: 0.5 }
        }] })
      });
      return response.ok;
    })();
  `),
    true,
  );
  await armSavedStart(6000, process.env.HITO_TEST_SAVED_START === "zero");
  await click('button[aria-label="Play Fixture 440 Hz"]');
  await wait(async () => {
    const proof = await savedStartObservation();
    assert.equal(proof.error, null);
    const state = (await playerState()).state;
    return (
      proof.armed &&
      proof.playingPosition !== null &&
      state.playing &&
      state.speed === 1000 &&
      (await uiPosition()) >= 6.1 &&
      audible()
    );
  }, "History loads the current Rust cursor over stale rendered History and keeps current speed");
  await stopSavedStartObservation();
  await noBrowserAudio();
  await close(restartedOrigin);
  assert.equal(
    logs().split('"event":"desktop_stopped"').length,
    previousStops + 1,
  );
  assert.equal(logs().includes("desktop fixture account password"), false);
  assert.equal(logs().includes("ticket="), false);
  assert.equal(logs().includes("desktop-smoke-code"), false);
  assert.equal(logs().includes("desktop-smoke-access"), false);
  console.log(
    "Standalone Tauri setup, scan, native output, navigation, reload, Previous, shuffle/repeat, speed, pause/resume, progress, saved-position loading, persistence and joined shutdown passed.",
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
