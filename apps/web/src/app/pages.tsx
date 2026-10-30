import { type ComponentType, type ReactNode } from "react";
import Shell from "./Shell";
import NotFoundPage from "./NotFoundPage";

type PageComponent = ComponentType<any> & {
  layout?: (page: ReactNode) => ReactNode;
};
type PageModules = Record<string, { default?: PageComponent }>;

const layout = (page: ReactNode) => <Shell>{page}</Shell>;
const unframed = new Set(["LoginPage", "LockPage"]);

export function buildPages(modules: PageModules) {
  const pages: Record<string, PageComponent> = {};
  for (const [path, module] of Object.entries(modules)) {
    const name = path.slice(path.lastIndexOf("/") + 1, -".tsx".length);
    const page = module.default;
    if (typeof page !== "function")
      throw new Error(`${path} has no default page component`);
    if (Object.hasOwn(pages, name))
      throw new Error(`Page ${name} is exported by more than one feature`);
    pages[name] = unframed.has(name) ? page : Object.assign(page, { layout });
  }
  return pages;
}

export const pages = buildPages(
  import.meta.glob<{ default: PageComponent }>("../features/*/pages/*.tsx", {
    eager: true,
  }),
);

const framedNotFound: PageComponent = Object.assign(NotFoundPage, { layout });

export function resolvePage(name: string): PageComponent {
  if (name === "NotFoundPage") return framedNotFound;
  if (!Object.hasOwn(pages, name)) throw new Error(`Unknown page: ${name}`);
  return pages[name];
}
