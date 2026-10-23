import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { expect, test } from "vitest";
import { buildPages, pages, resolvePage } from "../pages";

const contract = resolve(
  import.meta.dirname,
  "../../../../../apps/server/src/inertia/props.rs",
);

function serverComponents() {
  if (!existsSync(contract))
    throw new Error(
      `The Inertia page names are read from ${contract}, which does not exist`,
    );
  const names = [
    ...readFileSync(contract, "utf8").matchAll(/=> "([A-Za-z]+Page)"/g),
  ].map((match) => match[1]);
  return [...new Set(names)];
}

test("the server emits the page components the registry knows", () => {
  expect(serverComponents().length).toBeGreaterThanOrEqual(19);
});

test.each(serverComponents())(
  "the registry resolves the server component %s",
  (name) => {
    expect(resolvePage(name)).toBeTypeOf("function");
  },
);

test("every registered page is a server component and unknown names are rejected", () => {
  expect(Object.keys(pages).sort()).toEqual(serverComponents().sort());
  expect(() => resolvePage("MissingPage")).toThrow("Unknown page: MissingPage");
});

test("two features exporting the same page name fail registration", () => {
  const page = () => null;
  expect(() =>
    buildPages({
      "../features/a/pages/SamePage.tsx": { default: page },
      "../features/b/pages/SamePage.tsx": { default: page },
    }),
  ).toThrow("Page SamePage is exported by more than one feature");
});

test("a page file without a default component fails registration", () => {
  expect(() => buildPages({ "../features/a/pages/EmptyPage.tsx": {} })).toThrow(
    "../features/a/pages/EmptyPage.tsx has no default page component",
  );
});
