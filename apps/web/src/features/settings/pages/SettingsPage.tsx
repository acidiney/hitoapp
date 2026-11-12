import type { Schema } from "../../../shared/schema";
import { useEffect, useMemo, useRef, useState, type MouseEvent } from "react";
import { Link, router, usePage } from "@inertiajs/react";
import { ChevronLeft } from "lucide-react";

import PluginSettings from "../../plugins/components/PluginSettings";
import { accountName } from "../../../shared/accountName";
import Avatar from "../../../shared/ui/Avatar";
import {
  UnsavedContext,
  createUnsavedRegistry,
} from "../../../shared/unsavedChanges";
import {
  useRevealSetting,
  type RevealRequest,
} from "../hooks/useRevealSetting";
import { useSettingsSearch } from "../hooks/useSettingsSearch";
import type { SearchHit } from "../model/settingsSearch";
import SettingsNav from "../components/SettingsNav";
import {
  SettingsSearchField,
  SettingsSearchResults,
  useSearchListId,
} from "../components/SettingsSearch";
import SettingsSectionBody from "../components/SettingsSectionBody";
import {
  readLocation,
  sectionHref,
  sectionMeta,
  tabSections,
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
  const listId = useSearchListId();
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
  const search = useSettingsSearch(props.user.admin, choose);
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
      <div
        className="settings settings-page"
        data-view={chosen ? "section" : "list"}
        data-searching={search.searching || undefined}
        onClickCapture={guard}
      >
        <div className="settings-head">
          <h1 ref={pageHeading} tabIndex={-1}>
            Settings
          </h1>
          <SettingsSearchField search={search} listId={listId} />
        </div>
        {search.searching ? (
          <SettingsSearchResults search={search} listId={listId} />
        ) : (
          <>
            {!chosen && (
              <Link className="settings-account" href={sectionHref("profile")}>
                <Avatar
                  id={props.user.id}
                  name={accountName(props.user)}
                  version={props.user.avatar_version}
                />
                <span>
                  <span className="settings-account-name">
                    {accountName(props.user)}
                  </span>
                  <span className="settings-account-role">
                    {props.user.display_name && `${props.user.username} · `}
                    {props.user.admin ? "Administrator" : "Member"}
                  </span>
                </span>
              </Link>
            )}
            <SettingsNav
              sections={tabSections(props.user.admin)}
              current={section}
            />
            <div className="settings-content">
              <div className="settings-section-heading">
                <Link
                  className="settings-back"
                  href="/settings"
                  aria-label="All settings"
                >
                  <ChevronLeft size={20} aria-hidden="true" />
                </Link>
                <h2 id="settings-title" ref={sectionHeading} tabIndex={-1}>
                  {meta.label}
                </h2>
              </div>
              <section
                aria-labelledby="settings-title"
                className="settings-section"
              >
                <SettingsSectionBody section={section} {...props} />
              </section>
            </div>
          </>
        )}
      </div>
    </UnsavedContext.Provider>
  );
}
