import { createElement } from "react";
import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";
import { HitoLogo, HitoMark } from "../brand/HitoLogo";

test("the logo is the supplied image, 15 px tall, with a dark-scheme source", () => {
  const { container } = render(createElement(HitoLogo));
  const image = screen.getByRole("img", { name: "hito" });
  expect(image.getAttribute("height")).toBe("15");
  expect(image.getAttribute("src")).toMatch(/hito-logo-on-light/);
  const dark = container.querySelector("source");
  expect(dark?.getAttribute("media")).toBe("(prefers-color-scheme: dark)");
  expect(dark?.getAttribute("srcset")).toMatch(/hito-logo(?!-on-light)/);
  expect(container.querySelector("[data-ring]")).toBeNull();
});

test("a panel that is always dark uses the dark logo only", () => {
  const { container } = render(
    createElement(HitoLogo, { height: 20, alwaysDark: true }),
  );
  const image = screen.getByRole("img", { name: "hito" });
  expect(image.getAttribute("height")).toBe("20");
  expect(image.getAttribute("src")).not.toMatch(/on-light/);
  expect(container.querySelector("source")).toBeNull();
});

test("the mark alone draws 10 px dots", () => {
  render(createElement(HitoMark));
  const image = screen.getByRole("img", { name: "hito" });
  expect(image.getAttribute("height")).toBe("10");
  expect(image.getAttribute("src")).toMatch(/hito-mark/);
});
