import { settingEntries, type SettingEntry } from "./settingsCatalog";
import { sectionHref, visibleSections, type SectionId } from "./sections";

export type SearchHit = {
  key: string;
  kind: "section" | "setting";
  section: SectionId;
  sectionLabel: string;
  place: string;
  label: string;
  description?: string;
  href: string;
};

export function settingAnchor(id: string): string {
  return `setting-${id}`;
}

export function settingHref(entry: Pick<SettingEntry, "id" | "section">) {
  return `${sectionHref(entry.section)}#${settingAnchor(entry.id)}`;
}

function normalise(text: string): string {
  return text
    .normalize("NFD")
    .replace(/\p{Diacritic}/gu, "")
    .toLowerCase();
}

type Candidate = { hit: SearchHit; title: string; rest: string };

function candidates(admin: boolean): Candidate[] {
  const sections = visibleSections(admin);
  const labels = new Map(
    sections.map((section) => [section.id, section.label]),
  );
  const found: Candidate[] = sections.map((section) => ({
    hit: {
      key: `section:${section.id}`,
      kind: "section",
      section: section.id,
      sectionLabel: section.label,
      place: "Section",
      label: section.label,
      description: section.description,
      href: sectionHref(section.id),
    },
    title: normalise(section.label),
    rest: normalise(section.description),
  }));
  for (const entry of settingEntries()) {
    const sectionLabel = labels.get(entry.section);
    if (sectionLabel === undefined || (entry.admin && !admin)) continue;
    found.push({
      hit: {
        key: `setting:${entry.id}`,
        kind: "setting",
        section: entry.section,
        sectionLabel,
        place: entry.within
          ? `${sectionLabel} · ${entry.within}`
          : sectionLabel,
        label: entry.label,
        description: entry.description,
        href: settingHref(entry),
      },
      title: normalise(entry.label),
      rest: normalise(
        `${entry.description ?? ""} ${entry.keywords ?? ""} ${sectionLabel}`,
      ),
    });
  }
  return found;
}

function rank(candidate: Candidate, words: string[]): number | null {
  const inTitle = words.every((word) => candidate.title.includes(word));
  if (inTitle) return candidate.title.startsWith(words[0]) ? 0 : 1;
  const everywhere = `${candidate.title} ${candidate.rest}`;
  return words.every((word) => everywhere.includes(word)) ? 2 : null;
}

export function searchSettings(query: string, admin: boolean): SearchHit[] {
  const words = normalise(query).split(/\s+/).filter(Boolean);
  if (words.length === 0) return [];
  const ranked: { hit: SearchHit; score: number; order: number }[] = [];
  candidates(admin).forEach((candidate, order) => {
    const score = rank(candidate, words);
    if (score !== null) ranked.push({ hit: candidate.hit, score, order });
  });
  ranked.sort((a, b) => a.score - b.score || a.order - b.order);
  return ranked.map((item) => item.hit);
}

function isEditable(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    (target.isContentEditable ||
      ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName))
  );
}

export function isSettingsSearchShortcut(event: KeyboardEvent): boolean {
  if (event.defaultPrevented || event.isComposing || event.altKey) return false;
  return (
    event.key === "/" &&
    !event.ctrlKey &&
    !event.metaKey &&
    !isEditable(event.target)
  );
}
