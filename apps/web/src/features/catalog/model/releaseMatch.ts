import type { Schema } from "../../../shared/schema";

type ReleaseGroup = Schema<"CatalogReleaseGroup">;

const kindLabels = {
  album: "Album",
  ep: "EP",
  single: "Single",
  broadcast: "Broadcast",
  other: "Other",
};

function plainTitle(title: string) {
  return title
    .normalize("NFKD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .replace(/\([^)]*\)|\[[^\]]*\]/g, " ")
    .replace(/[^\p{L}\p{N}]+/gu, " ")
    .trim();
}

export function releaseYear(group: ReleaseGroup) {
  return group.first_release_date?.slice(0, 4) ?? null;
}

export function groupSubtitle(group: ReleaseGroup) {
  const types = group.secondary_types.map((type, index) =>
    index === 0 ? type : type.toLowerCase(),
  );
  return [
    releaseYear(group),
    types.length ? types.join(", ") : group.kind ? kindLabels[group.kind] : "",
  ]
    .filter(Boolean)
    .join(" · ");
}

export function suggestedSubtitle(group: ReleaseGroup) {
  const credit = group.artist_credit
    ?.map((member) => member.credited_name + member.join_phrase)
    .join("");
  return [
    credit,
    releaseYear(group),
    group.kind ? kindLabels[group.kind] : "",
    "same title as yours",
  ]
    .filter(Boolean)
    .join(" · ");
}

export function matchReleases(
  groups: ReleaseGroup[],
  title: string,
  year: number | null,
) {
  const wanted = plainTitle(title);
  const same = wanted
    ? groups.filter((group) => plainTitle(group.title) === wanted)
    : [];
  const rank = (group: ReleaseGroup) =>
    (year !== null && releaseYear(group) === String(year) ? 0 : 2) +
    (group.kind === "album" ? 0 : 1);
  const ordered = [...same].sort((a, b) => rank(a) - rank(b));
  return { suggestion: ordered[0] ?? null, unique: ordered.length === 1 };
}
