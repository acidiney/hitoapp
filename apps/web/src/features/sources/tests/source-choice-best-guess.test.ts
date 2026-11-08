import { createElement } from "react";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import type { Schema } from "../../../shared/schema";
import SourceChoiceControls from "../components/SourceChoiceControls";
import { automaticChoice, choiceSourceA } from "../fixtures/sourceChoice";

function choiceWith(source: Schema<"Source">): Schema<"SourceChoice"> {
  return {
    ...automaticChoice,
    sources: [{ source, selected: false, selectable: true }],
  };
}

function renderControls(source: Schema<"Source">, fixSource = vi.fn()) {
  render(
    createElement(SourceChoiceControls, {
      choice: choiceWith(source),
      stagedSource: null,
      checking: false,
      pending: false,
      retrying: false,
      unavailable: false,
      error: "",
      statusError: "",
      blocked: false,
      canChoose: false,
      canReset: false,
      stage: vi.fn(),
      choose: vi.fn(),
      reset: vi.fn(),
      retry: vi.fn(),
      check: vi.fn(),
      fixSource,
    }),
  );
  return fixSource;
}

test("a source taken from the provider ranking is marked as a best guess with the provider named", () => {
  const fixSource = renderControls({
    ...choiceSourceA,
    match_kind: "best_guess",
    provider_label: "Owned music service",
  });
  const row = screen.getAllByRole("listitem")[0];
  const badge = within(row).getByText("Best guess");
  expect(badge.getAttribute("title")).toBe(
    "Chosen from Owned music service's top result because no exact match was found.",
  );
  fireEvent.click(within(row).getByRole("button", { name: "Fix source" }));
  expect(fixSource).toHaveBeenCalledTimes(1);
});

test("a best guess without a known provider name still explains itself", () => {
  renderControls({ ...choiceSourceA, match_kind: "best_guess" });
  expect(screen.getByText("Best guess").getAttribute("title")).toBe(
    "Chosen from the music service's top result because no exact match was found.",
  );
});

test("a verified source shows no best guess marker or fix action", () => {
  renderControls({ ...choiceSourceA, provider_label: "Owned music service" });
  expect(screen.queryByText("Best guess")).toBeNull();
  expect(screen.queryByRole("button", { name: "Fix source" })).toBeNull();
});
