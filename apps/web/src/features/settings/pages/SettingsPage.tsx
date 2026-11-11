import type { Schema } from "../../../shared/schema";
import { useEffect, useMemo, useRef, useState, type MouseEvent } from "react";
import { Link, router, usePage } from "@inertiajs/react";
import { ChevronLeft } from "lucide-react";

import PluginSettings from "../../plugins/components/PluginSettings";
import PageHeading from "../../../shared/ui/PageHeading";
import {
  UnsavedContext,
  createUnsavedRegistry,
} from "../../../shared/unsavedChanges";
import {
  useRevealSetting,
  type RevealRequest,
} from "../hooks/useRevealSetting";
import type { SearchHit } from "../model/settingsSearch";
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
  const { section, chosen, requested, anchor } = readLocation(
    url,
    props.user.admin,
  );
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
  const [request, setRequest] = useState<RevealRequest | null>(() =>
    anchor ? { anchor, serial: 0 } : null,
  );
  useEffect(() => {
    if (anchor)
      setRequest((before) =>
        before?.anchor === anchor
          ? before
          : { anchor, serial: (before?.serial ?? 0) + 1 },
      );
  }, [anchor]);
  useRevealSetting(request);
  function leaveConfirmed(): boolean {
    return (
      !unsaved.hasUnsaved() ||
      window.confirm("Leave without saving your changes?")
    );
  }
  function guard(event: MouseEvent) {
    const link = (event.target as Element).closest("a");
    if (link && !leaveConfirmed()) {
      event.preventDefault();
      event.stopPropagation();
    }
  }
  function choose(hit: SearchHit) {
    if (!leaveConfirmed()) return;
    const target = hit.href.split("#")[1] ?? null;
    if (target)
      setRequest((before) => ({
        anchor: target,
        serial: (before?.serial ?? 0) + 1,
      }));
    router.visit(hit.href, { preserveState: true, preserveScroll: true });
  }
  if (section === "plugins")
    return (
      <UnsavedContext.Provider value={unsaved}>
        <PluginSettings
          account={props.user.id}
          admin={props.user.admin}
          desktop={props.native_playback}
        />
      </UnsavedContext.Provider>
    );
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
          admin={props.user.admin}
          onChoose={choose}
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
