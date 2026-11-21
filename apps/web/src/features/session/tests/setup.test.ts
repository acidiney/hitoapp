import { createElement } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import LoginPage from "../pages/LoginPage";
import { sessionApi } from "../api/sessionApi";
import { HttpError } from "../../../shared/http";

vi.mock("@inertiajs/react", () => ({ router: { clearHistory: vi.fn() } }));
vi.mock("../api/sessionApi", () => ({
  sessionApi: { signIn: vi.fn(), signOut: vi.fn() },
}));
afterEach(cleanup);

const setupPage = () =>
  render(
    createElement(LoginPage, { needs_setup: true } as Parameters<
      typeof LoginPage
    >[0]),
  );

test("first-run setup explains the first account and gives password guidance", () => {
  setupPage();
  expect(
    screen.getByRole("heading", { name: "Set up Hito", level: 1 }),
  ).toBeTruthy();
  expect(screen.getByText(/first one becomes the administrator/i)).toBeTruthy();
  const password = screen.getByLabelText(/^Password/);
  expect(password.getAttribute("autocomplete")).toBe("new-password");
  expect(password.getAttribute("minlength")).toBe("12");
  expect(
    password.getAttribute("aria-describedby") &&
      document.getElementById(password.getAttribute("aria-describedby")!)
        ?.textContent,
  ).toMatch(/at least 12 characters/i);
  expect(screen.getByRole("list", { name: "Setup steps" })).toBeTruthy();
  expect(screen.getByText("2. Music folders")).toBeTruthy();
});

test("the password hint counts characters until the minimum is reached", () => {
  setupPage();
  const password = screen.getByLabelText(/^Password/);
  const hint = () =>
    document.getElementById("setup-password-hint")?.textContent;
  fireEvent.change(password, { target: { value: "correct h" } });
  expect(hint()).toBe("9 of 12 characters");
  fireEvent.change(password, { target: { value: "correct horse" } });
  expect(hint()).toBe("Long enough");
});

test("the eye shows and hides the password without submitting", () => {
  setupPage();
  const password = screen.getByLabelText(/^Password/) as HTMLInputElement;
  const eye = screen.getByRole("button", { name: "Show password" });
  expect(eye.getAttribute("aria-pressed")).toBe("false");
  expect(password.type).toBe("password");
  fireEvent.click(eye);
  expect(eye.getAttribute("aria-pressed")).toBe("true");
  expect(password.type).toBe("text");
  expect(sessionApi.signIn).not.toHaveBeenCalled();
});

test("setup is never linked to or from Sign-in", () => {
  setupPage();
  expect(screen.queryByRole("link")).toBeNull();
  cleanup();
  render(
    createElement(LoginPage, { needs_setup: false } as Parameters<
      typeof LoginPage
    >[0]),
  );
  expect(screen.queryByRole("link")).toBeNull();
  expect(screen.queryByText(/create.*account/i)).toBeNull();
});

test("a rejected setup shows the error as an alert and keeps the form", async () => {
  vi.mocked(sessionApi.signIn).mockRejectedValueOnce(new Error("Too weak"));
  setupPage();
  fireEvent.change(screen.getByLabelText("Username"), {
    target: { value: "admin" },
  });
  fireEvent.change(screen.getByLabelText(/^Password/), {
    target: { value: "correct horse battery" },
  });
  fireEvent.submit(
    screen.getByRole("button", { name: "Create administrator" }),
  );
  expect((await screen.findByRole("alert")).textContent).toBe("Too weak");
  expect(sessionApi.signIn).toHaveBeenCalledWith(
    true,
    "admin",
    "correct horse battery",
  );
});

test("when another device finishes setup first, the page turns into sign-in and says why", async () => {
  vi.mocked(sessionApi.signIn).mockRejectedValueOnce(
    new HttpError(409, "Setup is already complete"),
  );
  setupPage();
  fireEvent.change(screen.getByLabelText("Username"), {
    target: { value: "river" },
  });
  fireEvent.change(screen.getByLabelText(/^Password/), {
    target: { value: "correct horse battery" },
  });
  fireEvent.submit(screen.getByRole("button", { name: /Create/ }));
  expect(
    await screen.findByRole("heading", { name: "Sign in", level: 1 }),
  ).toBeTruthy();
  expect(screen.getByRole("alert").textContent).toBe(
    "Someone else finished setting up this server. Sign in with your account.",
  );
  expect((screen.getByLabelText("Username") as HTMLInputElement).value).toBe(
    "river",
  );
  vi.mocked(sessionApi.signIn).mockResolvedValueOnce(undefined);
  fireEvent.change(screen.getByLabelText(/^Password/), {
    target: { value: "correct horse battery" },
  });
  fireEvent.submit(screen.getByRole("button", { name: "Sign in" }));
  await vi.waitFor(() =>
    expect(sessionApi.signIn).toHaveBeenLastCalledWith(
      false,
      "river",
      "correct horse battery",
    ),
  );
});
