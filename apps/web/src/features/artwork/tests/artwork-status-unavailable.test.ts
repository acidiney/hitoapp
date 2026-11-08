import { createElement } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import ArtworkStatus from "../components/ArtworkStatus";
import ArtworkRetryMenu from "../components/ArtworkRetryMenu";

const artwork = vi.hoisted(() => ({
  state: { imageFailed: true, phase: "unavailable", retry: () => {} } as {
    imageFailed: boolean;
    phase: string;
    retry: () => void;
  },
}));
vi.mock("../components/ArtworkProvider", () => ({
  useArtwork: () => artwork.state,
}));

test("no provider shows nothing in the page body", () => {
  artwork.state = { imageFailed: true, phase: "unavailable", retry: () => {} };
  const { container } = render(createElement(ArtworkStatus, { release: "r" }));
  expect(container.textContent).toBe("");
});

test("no provider offers the retry in a quiet menu", () => {
  const retry = vi.fn();
  artwork.state = { imageFailed: true, phase: "unavailable", retry };
  render(createElement(ArtworkRetryMenu, { release: "r" }));
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Retry artwork" }));
  expect(retry).toHaveBeenCalledTimes(1);
});

test("the menu is absent while the artwork has not failed", () => {
  artwork.state = { imageFailed: false, phase: "ready", retry: () => {} };
  const { container } = render(
    createElement(ArtworkRetryMenu, { release: "r" }),
  );
  expect(container.textContent).toBe("");
});

test("a real failure still offers a retry", () => {
  artwork.state = { imageFailed: true, phase: "failed", retry: () => {} };
  render(createElement(ArtworkStatus, { release: "r" }));
  expect(screen.getByText("Artwork could not be retrieved.")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Retry artwork" })).toBeTruthy();
});

test("a real failure keeps its own message and no duplicate menu", () => {
  artwork.state = { imageFailed: true, phase: "failed", retry: () => {} };
  const { container } = render(
    createElement(ArtworkRetryMenu, { release: "r" }),
  );
  expect(container.textContent).toBe("");
});
