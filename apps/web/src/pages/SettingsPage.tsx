import { useRef, useState, type KeyboardEvent } from "react";
import type { SettingsPageProps } from "../types";
import AccountSettings from "../components/AccountSettings";
import LibrarySettings from "../components/LibrarySettings";
import PageHeading from "../components/PageHeading";
import { log } from "../log";

export default function SettingsPage(props: SettingsPageProps) {
  const [selected, setSelected] = useState("library");
  const navigation = useRef<HTMLDivElement>(null);
  const tabs = [
    { id: "library", label: "Library" },
    { id: "privacy", label: "Privacy" },
    ...(props.user.admin ? [{ id: "accounts", label: "Accounts" }] : []),
  ];
  function selectTab(mode: string) {
    log.debug("settings.section", { mode, account_id: props.user.id });
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
          {selected === "library" && <LibrarySettings {...props} />}
          {selected === "privacy" && (
            <section className="settings-panel">
              <div className="settings-intro">
                <h2>A private place to listen</h2>
                <p>
                  Your playlists, likes, and listening history belong to your
                  account.
                </p>
              </div>
              <div className="setting-row">
                <div>
                  <h3>Playback reporting</h3>
                  <p>No playback outcomes are sent to a community service.</p>
                </div>
                <span className="setting-badge">Off</span>
              </div>
              <div className="setting-row">
                <div>
                  <h3>Metadata contributions</h3>
                  <p>No library corrections are shared outside this server.</p>
                </div>
                <span className="setting-badge">Off</span>
              </div>
            </section>
          )}
          {selected === "accounts" && props.user.admin && (
            <AccountSettings
              key={props.user.id}
              account={props.user.id}
              library={props.libraries[0]?.id}
            />
          )}
        </div>
      </div>
    </>
  );
}
