import { Link } from "@inertiajs/react";
import { LIBRARY_SECTIONS, currentSection } from "./destinations";
import { log } from "../shared/log";

export default function LibrarySections({ path }: { path: string }) {
  const current = currentSection(path);
  if (!current) return null;
  return (
    <nav className="library-sections" aria-label="Library sections">
      {LIBRARY_SECTIONS.map(({ href, label }) => (
        <Link
          key={href}
          href={href}
          aria-current={href === current ? "page" : undefined}
          onClick={() => log.info("navigation.requested", { route: href })}
        >
          {label}
        </Link>
      ))}
    </nav>
  );
}
