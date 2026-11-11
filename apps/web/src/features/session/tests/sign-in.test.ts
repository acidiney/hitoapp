import { createElement } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import LoginPage from "../pages/LoginPage";
import { sessionApi } from "../api/sessionApi";

vi.mock("@inertiajs/react", () => ({ router: { clearHistory: vi.fn() } }));
vi.mock("../api/sessionApi", () => ({
  sessionApi: { signIn: vi.fn(), signOut: vi.fn() },
}));

function fillAndSubmit() {
  fireEvent.change(screen.getByLabelText("Username"), {
    target: { value: "alice" },
  });
  fireEvent.change(screen.getByLabelText("Password"), {
    target: { value: "correct horse battery" },
  });
  fireEvent.submit(screen.getByRole("button", { name: "Sign in" }));
}

test("a rejected sign-in keeps the form and shows the error", async () => {
  vi.mocked(sessionApi.signIn).mockRejectedValueOnce(new Error("Wrong login"));
  render(
    createElement(LoginPage, { needs_setup: false } as Parameters<
      typeof LoginPage
    >[0]),
  );
  fillAndSubmit();
  expect(await screen.findByRole("alert")).toHaveProperty(
    "textContent",
    "Wrong login",
  );
  expect(sessionApi.signIn).toHaveBeenCalledWith(
    false,
    "alice",
    "correct horse battery",
  );
  await waitFor(() =>
    expect(
      (screen.getByRole("button", { name: "Sign in" }) as HTMLButtonElement)
        .disabled,
    ).toBe(false),
  );
});

test("a rejected sign-in clears the password and returns focus to it", async () => {
  vi.mocked(sessionApi.signIn).mockRejectedValueOnce(new Error("Wrong login"));
  render(
    createElement(LoginPage, { needs_setup: false } as Parameters<
      typeof LoginPage
    >[0]),
  );
  fillAndSubmit();
  await screen.findByRole("alert");
  const password = screen.getByLabelText("Password") as HTMLInputElement;
  await waitFor(() => expect(document.activeElement).toBe(password));
  expect(password.value).toBe("");
  expect((screen.getByLabelText("Username") as HTMLInputElement).value).toBe(
    "alice",
  );
});

test("Caps Lock is announced while typing a password", () => {
  render(
    createElement(LoginPage, { needs_setup: false } as Parameters<
      typeof LoginPage
    >[0]),
  );
  fireEvent.keyDown(screen.getByLabelText("Password"), {
    key: "A",
    modifierCapsLock: true,
  });
  expect(screen.getByText("Caps Lock is on")).toBeTruthy();
  fireEvent.blur(screen.getByLabelText("Password"));
  expect(screen.queryByText("Caps Lock is on")).toBeNull();
});
