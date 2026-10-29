import { createElement, type ReactNode } from "react";
import {
  act,
  fireEvent,
  render,
  renderHook,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import AccountMenu from "../../../app/AccountMenu";
import LockPage from "../pages/LockPage";
import ScreenLockSettings from "../components/ScreenLockSettings";
import { useIdleLock } from "../hooks/useIdleLock";
import { lockApi } from "../api/lockApi";
import { sessionApi } from "../api/sessionApi";
import { HttpError } from "../../../shared/http";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { usePlayer } from "../../playback/components/PlaybackProvider";

vi.mock("@inertiajs/react", () => ({
  router: { clearHistory: vi.fn(), reload: vi.fn() },
  Link: ({ children, href }: { children: ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));
vi.mock("../api/lockApi", () => ({
  lockApi: {
    state: vi.fn(),
    activity: vi.fn(),
    lock: vi.fn(),
    unlock: vi.fn(),
    setPin: vi.fn(),
    removePin: vi.fn(),
  },
}));
vi.mock("../api/sessionApi", () => ({
  sessionApi: { signIn: vi.fn(), signOut: vi.fn() },
}));
vi.mock("../session", () => ({ notifySessionChanged: vi.fn() }));
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: vi.fn(),
}));

const reload = vi.fn();
const replace = vi.fn();
const user = { id: "alice-id", username: "alice", admin: false };
const locked = { pin_enabled: true, idle_minutes: 5, locked: true };
const unlocked = { ...locked, locked: false };

beforeEach(() => {
  vi.clearAllMocks();
  vi.stubGlobal("location", { reload, replace, href: "http://hito.local/" });
});

function sidebar(lock: typeof unlocked) {
  const clear = vi.fn();
  vi.mocked(usePlayer).mockReturnValue({ clear } as never);
  render(
    createElement(
      NoticeProvider,
      null,
      createElement(AccountMenu, { user, path: "/library", lock }),
    ),
  );
  return clear;
}

test("the account menu offers Lock only when the account has a PIN", () => {
  sidebar({ pin_enabled: false, idle_minutes: 0, locked: false });
  expect(screen.queryByRole("button", { name: "Lock" })).toBeNull();
});

test("locking stops playback for the account before the lock screen replaces the page", async () => {
  const order: string[] = [];
  const clear = sidebar(unlocked);
  clear.mockImplementation(() => order.push("stop"));
  vi.mocked(lockApi.lock).mockImplementation(async () => {
    order.push("lock");
    return locked;
  });
  fireEvent.click(screen.getByRole("button", { name: "Lock" }));
  await waitFor(() => expect(reload).toHaveBeenCalled());
  expect(order).toEqual(["stop", "lock"]);
  expect(router.clearHistory).toHaveBeenCalled();
  expect(lockApi.lock).toHaveBeenCalledWith("alice-id");
});

test("the lock screen shows only the account name and unlocks with the PIN", async () => {
  vi.mocked(lockApi.unlock).mockResolvedValue(unlocked);
  render(
    createElement(LockPage, {
      errors: {},
      locked: true,
      username: "alice",
      account_id: "alice-id",
    }),
  );
  expect(screen.getByText("alice")).toBeTruthy();
  expect(document.body.textContent).not.toMatch(/Library|Playlists/);
  fireEvent.change(screen.getByLabelText("PIN"), { target: { value: "1234" } });
  fireEvent.submit(screen.getByRole("button", { name: "Unlock" }));
  await waitFor(() => expect(reload).toHaveBeenCalled());
  expect(router.clearHistory).toHaveBeenCalled();
  expect(lockApi.unlock).toHaveBeenCalledWith("alice-id", "1234");
});

test("a wrong PIN keeps the lock screen and shows the message", async () => {
  vi.mocked(lockApi.unlock).mockRejectedValue(
    new HttpError(422, "That PIN is not correct. 3 attempts left"),
  );
  render(
    createElement(LockPage, {
      errors: {},
      locked: true,
      username: "alice",
      account_id: "alice-id",
    }),
  );
  fireEvent.change(screen.getByLabelText("PIN"), { target: { value: "0000" } });
  fireEvent.submit(screen.getByRole("button", { name: "Unlock" }));
  expect((await screen.findByRole("alert")).textContent).toContain(
    "3 attempts",
  );
  expect(reload).not.toHaveBeenCalled();
});

test("the fifth wrong PIN returns to the sign-in page", async () => {
  vi.mocked(lockApi.unlock).mockRejectedValue(
    new HttpError(401, "Too many wrong PINs. Sign in again."),
  );
  render(
    createElement(LockPage, {
      errors: {},
      locked: true,
      username: "alice",
      account_id: "alice-id",
    }),
  );
  fireEvent.change(screen.getByLabelText("PIN"), { target: { value: "0000" } });
  fireEvent.submit(screen.getByRole("button", { name: "Unlock" }));
  await waitFor(() => expect(replace).toHaveBeenCalledWith("/"));
});

test("signing out stays available on the lock screen", async () => {
  vi.mocked(sessionApi.signOut).mockResolvedValue();
  render(
    createElement(LockPage, {
      errors: {},
      locked: true,
      username: "alice",
      account_id: "alice-id",
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Sign out" }));
  await waitFor(() =>
    expect(sessionApi.signOut).toHaveBeenCalledWith("alice-id"),
  );
});

test("PIN settings need the password and can remove the PIN", async () => {
  vi.mocked(lockApi.setPin).mockResolvedValue(unlocked);
  vi.mocked(lockApi.removePin).mockResolvedValue({
    pin_enabled: false,
    idle_minutes: 0,
    locked: false,
  });
  render(
    createElement(
      NoticeProvider,
      null,
      createElement(ScreenLockSettings, {
        account: "alice-id",
        lock: { pin_enabled: false, idle_minutes: 0, locked: false },
      }),
    ),
  );
  fireEvent.change(screen.getByLabelText("PIN"), { target: { value: "2468" } });
  fireEvent.change(screen.getByLabelText("Current password"), {
    target: { value: "a long test password" },
  });
  fireEvent.click(
    within(
      screen.getByRole("radiogroup", { name: "Lock when idle" }),
    ).getByRole("radio", { name: "15 min" }),
  );
  fireEvent.submit(screen.getByRole("button", { name: "Save PIN" }));
  await waitFor(() =>
    expect(lockApi.setPin).toHaveBeenCalledWith("alice-id", {
      password: "a long test password",
      pin: "2468",
      idle_minutes: 15,
    }),
  );
  fireEvent.change(await screen.findByLabelText("Current password"), {
    target: { value: "a long test password" },
  });
  fireEvent.click(await screen.findByRole("button", { name: "Remove PIN" }));
  await waitFor(() =>
    expect(lockApi.removePin).toHaveBeenCalledWith("alice-id", {
      password: "a long test password",
    }),
  );
});

test("an idle page checks the lock after the chosen period and stops playback", async () => {
  vi.useFakeTimers();
  const onLocked = vi.fn();
  vi.mocked(lockApi.state).mockResolvedValue(locked);
  renderHook(() => useIdleLock("alice-id", unlocked, onLocked));
  await act(() => vi.advanceTimersByTimeAsync(4 * 60_000));
  expect(lockApi.state).not.toHaveBeenCalled();
  await act(() => vi.advanceTimersByTimeAsync(2 * 60_000));
  expect(lockApi.state).toHaveBeenCalledWith("alice-id", expect.anything());
  expect(onLocked).toHaveBeenCalled();
  expect(router.clearHistory).toHaveBeenCalled();
  expect(reload).toHaveBeenCalled();
});

test("input keeps the page awake by sending the activity signal, at most every 30 seconds", async () => {
  vi.useFakeTimers();
  vi.mocked(lockApi.activity).mockResolvedValue(unlocked);
  renderHook(() => useIdleLock("alice-id", unlocked, vi.fn()));
  for (let minute = 0; minute < 8; minute++) {
    await act(() => vi.advanceTimersByTimeAsync(60_000));
    fireEvent.keyDown(document, { key: "a" });
    fireEvent.keyDown(document, { key: "b" });
  }
  expect(lockApi.activity).toHaveBeenCalledTimes(8);
  expect(lockApi.state).not.toHaveBeenCalled();
  expect(reload).not.toHaveBeenCalled();
});

test("an activity answer that reports a lock stops playback and reloads", async () => {
  vi.useFakeTimers();
  const onLocked = vi.fn();
  vi.mocked(lockApi.activity).mockResolvedValue(locked);
  renderHook(() => useIdleLock("alice-id", unlocked, onLocked));
  fireEvent.keyDown(document, { key: "a" });
  await act(() => vi.advanceTimersByTimeAsync(0));
  expect(onLocked).toHaveBeenCalled();
  expect(reload).toHaveBeenCalled();
});

test("accounts without an idle period never check the lock", async () => {
  vi.useFakeTimers();
  renderHook(() =>
    useIdleLock("alice-id", { ...unlocked, idle_minutes: 0 }, vi.fn()),
  );
  fireEvent.keyDown(document, { key: "a" });
  await act(() => vi.advanceTimersByTimeAsync(2 * 60 * 60_000));
  expect(lockApi.state).not.toHaveBeenCalled();
});
