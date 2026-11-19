import { createElement } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import LoginPage from "../../session/pages/LoginPage";
import { sessionApi } from "../../session/api/sessionApi";
import { placeApi } from "../api/placeApi";

vi.mock("@inertiajs/react", () => ({ router: { clearHistory: vi.fn() } }));
vi.mock("../../session/api/sessionApi", () => ({
  sessionApi: { signIn: vi.fn(), signOut: vi.fn() },
}));
vi.mock("../api/placeApi", () => ({
  placeApi: { offer: vi.fn(), change: vi.fn(), forgetMe: vi.fn() },
}));

type Props = Parameters<typeof LoginPage>[0];

function inApp(remembered: string | null) {
  vi.mocked(placeApi.offer).mockResolvedValue({
    changeable: true,
    current: {
      kind: "server",
      origin: "https://music.example.org",
      remembered,
    },
  });
}

beforeEach(() => {
  vi.mocked(placeApi.offer).mockReset();
  vi.mocked(placeApi.change).mockReset().mockResolvedValue(undefined);
  vi.mocked(placeApi.forgetMe).mockReset().mockResolvedValue(undefined);
  vi.mocked(sessionApi.signIn).mockReset().mockResolvedValue(undefined);
});

test("in a browser the sign-in page is unchanged", async () => {
  vi.mocked(placeApi.offer).mockResolvedValue({
    changeable: false,
    current: null,
  });
  render(createElement(LoginPage, { needs_setup: false } as Props));
  await waitFor(() => expect(placeApi.offer).toHaveBeenCalled());
  expect(screen.getByRole("heading", { name: "Sign in" })).toBeTruthy();
  expect(screen.getByText(window.location.host)).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Change server" })).toBeNull();
});

test("inside the app, sign in names the server and can change it", async () => {
  inApp(null);
  render(createElement(LoginPage, { needs_setup: false } as Props));
  expect(await screen.findByText("https://music.example.org")).toBeTruthy();
  expect(screen.getByRole("heading", { name: "Sign in" })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Change server" }));
  await waitFor(() => expect(placeApi.change).toHaveBeenCalled());
});

test("a remembered account is welcomed back and signs in with its own name", async () => {
  inApp("river");
  render(createElement(LoginPage, { needs_setup: false } as Props));
  expect(
    await screen.findByRole("heading", { name: "Welcome back" }),
  ).toBeTruthy();
  expect(screen.getByText("river")).toBeTruthy();
  expect(screen.queryByLabelText("Username")).toBeNull();
  fireEvent.change(screen.getByLabelText("Password"), {
    target: { value: "correct horse battery" },
  });
  fireEvent.submit(screen.getByRole("button", { name: "Sign in" }));
  await waitFor(() =>
    expect(sessionApi.signIn).toHaveBeenCalledWith(
      false,
      "river",
      "correct horse battery",
    ),
  );
});

test("another account starts empty, and forget me drops the remembered name", async () => {
  inApp("river");
  render(createElement(LoginPage, { needs_setup: false } as Props));
  fireEvent.click(
    await screen.findByRole("button", { name: "Use another account" }),
  );
  expect(screen.getByRole("heading", { name: "Sign in" })).toBeTruthy();
  expect((screen.getByLabelText("Username") as HTMLInputElement).value).toBe(
    "",
  );
  expect(
    screen.queryByRole("button", { name: "Forget me on this device" }),
  ).toBeNull();
  render(createElement(LoginPage, { needs_setup: false } as Props));
  const forget = await screen.findAllByRole("button", {
    name: "Forget me on this device",
  });
  fireEvent.click(forget[0]);
  await waitFor(() => expect(placeApi.forgetMe).toHaveBeenCalled());
  expect(screen.queryByRole("heading", { name: "Welcome back" })).toBeNull();
});

test("setting up a new server inside the app names it and can change it", async () => {
  inApp(null);
  render(createElement(LoginPage, { needs_setup: true } as Props));
  expect(await screen.findByText("https://music.example.org")).toBeTruthy();
  expect(screen.getByRole("heading", { name: "Set up Hito" })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Change server" }));
  await waitFor(() => expect(placeApi.change).toHaveBeenCalled());
});
