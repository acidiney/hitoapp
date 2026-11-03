#!/bin/sh
set -eu
umask 077

hito_repository=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd -P)
case $(uname -s) in
  Darwin) hito_installation="$HOME/Library/Application Support/Hito" ;;
  Linux) hito_installation="${XDG_CONFIG_HOME:-$HOME/.config}/hito" ;;
  *) printf '%s\n' 'Use Hito Setup.ps1 on Windows.'; exit 1 ;;
esac
mkdir -p "$hito_installation"
chmod 700 "$hito_installation"
if ! mkdir "$hito_installation/setup.lock" 2>/dev/null; then
  printf '%s\n' 'Hito Setup is already open. Close it before trying again.'
  exit 1
fi
hito_stopped=0
hito_container=
finish() {
  hito_exit=$?
  if [ "$hito_exit" -ne 0 ]; then
    printf '%s\n' 'Hito setup failed. Your library data and original music files are preserved.'
    if [ "$hito_stopped" -eq 1 ] && [ -n "$hito_container" ]; then
      docker start "$hito_container" >/dev/null 2>&1 || true
    fi
  fi
  rmdir "$hito_installation/setup.lock"
}
trap finish EXIT

run() {
  hito_step=$1
  shift
  printf '{"event":"installation.%s.started"}\n' "$hito_step" >> "$hito_installation/setup.log"
  hito_output="$hito_installation/$hito_step.out"
  if "$@" > "$hito_output" 2>&1; then
    printf '{"event":"installation.%s.completed"}\n' "$hito_step" >> "$hito_installation/setup.log"
    rm -f "$hito_output"
  else
    printf '{"event":"installation.%s.failed"}\n' "$hito_step" >> "$hito_installation/setup.log"
    hito_reason=$(sed -n 's/.*"event":"installation.failed","message":"\(.*\)"}.*/\1/p' "$hito_output" | tail -n 1)
    if [ -n "$hito_reason" ]; then
      printf 'Hito could not finish the %s step: %s\n' "$hito_step" "$hito_reason"
    else
      printf 'Hito could not finish the %s step. The last lines it printed:\n' "$hito_step"
      tail -n 15 "$hito_output"
      printf 'The full output is in %s\n' "$hito_output"
    fi
    exit 1
  fi
}

run docker-check docker info
rm -f "$hito_installation/selection.json" "$hito_installation/selection.raw"
case $(uname -s) in
  Darwin)
    if ! osascript -l JavaScript > "$hito_installation/selection.json" 2>/dev/null <<'JXA'
ObjC.import('Foundation');
const app = Application.currentApplication();
app.includeStandardAdditions = true;
try {
  const folders = app.chooseFolder({ withPrompt: 'Choose music folders to share with Hito. Existing folders are kept.', multipleSelectionsAllowed: true });
  JSON.stringify(folders.map(folder => ObjC.unwrap($(String(folder)).stringByResolvingSymlinksInPath)));
} catch (error) {
  if (error.errorNumber === -128) '[]'; else throw error;
}
JXA
    then exit 1; fi
    [ "$(cat "$hito_installation/selection.json")" != '[]' ] || exit 0
    ;;
  Linux)
    if command -v zenity >/dev/null 2>&1; then
      hito_choice=$(zenity --file-selection --directory --title='Choose a music folder for Hito') || exit 0
    elif command -v kdialog >/dev/null 2>&1; then
      hito_choice=$(kdialog --getexistingdirectory "$HOME" --title 'Choose a music folder for Hito') || exit 0
    else
      printf '%s\n' 'Install zenity or kdialog to open the native folder chooser.'
      exit 1
    fi
    hito_choice=$(CDPATH= cd -- "$hito_choice" && pwd -P)
    printf '%s' "$hito_choice" > "$hito_installation/selection.raw"
    docker run --rm -i node:22-bookworm-slim node --input-type=module -e 'let value=""; for await (const chunk of process.stdin) value += chunk; process.stdout.write(JSON.stringify([value]));' < "$hito_installation/selection.raw" > "$hito_installation/selection.json"
    rm "$hito_installation/selection.raw"
    ;;
esac

if [ -f "$hito_installation/plan.txt" ]; then
  read -r hito_saved_project < "$hito_installation/plan.txt"
  hito_container=$(docker ps -a --filter 'label=com.docker.compose.service=server' --filter "label=com.docker.compose.project=$hito_saved_project" --format '{{.ID}}')
else
  hito_container=$(docker ps -a --filter 'label=com.docker.compose.service=server' --filter "label=com.docker.compose.project.working_dir=$hito_repository" --format '{{.ID}}')
fi
case "$hito_container" in *'
'*) printf '%s\n' 'More than one Hito installation was found. No configuration was changed.'; exit 1 ;; esac
if [ -n "$hito_container" ]; then
  docker inspect --format '{"project":{{json (index .Config.Labels "com.docker.compose.project")}},"mounts":{{json .Mounts}},"ports":{{json (index .HostConfig.PortBindings "8080/tcp")}}}' "$hito_container" > "$hito_installation/probe.json"
else
  printf 'null\n' > "$hito_installation/probe.json"
fi
printf '%s\n' 'Preparing read-only music folders…'
run configuration docker run --rm --user "$(id -u):$(id -g)" -v "$hito_repository:/workspace:ro" -v "$hito_installation:/setup" node:22-bookworm-slim node /workspace/scripts/setup/prepare.mjs --installation /setup --host-installation "$hito_installation" --repository "$hito_repository"
{
  read -r hito_project
  read -r hito_volume
  read -r hito_port
} < "$hito_installation/plan.txt"
printf '%s\n' 'Building Hito in Docker. The first build can take several minutes…'
run build docker compose -p "$hito_project" -f "$hito_installation/compose.json" build server
if docker volume inspect "$hito_volume" >/dev/null 2>&1; then
  mkdir -p "$hito_installation/backups"
  run stop docker compose -p "$hito_project" -f "$hito_installation/compose.json" stop server
  hito_stopped=1
  hito_image=$(docker image inspect "hito-local/server:$hito_project" --format '{{.Id}}')
  hito_backup="$hito_installation/backups/hito-$(date -u +%Y%m%dT%H%M%SZ)-$$.tar.gz"
  if ! docker run --rm --user 0 --mount "type=volume,source=$hito_volume,target=/data,readonly" --entrypoint tar "$hito_image" -C /data -czf - . > "$hito_backup" 2>/dev/null; then
    rm -f "$hito_backup"
    printf '{"event":"installation.backup.failed"}\n' >> "$hito_installation/setup.log"
    exit 1
  fi
  printf '{"event":"installation.backup.completed"}\n' >> "$hito_installation/setup.log"
fi
run start docker compose -p "$hito_project" -f "$hito_installation/compose.json" up -d server
hito_stopped=0
printf '%s\n' 'Hito is starting. Add the shared folders in Settings → Library.'
case $(uname -s) in
  Darwin) open "http://localhost:$hito_port/settings" ;;
  Linux) xdg-open "http://localhost:$hito_port/settings" >/dev/null 2>&1 || true ;;
esac
