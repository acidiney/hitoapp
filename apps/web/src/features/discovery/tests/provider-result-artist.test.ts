import type { Schema } from "../../../shared/schema";
import { createElement, type ReactNode } from "react";
import { render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import ProviderResults from "../components/ProviderResults";

vi.mock("@inertiajs/react", () => ({
  Link: ({ href, children, ...rest }: { href: string; children: ReactNode }) =>
    createElement("a", { href, ...rest }, children),
  router: { visit: vi.fn() },
}));

const saved: Schema<"Recording"> = {
  id: "saved",
  library_id: "lib",
  title: "Fancy",
  artist: "Library artist",
  inherited_title: "Fancy",
  inherited_artist: "Library artist",
  duration_ms: 1000,
  available: true,
  plays_from_services: true,
  liked: false,
};

function show(items: Schema<"ProviderRecording">[]) {
  render(
    createElement(ProviderResults, {
      items,
      saved: new Map([["owned", saved]]),
      selection: null,
      pending: false,
      canPlay: true,
      service: "Video service",
      onSelect: vi.fn(),
    }),
  );
}

test("a result without a service artist shows the artist of the matching library track", () => {
  show([
    { id: "owned", title: "Fancy", artist: "" },
    { id: "other", title: "Elsewhere", artist: "Unknown artist" },
  ]);
  expect(
    screen.getByText("Library artist · Video service · in your library"),
  ).toBeTruthy();
  expect(screen.getByText("Video service · not in your library")).toBeTruthy();
});

test("the service artist wins when the service supplies one", () => {
  show([{ id: "owned", title: "Fancy", artist: "Service artist" }]);
  expect(
    screen.getByText("Service artist · Video service · in your library"),
  ).toBeTruthy();
});
