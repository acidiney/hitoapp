import { createElement } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import type { Schema } from "../../../shared/schema";
import DiscoverySearch from "../components/DiscoverySearch";

function connection(id: string, name: string): Schema<"PluginInstallation"> {
  return {
    id,
    service_id: `${id}-service`,
    revision: 1,
    checked_at: 1,
    credential_set: false,
    manifest: {
      id: `org.hito.${id}`,
      name,
      protocol_version: 1,
      capabilities: ["search", "sources"],
    },
  };
}

test("the music service is chosen from the shared listbox, never a native select", () => {
  const onChoose = vi.fn();
  const { container } = render(
    createElement(DiscoverySearch, {
      connections: [connection("one", "YouTube"), connection("two", "Spotify")],
      names: {},
      selected: "one",
      pending: false,
      onChoose,
      onSearch: vi.fn(async () => true),
    }),
  );
  expect(container.querySelector("select")).toBeNull();
  const control = screen.getByRole("combobox", { name: "Music service" });
  expect(control.textContent).toContain("YouTube");
  fireEvent.click(control);
  fireEvent.click(screen.getByRole("option", { name: "Spotify" }));
  expect(onChoose).toHaveBeenCalledWith("two");
});
