import { createElement } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test } from "vitest";
import LyricsAutoSetting from "../../lyrics/components/LyricsAutoSetting";

test("Load lyrics automatically is on by default and kept per account", () => {
  render(createElement(LyricsAutoSetting, { account: "a" }));
  const toggle = screen.getByRole("switch", {
    name: "Load lyrics automatically",
  });
  expect(toggle.getAttribute("aria-checked")).toBe("true");
  fireEvent.click(toggle);
  expect(localStorage.getItem("hito:lyrics-auto:a")).toBe("off");
});
