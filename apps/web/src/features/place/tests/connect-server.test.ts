import { createElement } from "react";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ConnectServer from "../components/ConnectServer";
import SavedServer from "../components/SavedServer";
import type { ConnectOutcome, ServerPlace } from "../api/desktopPlace";

function place(
  outcomes: ConnectOutcome[],
  recent = [] as Awaited<ReturnType<ServerPlace["recent"]>>,
): ServerPlace {
  return {
    connect: vi.fn(
      async (): Promise<ConnectOutcome> =>
        outcomes.shift() ?? { outcome: "opened" },
    ),
    cancel: vi.fn(async () => {}),
    recent: vi.fn(async () => recent),
    forget: vi.fn(async () => ({ told: true })),
    saved: vi.fn(async () => null),
  };
}

function type(address: string) {
  fireEvent.change(screen.getByLabelText("Server address"), {
    target: { value: address },
  });
  fireEvent.click(screen.getByRole("button", { name: "Connect" }));
}

afterEach(() => vi.useRealTimers());

test("the address screen explains what to type and goes back", async () => {
  const back = vi.fn();
  render(createElement(ConnectServer, { place: place([]), onBack: back }));
  expect(
    screen.getByRole("heading", { name: "Connect to a server" }),
  ).toBeTruthy();
  expect(
    screen.getByText(
      "The address you open in a browser. You can leave out http:// or https://.",
    ),
  ).toBeTruthy();
  const field = screen.getByLabelText("Server address") as HTMLInputElement;
  expect(field.placeholder).toBe("hito.home:8080");
  expect(field.getAttribute("autocapitalize")).toBe("off");
  expect(field.getAttribute("spellcheck")).toBe("false");
  fireEvent.click(screen.getByRole("button", { name: "Back" }));
  expect(back).toHaveBeenCalled();
});

test("connecting shows that Hito is being looked for, and cancel keeps the address", async () => {
  const server = place([]);
  let finish!: (outcome: ConnectOutcome) => void;
  vi.mocked(server.connect).mockImplementationOnce(
    () => new Promise((done) => (finish = done)),
  );
  render(createElement(ConnectServer, { place: server, onBack: vi.fn() }));
  type("hito.home:8080");
  expect(await screen.findByText("Looking for Hito…")).toBeTruthy();
  expect(server.connect).toHaveBeenCalledWith("hito.home:8080", false);
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(server.cancel).toHaveBeenCalled();
  await act(async () =>
    finish({ outcome: "no_answer", shown: "hito.home:8080" }),
  );
  expect(screen.queryByRole("alert")).toBeNull();
  expect(
    (screen.getByLabelText("Server address") as HTMLInputElement).value,
  ).toBe("hito.home:8080");
});

test("each problem is named in the design's words and keeps the address", async () => {
  const server = place([
    { outcome: "not_an_address" },
    { outcome: "no_answer", shown: "hito.home:8080" },
    { outcome: "not_hito", host: "192.168.1.1" },
    { outcome: "certificate", host: "hito.home" },
  ]);
  render(createElement(ConnectServer, { place: server, onBack: vi.fn() }));
  type("my server");
  expect((await screen.findByRole("alert")).textContent).toBe(
    "Type an address like hito.home:8080 or 192.168.1.20:8080.",
  );
  type("hito.home:8080");
  await waitFor(() =>
    expect(screen.getByRole("alert").textContent).toBe(
      "No answer from hito.home:8080. Check the address, and that this device is on the same network or VPN.",
    ),
  );
  expect(document.activeElement).toBe(screen.getByLabelText("Server address"));
  type("192.168.1.1");
  await waitFor(() =>
    expect(screen.getByRole("alert").textContent).toBe(
      "192.168.1.1 answered, but it is not a Hito server.",
    ),
  );
  type("https://hito.home");
  await waitFor(() =>
    expect(screen.getByRole("alert").textContent).toBe(
      "The certificate for hito.home can't be checked, so Hito won't send your password there.Give the server a certificate this device trusts, or use http:// inside your VPN.",
    ),
  );
  expect(screen.queryByRole("button", { name: /anyway/ })).toBeNull();
});

test("a plain http server asks once before connecting", async () => {
  const notEncrypted = {
    outcome: "not_encrypted",
    origin: "http://192.168.1.20:8080/",
    shown: "http://192.168.1.20:8080",
  } as const;
  const server = place([notEncrypted, notEncrypted, { outcome: "opened" }]);
  render(createElement(ConnectServer, { place: server, onBack: vi.fn() }));
  type("192.168.1.20:8080");
  expect(
    await screen.findByRole("heading", {
      name: "This connection is not encrypted",
    }),
  ).toBeTruthy();
  expect(screen.getByText("http://192.168.1.20:8080")).toBeTruthy();
  expect(
    screen.getByText(
      "Anyone on the same network could read what you send, including your password.",
    ),
  ).toBeTruthy();
  expect(
    screen.getByText(
      "That is fine at home or inside a VPN. Don't use it over the open internet.",
    ),
  ).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Change address" }));
  expect(
    ((await screen.findByLabelText("Server address")) as HTMLInputElement)
      .value,
  ).toBe("192.168.1.20:8080");
  type("192.168.1.20:8080");
  await screen.findByRole("heading", {
    name: "This connection is not encrypted",
  });
  fireEvent.click(screen.getByRole("button", { name: "Connect anyway" }));
  await waitFor(() =>
    expect(server.connect).toHaveBeenLastCalledWith(
      "http://192.168.1.20:8080/",
      true,
    ),
  );
});

test("recent servers are listed newest first, open again, and are forgotten after asking", async () => {
  const day = 86_400;
  const now = Date.now() / 1000;
  const server = place(
    [{ outcome: "opened" }],
    [
      {
        origin: "https://music.example.org/",
        shown: "https://music.example.org",
        account: "Acidiney",
        used_at: now - day,
      },
      {
        origin: "http://192.168.1.20:8080/",
        shown: "http://192.168.1.20:8080",
        account: null,
        used_at: now - 21 * day,
      },
    ],
  );
  render(createElement(ConnectServer, { place: server, onBack: vi.fn() }));
  expect(await screen.findByRole("heading", { name: "Recent" })).toBeTruthy();
  expect(screen.getByText("Acidiney · used yesterday")).toBeTruthy();
  expect(screen.getByText("used 3 weeks ago")).toBeTruthy();
  fireEvent.click(
    screen.getByRole("button", { name: "Forget http://192.168.1.20:8080" }),
  );
  const dialog = await screen.findByRole("dialog", {
    name: "Forget http://192.168.1.20:8080?",
  });
  expect(dialog.textContent).toContain(
    "This device is signed out of it and the address leaves this list. Nothing on the server is deleted.",
  );
  fireEvent.click(screen.getByRole("button", { name: "Forget" }));
  await waitFor(() =>
    expect(server.forget).toHaveBeenCalledWith("http://192.168.1.20:8080/"),
  );
  await waitFor(() =>
    expect(screen.queryByText("used 3 weeks ago")).toBeNull(),
  );
  fireEvent.click(
    screen.getByRole("button", { name: /^https:\/\/music\.example\.org/ }),
  );
  await waitFor(() =>
    expect(server.connect).toHaveBeenCalledWith(
      "https://music.example.org/",
      false,
    ),
  );
});

test("an unreachable saved server says so and tries again every ten seconds", async () => {
  vi.useFakeTimers();
  const unreachable = {
    outcome: "no_answer",
    shown: "https://music.example.org",
  } as const;
  const server = place([unreachable, unreachable, unreachable]);
  const changeServer = vi.fn();
  render(
    createElement(SavedServer, {
      place: server,
      origin: "https://music.example.org/",
      onChangeServer: changeServer,
    }),
  );
  await act(async () => {});
  expect(
    screen.getByRole("heading", { name: "Can't reach your server" }),
  ).toBeTruthy();
  expect(
    screen.getByText("https://music.example.org · no answer"),
  ).toBeTruthy();
  expect(
    screen.getByText(
      "Check that this device is on the same network or VPN as the server.",
    ),
  ).toBeTruthy();
  expect(server.connect).toHaveBeenCalledTimes(1);
  await act(async () => {
    vi.advanceTimersByTime(10_000);
  });
  expect(server.connect).toHaveBeenCalledTimes(2);
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  await act(async () => {});
  expect(server.connect).toHaveBeenCalledTimes(3);
  fireEvent.click(screen.getByRole("button", { name: "Change server" }));
  expect(changeServer).toHaveBeenCalled();
});

test("a saved server whose certificate can't be checked can't be used", async () => {
  const server = place([{ outcome: "certificate", host: "music.example.org" }]);
  render(
    createElement(SavedServer, {
      place: server,
      origin: "https://music.example.org/",
      onChangeServer: vi.fn(),
    }),
  );
  expect(
    await screen.findByRole("heading", { name: "Can't use this server" }),
  ).toBeTruthy();
  expect(
    screen.getByText(
      "The certificate for music.example.org can't be checked, so Hito won't send your password there.",
    ),
  ).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
});

test("forgetting a server that can't be told says the sign-in may stay listed there", async () => {
  const server = place(
    [],
    [
      {
        origin: "https://music.example.org/",
        shown: "https://music.example.org",
        account: "Acidiney",
        used_at: Date.now() / 1000,
      },
    ],
  );
  vi.mocked(server.forget).mockResolvedValueOnce({ told: false });
  render(createElement(ConnectServer, { place: server, onBack: vi.fn() }));
  fireEvent.click(
    await screen.findByRole("button", {
      name: "Forget https://music.example.org",
    }),
  );
  await screen.findByRole("dialog");
  fireEvent.click(screen.getByRole("button", { name: "Forget" }));
  expect((await screen.findByRole("status")).textContent).toBe(
    "https://music.example.org could not be told. This device may stay in the server's list of signed-in devices.",
  );
});

test("a server and app that don't match say which one to update", async () => {
  const server = place([
    { outcome: "server_too_old", version: "0.3.0" },
    { outcome: "server_too_old", version: null },
    { outcome: "app_too_old" },
  ]);
  render(createElement(ConnectServer, { place: server, onBack: vi.fn() }));
  type("hito.home:8080");
  expect((await screen.findByRole("alert")).textContent).toBe(
    "This server runs Hito 0.3.0, which is too old for this app. Update the server, or open it in a browser.",
  );
  type("hito.home:8080");
  await waitFor(() =>
    expect(screen.getByRole("alert").textContent).toBe(
      "This server runs an older Hito, which is too old for this app. Update the server, or open it in a browser.",
    ),
  );
  type("hito.home:8080");
  await waitFor(() =>
    expect(screen.getByRole("alert").textContent).toBe(
      "This app is older than the server. Update the app.",
    ),
  );
});
