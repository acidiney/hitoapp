import { createElement } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test } from "vitest";
import SearchServicesSetting from "../../search/components/SearchServicesSetting";

test("the switch is on by default and the choice is kept per account", () => {
  const first = render(createElement(SearchServicesSetting, { account: "a" }));
  const toggle = () =>
    screen.getByRole("switch", { name: "Search connected services automatically" });
  expect(toggle().getAttribute("aria-checked")).toBe("true");
  fireEvent.click(toggle());
  expect(toggle().getAttribute("aria-checked")).toBe("false");
  expect(localStorage.getItem("hito:search-services-auto:a")).toBe("off");
  first.unmount();
  render(createElement(SearchServicesSetting, { account: "b" }));
  expect(toggle().getAttribute("aria-checked")).toBe("true");
});
