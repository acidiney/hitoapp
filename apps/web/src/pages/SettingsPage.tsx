import { useRef, useState, type KeyboardEvent } from "react";
import { usePage } from "@inertiajs/react";
import type { SettingsPageProps } from "../shared/types";
import AccountSettings from "../components/AccountSettings";
import LibrarySettings from "../components/LibrarySettings";
import PluginSettings from "../components/plugins/PluginSettings";
import MetadataPreferences from "../components/metadata/MetadataPreferences";
import CommunitySettings from "../components/CommunitySettings";
import CommunityRankingSettings from "../components/CommunityRankingSettings";
import PageHeading from "../shared/ui/PageHeading";
import { log } from "../shared/log";

export default function SettingsPage(props: SettingsPageProps) {
  const { url } = usePage();
  const [selected, setSelected] = useState(() => {
    const section = new URL(url, "http://hito.local").searchParams.get(
      "section",
    );
    return section === "plugins" ||
      section === "metadata" ||
      section === "privacy"
      ? section
      : "library";
  });
  const navigation = useRef<HTMLDivElement>(null);
  const tabs = [
    { id: "library", label: "Library" },
    { id: "privacy", label: "Privacy" },
    { id: "metadata", label: "Metadata" },
    { id: "plugins", label: "Plugins" },
    ...(props.user.admin ? [{ id: "accounts", label: "Accounts" }] : []),
  ];
  function selectTab(mode: string) {
    log.debug("settings.section", { mode });
    setSelected(mode);
  }
  function navigate(event: KeyboardEvent<HTMLButtonElement>, index: number) {
    const next =
      event.key === "ArrowRight"
        ? (index + 1) % tabs.length
        : event.key === "ArrowLeft"
          ? (index + tabs.length - 1) % tabs.length
          : event.key === "Home"
            ? 0
            : event.key === "End"
              ? tabs.length - 1
              : null;
    if (next === null) return;
    event.preventDefault();
    selectTab(tabs[next].id);
    navigation.current
      ?.querySelectorAll<HTMLButtonElement>("[role=tab]")
      [next]?.focus();
  }
  return (
    <>
      <PageHeading title="Settings" description="Make Hito feel at home." />
      <div className="settings">
        <div
          className="settings-tabs"
          role="tablist"
          aria-label="Settings sections"
          ref={navigation}
        >
          {tabs.map((tab, index) => (
            <button
              key={tab.id}
              role="tab"
              id={`tab-${tab.id}`}
              aria-selected={selected === tab.id}
              aria-controls="settings-content"
              tabIndex={selected === tab.id ? 0 : -1}
              onClick={() => selectTab(tab.id)}
              onKeyDown={(event) => navigate(event, index)}
            >
              {tab.label}
            </button>
          ))}
        </div>
        <div
          id="settings-content"
          role="tabpanel"
          aria-labelledby={`tab-${selected}`}
        >
          {selected === "plugins" && (
            <PluginSettings account={props.user.id} admin={props.user.admin} />
          )}
          {selected === "metadata" && (
            <MetadataPreferences key={props.user.id} account={props.user.id} />
          )}
          {selected === "library" && <LibrarySettings {...props} />}
          {selected === "privacy" && (
            <>
              <CommunitySettings
                key={`community-preferences:${props.user.id}`}
                account={props.user.id}
              />
              <CommunityRankingSettings
                key={`community-rankings:${props.user.id}`}
                account={props.user.id}
              />
            </>
          )}
          {selected === "accounts" && props.user.admin && (
            <AccountSettings
              key={props.user.id}
              account={props.user.id}
              library={props.libraries.find((library) => !library.personal)?.id}
            />
          )}
        </div>
      </div>
    </>
  );
}
