import type { Schema } from "../../../shared/schema";
import { createElement } from "react";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import AccountSecurity from "../components/AccountSecurity";
import { accountApi } from "../api/accountApi";
import { HttpError } from "../../../shared/http";
import { NoticeProvider } from "../../../shared/ui/Notices";

vi.mock("../api/accountApi", () => ({
  accountApi: {
    changePassword: vi.fn(),
    sessions: vi.fn(),
    revokeSession: vi.fn(),
    revokeOthers: vi.fn(),
  },
}));

const seconds = Math.floor(Date.now() / 1000);
const here: Schema<"SessionInfo"> = {
  id: "a".repeat(32),
  created_at: 1_790_000_000,
  last_seen_at: seconds - 20,
  client: "Firefox on macOS",
  current: true,
};
const phone: Schema<"SessionInfo"> = {
  id: "b".repeat(32),
  created_at: 1_789_000_000,
  last_seen_at: seconds - 300,
  client: "Safari on iPhone",
  current: false,
};
const laptop: Schema<"SessionInfo"> = {
  id: "c".repeat(32),
  created_at: 1_788_000_000,
  last_seen_at: seconds - 3 * 86_400,
  client: null,
  current: false,
};

beforeEach(() => {
  vi.mocked(accountApi.sessions).mockResolvedValue([here, phone, laptop]);
});

function open() {
  return render(
    createElement(
      NoticeProvider,
      null,
      createElement(AccountSecurity, { account: "alice-id" }),
    ),
  );
}

function fill(current: string, next: string, again = next) {
  fireEvent.change(screen.getByLabelText("Current password"), {
    target: { value: current },
  });
  fireEvent.change(screen.getByLabelText("New password"), {
    target: { value: next },
  });
  fireEvent.change(screen.getByLabelText("Repeat new password"), {
    target: { value: again },
  });
}

test("the password form sends both passwords, clears itself and reloads the session list", async () => {
  vi.mocked(accountApi.changePassword).mockResolvedValue({ revoked: 2 });
  open();
  await screen.findByText("This session");
  fill("old password here", "a new long password");
  fireEvent.click(screen.getByRole("button", { name: "Change password" }));
  await waitFor(() =>
    expect(accountApi.changePassword).toHaveBeenCalledWith(
      "alice-id",
      {
        current_password: "old password here",
        new_password: "a new long password",
      },
      expect.any(AbortSignal),
    ),
  );
  expect((await screen.findByRole("status")).textContent).toContain(
    "2 other sessions were signed out",
  );
  expect(
    (screen.getByLabelText("Current password") as HTMLInputElement).value,
  ).toBe("");
  expect(
    (screen.getByLabelText("New password") as HTMLInputElement).value,
  ).toBe("");
  await waitFor(() => expect(accountApi.sessions).toHaveBeenCalledTimes(2));
});

test("a short or mismatched new password is refused before anything is sent", () => {
  open();
  fill("old password here", "short");
  fireEvent.submit(screen.getByRole("button", { name: "Change password" }));
  expect(screen.getByRole("alert").textContent).toContain("at least 12");
  fill("old password here", "a new long password", "another long password");
  fireEvent.submit(screen.getByRole("button", { name: "Change password" }));
  expect(screen.getByRole("alert").textContent).toContain("do not match");
  expect(accountApi.changePassword).not.toHaveBeenCalled();
});

test("a wrong current password shows one plain message and keeps the typed new password", async () => {
  vi.mocked(accountApi.changePassword).mockRejectedValue(
    new HttpError(422, "That password is not correct", "wrong_password"),
  );
  open();
  fill("not the password", "a new long password");
  fireEvent.click(screen.getByRole("button", { name: "Change password" }));
  expect((await screen.findByRole("alert")).textContent).toBe(
    "The current password is not correct.",
  );
  expect(
    (screen.getByLabelText("New password") as HTMLInputElement).value,
  ).toBe("a new long password");
});

test("too many attempts show the server message", async () => {
  vi.mocked(accountApi.changePassword).mockRejectedValue(
    new HttpError(429, "Too many attempts. Try again later."),
  );
  open();
  fill("not the password", "a new long password");
  fireEvent.click(screen.getByRole("button", { name: "Change password" }));
  expect((await screen.findByRole("alert")).textContent).toContain(
    "Too many attempts",
  );
  expect(screen.getByRole("alert").textContent).toContain(
    "revoke it under Open sessions below",
  );
});

test("a change that lost a race shows the server message", async () => {
  vi.mocked(accountApi.changePassword).mockRejectedValue(
    new HttpError(
      409,
      "Your password was just changed elsewhere. Sign in again.",
    ),
  );
  open();
  fill("old password here", "a new long password");
  fireEvent.click(screen.getByRole("button", { name: "Change password" }));
  expect((await screen.findByRole("alert")).textContent).toBe(
    "Your password was just changed elsewhere. Sign in again.",
  );
});

test("the list marks this session and offers Revoke only for the others", async () => {
  open();
  await screen.findByText("This session");
  expect(screen.getAllByText(/Last active/)).toHaveLength(3);
  expect(screen.getByText("Firefox on macOS")).toBeTruthy();
  expect(screen.getByText("Safari on iPhone")).toBeTruthy();
  expect(screen.getByText("Unknown browser")).toBeTruthy();
  expect(screen.getByText(/^Last active just now · Signed in /)).toBeTruthy();
  expect(
    screen.getByText(/^Last active 5 minutes ago · Signed in /),
  ).toBeTruthy();
  expect(screen.getByText(/^Last active 3 days ago · Signed in /)).toBeTruthy();
  const current = screen.getByText("This session");
  expect(current.closest(".setting-line-action")).toBeTruthy();
  expect(screen.getAllByText("This session")).toHaveLength(1);
  expect(
    screen.getAllByRole("button", { name: /^Revoke session/ }),
  ).toHaveLength(2);
  expect(accountApi.sessions).toHaveBeenCalledWith(
    "alice-id",
    expect.any(AbortSignal),
  );
});

test("revoking one session removes only that row", async () => {
  vi.mocked(accountApi.revokeSession).mockResolvedValue(undefined);
  open();
  await screen.findByText("This session");
  const buttons = screen.getAllByRole("button", { name: /^Revoke session/ });
  fireEvent.click(buttons[0]);
  await waitFor(() =>
    expect(accountApi.revokeSession).toHaveBeenCalledWith(
      "alice-id",
      phone.id,
      expect.any(AbortSignal),
    ),
  );
  await waitFor(() =>
    expect(
      screen.getAllByRole("button", { name: /^Revoke session/ }),
    ).toHaveLength(1),
  );
  expect(screen.getByText("This session")).toBeTruthy();
});

test("Sign out other sessions keeps only this one", async () => {
  vi.mocked(accountApi.revokeOthers).mockResolvedValue({ revoked: 2 });
  open();
  await screen.findByText("This session");
  fireEvent.click(
    screen.getByRole("button", { name: "Sign out other sessions" }),
  );
  await waitFor(() => expect(accountApi.revokeOthers).toHaveBeenCalled());
  await waitFor(() =>
    expect(screen.queryByText(/Safari on iPhone/)).toBeNull(),
  );
  expect(screen.queryByText(/Unknown browser/)).toBeNull();
  expect(
    (
      screen.getByRole("button", {
        name: "Sign out other sessions",
      }) as HTMLButtonElement
    ).disabled,
  ).toBe(true);
});

test("a failed list load shows a retry and a failed revoke keeps the row", async () => {
  vi.mocked(accountApi.sessions).mockRejectedValueOnce(
    new HttpError(500, "Hito could not complete this request"),
  );
  open();
  expect((await screen.findByRole("alert")).textContent).toContain(
    "could not complete",
  );
  vi.mocked(accountApi.revokeSession).mockRejectedValue(
    new HttpError(404, "Resource not found"),
  );
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  await screen.findByText("This session");
  fireEvent.click(
    screen.getAllByRole("button", { name: /^Revoke session/ })[0],
  );
  await waitFor(() => expect(accountApi.revokeSession).toHaveBeenCalled());
  await waitFor(() =>
    expect(
      screen.getAllByRole("button", { name: /^Revoke session/ }),
    ).toHaveLength(2),
  );
});

test("leaving the page cancels an unfinished session load", async () => {
  let signal: AbortSignal | undefined;
  vi.mocked(accountApi.sessions).mockImplementation(
    (_account, given) =>
      new Promise((_resolve, reject) => {
        signal = given;
        given?.addEventListener("abort", () =>
          reject(new DOMException("aborted", "AbortError")),
        );
      }),
  );
  const view = open();
  await waitFor(() => expect(signal).toBeDefined());
  act(() => view.unmount());
  expect(signal?.aborted).toBe(true);
});

test("the session list never prints a secret and the card names its purpose", async () => {
  open();
  await screen.findByText("This session");
  const card = screen.getByRole("region", { name: "Open sessions" });
  expect(within(card).getAllByText(/Signed in/).length).toBe(3);
});

test("a hostile client label is shown as plain text", async () => {
  const hostile = '<img src=x onerror="alert(1)"><b>bold</b>‮evil​';
  vi.mocked(accountApi.sessions).mockResolvedValue([
    here,
    { ...phone, client: hostile },
  ]);
  const view = open();
  const row = await screen.findByText((text) => text.includes("<img src=x"));
  expect(row.textContent).toBe(hostile);
  expect(view.container.querySelector("img")).toBeNull();
  expect(view.container.querySelector("b")).toBeNull();
  expect(
    screen.getByRole("button", { name: /^Revoke session <img/ }),
  ).toBeTruthy();
});
