const key = "hito-connection-reload";
const namePrefix = "hito-reload:";
export const reloadSpacingMs = 15_000;

let remembered = 0;

function stored() {
  try {
    const value = sessionStorage.getItem(key);
    if (value !== null) return Number(value) || 0;
  } catch {}
  if (window.name.startsWith(namePrefix))
    return Number(window.name.slice(namePrefix.length)) || 0;
  return remembered;
}

export function reloadWait(now = Date.now()) {
  return Math.max(0, reloadSpacingMs - (now - stored()));
}

export function markReload(now = Date.now()) {
  remembered = now;
  try {
    sessionStorage.setItem(key, String(now));
    return;
  } catch {}
  if (window.name === "" || window.name.startsWith(namePrefix))
    window.name = `${namePrefix}${now}`;
}

export function markPageLoaded() {
  remembered = 0;
  try {
    sessionStorage.removeItem(key);
  } catch {}
  if (window.name.startsWith(namePrefix)) window.name = "";
}
