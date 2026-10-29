import { useEffect, useRef } from "react";
import { Link, usePage } from "@inertiajs/react";
import { ChevronLeft } from "lucide-react";
import type { SettingsPageProps } from "../../../shared/types";
import PageHeading from "../../../shared/ui/PageHeading";
import SettingsNav from "../components/SettingsNav";
import SettingsSectionBody from "../components/SettingsSectionBody";
import { readLocation, sectionMeta, visibleGroups } from "../sections";

export default function SettingsPage(props: SettingsPageProps) {
  const { url } = usePage();
  const { section, chosen } = readLocation(url, props.user.admin);
  const meta = sectionMeta(section);
  const heading = useRef<HTMLHeadingElement>(null);
  const previous = useRef(section);
  useEffect(() => {
    if (previous.current !== section) heading.current?.focus();
    previous.current = section;
  }, [section]);
  return (
    <>
      <PageHeading title="Settings" description="Make Hito feel at home." />
      <div className="settings" data-view={chosen ? "section" : "list"}>
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
              <h2 id="settings-title" ref={heading} tabIndex={-1}>
                {meta.label}
              </h2>
              <p className="muted">{meta.description}</p>
            </div>
            <SettingsSectionBody section={section} {...props} />
          </section>
        </div>
      </div>
    </>
  );
}
