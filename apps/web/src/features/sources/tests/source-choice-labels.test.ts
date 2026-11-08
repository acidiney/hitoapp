import { createElement } from "react";
import { render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import type { Schema } from "../../../shared/schema";
import SourceChoiceControls from "../components/SourceChoiceControls";
import { automaticChoice, choiceSourceA } from "../fixtures/sourceChoice";

function renderRow(source: Schema<"Source">) {
  render(
    createElement(SourceChoiceControls, {
      choice: {
        ...automaticChoice,
        sources: [{ source, selected: false, selectable: true }],
      },
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
      fixSource: vi.fn(),
    }),
  );
  return screen.getAllByRole("listitem")[0];
}

test("a service source leads with the service name and shows the audio format as secondary text", () => {
  const row = renderRow({
    ...choiceSourceA,
    label: "AAC / M4A",
    provider_label: "YouTube",
  });
  expect(within(row).getByRole("radio", { name: "YouTube" })).toBeTruthy();
  expect(within(row).getByText("AAC · M4A")).toBeTruthy();
  expect(within(row).queryByText("plugin")).toBeNull();
  expect(within(row).queryByText("AAC / M4A")).toBeNull();
});

test("a service source without a known name never shows the word plugin", () => {
  const row = renderRow({ ...choiceSourceA, label: "AAC / M4A" });
  expect(within(row).queryByText("plugin")).toBeNull();
  expect(within(row).getByText("Music service")).toBeTruthy();
});

test("a local source keeps its own label and the music folder origin", () => {
  const row = renderRow({
    ...choiceSourceA,
    provider: "local",
    label: "Local file",
  });
  expect(within(row).getByRole("radio", { name: "Local file" })).toBeTruthy();
  expect(within(row).getByText("Music folder")).toBeTruthy();
});
