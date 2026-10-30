import { createElement } from "react";
import { render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import ArtworkStatus from "../components/ArtworkStatus";

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

test("no provider shows no message, only a quiet retry", () => {
  artwork.state = { imageFailed: true, phase: "unavailable", retry: () => {} };
  const { container } = render(createElement(ArtworkStatus, { release: "r" }));
  expect(container.textContent).toBe("Retry artwork");
  expect(screen.queryByRole("status")).toBeNull();
  expect(
    container.querySelector(".artwork-status")?.getAttribute("data-quiet"),
  ).toBe("true");
});

test("a real failure still offers a retry", () => {
  artwork.state = { imageFailed: true, phase: "failed", retry: () => {} };
  render(createElement(ArtworkStatus, { release: "r" }));
  expect(screen.getByText("Artwork could not be retrieved.")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Retry artwork" })).toBeTruthy();
});
