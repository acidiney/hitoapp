import { type ComponentType, type ReactNode } from "react";
import Shell from "./Shell";

type PageComponent = ComponentType<any> & {
  layout?: (page: ReactNode) => ReactNode;
};

const modules = import.meta.glob<{ default: PageComponent }>(
  "../features/*/pages/*.tsx",
  { eager: true },
);

const layout = (page: ReactNode) => <Shell>{page}</Shell>;
const unframed = new Set(["LoginPage"]);

export const pages: Record<string, PageComponent> = Object.fromEntries(
  Object.entries(modules).map(([path, module]) => {
    const name = path.slice(path.lastIndexOf("/") + 1, -".tsx".length);
    const page = module.default;
    return [name, unframed.has(name) ? page : Object.assign(page, { layout })];
  }),
);

export function resolvePage(name: string): PageComponent {
  if (!Object.hasOwn(pages, name)) throw new Error(`Unknown page: ${name}`);
  return pages[name];
}
