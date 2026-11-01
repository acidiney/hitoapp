const MUSICBRAINZ_ID =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

export function isMusicBrainzId(value: string): boolean {
  return MUSICBRAINZ_ID.test(value);
}
