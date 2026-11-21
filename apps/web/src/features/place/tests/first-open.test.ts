import { createElement } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import FirstOpen from "../components/FirstOpen";

function deferred() {
  let resolve!: () => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<void>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

test("the first open asks where the music is, with both places and how to change it later", () => {
  render(
    createElement(FirstOpen, {
      startHere: vi.fn(),
      chooseServer: vi.fn(),
    }),
  );
  expect(
    screen.getByRole("heading", { name: "Where is your music?" }),
  ).toBeTruthy();
  expect(
    screen.getByText(
      "Choose which library this app opens. Your music files are never moved.",
    ),
  ).toBeTruthy();
  const group = screen.getByRole("group", {
    name: "Where Hito keeps its library",
  });
  expect(group.querySelectorAll("button").length).toBe(2);
  expect(screen.getByRole("button", { name: /On this computer/ })).toBeTruthy();
  expect(screen.getByRole("button", { name: /On a Hito server/ })).toBeTruthy();
  expect(
    screen.getByText("You can change this later in Settings."),
  ).toBeTruthy();
});

test("choosing this computer starts the library and says so while it starts", async () => {
  const start = deferred();
  const startHere = vi.fn(() => start.promise);
  render(createElement(FirstOpen, { startHere, chooseServer: vi.fn() }));
  fireEvent.click(screen.getByRole("button", { name: /On this computer/ }));
  expect(startHere).toHaveBeenCalledTimes(1);
  expect(
    await screen.findByRole("heading", {
      name: "Starting Hito on this computer…",
    }),
  ).toBeTruthy();
  expect(screen.getByText("Opening your library")).toBeTruthy();
  start.resolve();
});

test("a library that cannot start offers to try again or to connect to a server instead", async () => {
  const startHere = vi
    .fn()
    .mockRejectedValueOnce(new Error("startup"))
    .mockReturnValueOnce(new Promise<void>(() => {}));
  const chooseServer = vi.fn();
  render(createElement(FirstOpen, { startHere, chooseServer }));
  fireEvent.click(screen.getByRole("button", { name: /On this computer/ }));
  expect(
    await screen.findByRole("heading", {
      name: "Couldn't start Hito on this computer",
    }),
  ).toBeTruthy();
  expect(screen.getByText("Your library was not changed.")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  expect(startHere).toHaveBeenCalledTimes(2);
  expect(
    await screen.findByRole("heading", {
      name: "Starting Hito on this computer…",
    }),
  ).toBeTruthy();
});

test("connect to a server instead leaves the failed start for the server choice", async () => {
  const chooseServer = vi.fn();
  render(
    createElement(FirstOpen, {
      startHere: vi.fn().mockRejectedValue(new Error("startup")),
      chooseServer,
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: /On this computer/ }));
  fireEvent.click(
    await screen.findByRole("button", { name: "Connect to a server instead" }),
  );
  expect(chooseServer).toHaveBeenCalledTimes(1);
});

test("choosing a server hands over to the server choice", () => {
  const chooseServer = vi.fn();
  render(createElement(FirstOpen, { startHere: vi.fn(), chooseServer }));
  fireEvent.click(screen.getByRole("button", { name: /On a Hito server/ }));
  expect(chooseServer).toHaveBeenCalledTimes(1);
});
