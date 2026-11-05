import { useState } from "react";
import { Link } from "@inertiajs/react";
import { ChevronRight } from "lucide-react";
import { log } from "../../../shared/log";
import {
  sectionHref,
  type SectionGroup,
  type SectionId,
} from "../model/sections";
import type { SearchHit } from "../model/settingsSearch";
import SettingsSearch from "./SettingsSearch";

export default function SettingsNav({
  groups,
  current,
  admin,
  onChoose,
}: {
  groups: SectionGroup[];
  current: SectionId;
  admin: boolean;
  onChoose: (hit: SearchHit) => void;
}) {
  const [query, setQuery] = useState("");
  const searching = query.trim() !== "";

  return (
    <nav className="settings-nav" aria-label="Settings sections">
      <SettingsSearch
        admin={admin}
        query={query}
        onQuery={setQuery}
        onChoose={(hit) => {
          setQuery("");
          onChoose(hit);
        }}
      />
      {!searching &&
        groups.map((group) => (
          <div key={group.label} role="group" aria-label={group.label}>
            <p className="settings-nav-group" aria-hidden="true">
              {group.label}
            </p>
            {group.sections.map((section) => (
              <Link
                key={section.id}
                href={sectionHref(section.id)}
                preserveState
                preserveScroll
                aria-current={current === section.id ? "page" : undefined}
                onClick={() =>
                  log.debug("settings.section", { mode: section.id })
                }
              >
                {section.label}
                <ChevronRight size={16} aria-hidden="true" />
              </Link>
            ))}
          </div>
        ))}
    </nav>
  );
}
