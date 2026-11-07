import { useRef } from "react";
import { Link } from "@inertiajs/react";
import { LIBRARY_SECTIONS, currentSection } from "./destinations";
import { log } from "../shared/log";
import { useScrollFade } from "../shared/hooks/useScrollFade";

function SectionStrip({ current }: { current: string }) {
  const strip = useRef<HTMLElement>(null);
  useScrollFade(strip);
  return (
    <nav ref={strip} className="library-sections" aria-label="Library sections">
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

export default function LibrarySections({ path }: { path: string }) {
  const current = currentSection(path);
  return current ? <SectionStrip current={current} /> : null;
}
