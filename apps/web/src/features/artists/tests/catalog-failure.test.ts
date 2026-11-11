import { createElement } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import CatalogFailure from "../components/CatalogFailure";

test("a failure is a short alert with a text action and no pill button", () => {
  const retry = vi.fn();
  render(
    createElement(CatalogFailure, {
      message: "Editions are unavailable. Try again.",
      retryLabel: "Try editions again",
      pending: false,
      onRetry: retry,
    }),
  );
  expect(screen.getByRole("alert").textContent).toBe(
    "Editions are unavailable. Try again.",
  );
  const action = screen.getByRole("button", { name: "Try editions again" });
  expect(action.textContent).toBe("Try again");
  expect(action.className).toBe("text-action");
  fireEvent.click(action);
  expect(retry).toHaveBeenCalledTimes(1);
});

test("restart replaces the retry when the list must start over", () => {
  const restart = vi.fn();
  render(
    createElement(CatalogFailure, {
      message: "Browse again.",
      retryLabel: "Try editions again",
      pending: false,
      onRestart: restart,
    }),
  );
  expect(
    screen.queryByRole("button", { name: "Try editions again" }),
  ).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Restart editions" }));
  expect(restart).toHaveBeenCalledTimes(1);
});
