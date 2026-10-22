import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { expect, test } from "vitest";
import { pages, resolvePage } from "../pages";

const serverComponents = [
  ...readFileSync(
    resolve("../../crates/hito-server/src/inertia/props.rs"),
    "utf8",
  ).matchAll(/=> "([A-Za-z]+Page)"/g),
].map((match) => match[1]);

test("the server emits the page components the registry knows", () => {
  expect(serverComponents.length).toBeGreaterThanOrEqual(19);
});

test.each([...new Set(serverComponents)])(
  "the registry resolves the server component %s",
  (name) => {
    expect(resolvePage(name)).toBeTypeOf("function");
  },
);

test("every registered page is a server component and unknown names are rejected", () => {
  expect(Object.keys(pages).sort()).toEqual(
    [...new Set(serverComponents)].sort(),
  );
  expect(() => resolvePage("MissingPage")).toThrow("Unknown page: MissingPage");
});
