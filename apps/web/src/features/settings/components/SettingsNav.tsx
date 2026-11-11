import { useId } from "react";
import { Link } from "@inertiajs/react";
import { ChevronRight } from "lucide-react";
import { log } from "../../../shared/log";
import {
  sectionHref,
  type SectionId,
  type SettingsSection,
} from "../model/sections";

export default function SettingsNav({
  sections,
  current,
}: {
  sections: SettingsSection[];
  current: SectionId;
}) {
  const key = useId();
  return (
    <nav className="settings-nav" aria-label="Settings sections">
      {sections.map((section) => (
        <Link
          key={section.id}
          href={sectionHref(section.id)}
          preserveState
          preserveScroll
          aria-current={current === section.id ? "page" : undefined}
          aria-labelledby={`${key}-${section.id}-label`}
          aria-describedby={`${key}-${section.id}-hint`}
          onClick={() => log.debug("settings.section", { mode: section.id })}
        >
          <span className="settings-nav-copy">
            <span
              id={`${key}-${section.id}-label`}
              className="settings-nav-label"
            >
              {section.label}
            </span>
            <span
              id={`${key}-${section.id}-hint`}
              className="settings-nav-hint"
            >
              {section.description}
            </span>
          </span>
          <ChevronRight size={16} aria-hidden="true" />
        </Link>
      ))}
    </nav>
  );
}
