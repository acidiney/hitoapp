import type { Schema } from "../../../shared/schema";
import { useEffect, useMemo, useRef, type MouseEvent } from "react";
import { Link, usePage } from "@inertiajs/react";
import { ChevronLeft } from "lucide-react";

import PageHeading from "../../../shared/ui/PageHeading";
import {
  UnsavedContext,
  createUnsavedRegistry,
} from "../../../shared/unsavedChanges";
import SettingsNav from "../components/SettingsNav";
import SettingsSectionBody from "../components/SettingsSectionBody";
import {
  readLocation,
  sectionHref,
  sectionMeta,
  visibleGroups,
} from "../model/sections";

export default function SettingsPage(props: Schema<"SettingsPageProps">) {
  const { url } = usePage();
  const { section, chosen, requested } = readLocation(url, props.user.admin);
  const meta = sectionMeta(section);
  const sectionHeading = useRef<HTMLHeadingElement>(null);
  const pageHeading = useRef<HTMLHeadingElement>(null);
  const previous = useRef({ section, chosen });
  const unsaved = useMemo(createUnsavedRegistry, []);
  useEffect(() => {
    const before = previous.current;
    previous.current = { section, chosen };
    if (before.section === section && before.chosen === chosen) return;
    (chosen ? sectionHeading : pageHeading).current?.focus();
  }, [section, chosen]);
  useEffect(() => {
    if (requested !== null && requested !== section)
      window.history.replaceState(
        window.history.state,
        "",
        sectionHref(section),
      );
  }, [requested, section]);
  function guard(event: MouseEvent) {
    const link = (event.target as Element).closest("a");
    if (
      link &&
      unsaved.hasUnsaved() &&
      !window.confirm("Leave without saving your changes?")
    ) {
      event.preventDefault();
      event.stopPropagation();
    }
  }
  return (
    <UnsavedContext.Provider value={unsaved}>
      <PageHeading title="Settings" headingRef={pageHeading} />
      <div
        className="settings"
        data-view={chosen ? "section" : "list"}
        onClickCapture={guard}
      >
        <SettingsNav
          groups={visibleGroups(props.user.admin)}
          current={section}
        />
        <div className="settings-content">
          <Link className="settings-back" href="/settings">
            <ChevronLeft size={18} aria-hidden="true" />
            All settings
          </Link>
          <section
            aria-labelledby="settings-title"
            className="settings-section"
          >
            <div className="settings-section-heading">
              <h2 id="settings-title" ref={sectionHeading} tabIndex={-1}>
                {meta.label}
              </h2>
              <p className="muted">{meta.description}</p>
            </div>
            <SettingsSectionBody section={section} {...props} />
          </section>
        </div>
      </div>
    </UnsavedContext.Provider>
  );
}
