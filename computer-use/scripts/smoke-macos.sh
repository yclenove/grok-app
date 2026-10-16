#!/usr/bin/env bash
# Smoke TextEdit, Finder, and System Settings with system apps only.
# No Apple ID login. Temp files live under mktemp. The orchestrator runs this
# on a Mac; Linux CI does not execute it.
set -euo pipefail

WORKDIR="$(mktemp -d "${TMPDIR:-/tmp}/gcu-smoke-macos.XXXXXX")"
NOTE="${WORKDIR}/note.txt"
FOLDER="${WORKDIR}/Sidebar Folder"
SAMPLE="gcu smoke macOS 中文"
TEXTEDIT_WAS_RUNNING=0
SETTINGS_WAS_RUNNING=0
SETTINGS_APP=""

cleanup() {
  if [[ "${TEXTEDIT_WAS_RUNNING}" -eq 0 ]]; then
    osascript -e 'tell application "TextEdit" to quit' >/dev/null 2>&1 || true
  else
    osascript -e "tell application \"TextEdit\" to close (every document whose path is \"${NOTE}\")" >/dev/null 2>&1 || true
  fi
  osascript -e "tell application \"Finder\" to close (every Finder window whose target is (POSIX file \"${FOLDER}\" as alias))" >/dev/null 2>&1 || true
  if [[ -n "${SETTINGS_APP}" && "${SETTINGS_WAS_RUNNING}" -eq 0 ]]; then
    osascript -e "tell application \"${SETTINGS_APP}\" to quit" >/dev/null 2>&1 || true
  fi
  rm -rf "${WORKDIR}"
}
trap cleanup EXIT

if pgrep -qx TextEdit >/dev/null 2>&1; then
  TEXTEDIT_WAS_RUNNING=1
fi

osascript <<APPLESCRIPT
tell application "TextEdit"
  activate
  set theDoc to make new document
  set text of theDoc to "${SAMPLE}"
  save theDoc in POSIX file "${NOTE}"
end tell
APPLESCRIPT

saved=0
for _ in $(seq 1 40); do
  if [[ -s "${NOTE}" ]] && grep -F -q -- "${SAMPLE}" "${NOTE}"; then
    saved=1
    break
  fi
  sleep 0.2
done
if [[ "${saved}" -ne 1 ]]; then
  echo "TextEdit did not save ${NOTE}" >&2
  exit 1
fi

mkdir -p "${FOLDER}"
osascript <<APPLESCRIPT
tell application "Finder"
  set theWindow to make new Finder window
  set target of theWindow to (POSIX file "${FOLDER}" as alias)
end tell
APPLESCRIPT

navigated=0
for _ in $(seq 1 40); do
  current="$(osascript -e 'tell application "Finder" to POSIX path of (target of front Finder window as alias)' 2>/dev/null || true)"
  current="${current%/}"
  if [[ "${current}" == "${FOLDER}" ]]; then
    navigated=1
    break
  fi
  sleep 0.2
done
if [[ "${navigated}" -ne 1 ]]; then
  echo "Finder sidebar target was not ${FOLDER}" >&2
  exit 1
fi

if [[ -d "/System/Applications/System Settings.app" ]]; then
  SETTINGS_APP="System Settings"
elif [[ -d "/System/Applications/System Preferences.app" ]]; then
  SETTINGS_APP="System Preferences"
else
  echo "System Settings is not installed" >&2
  exit 1
fi
if pgrep -qx "${SETTINGS_APP}" >/dev/null 2>&1; then
  SETTINGS_WAS_RUNNING=1
fi
open -a "${SETTINGS_APP}"

front=0
for _ in $(seq 1 40); do
  name="$(osascript -e 'tell application "System Events" to get name of first application process whose frontmost is true' 2>/dev/null || true)"
  if [[ "${name}" == "${SETTINGS_APP}" ]]; then
    front=1
    break
  fi
  sleep 0.2
done
if [[ "${front}" -ne 1 ]]; then
  echo "${SETTINGS_APP} did not become frontmost" >&2
  exit 1
fi

osascript -e "tell application \"System Events\" to tell process \"${SETTINGS_APP}\" to set frontmost to true"
osascript -e "tell application \"System Events\" to tell process \"${SETTINGS_APP}\" to keystroke \"f\" using command down"
osascript -e "tell application \"System Events\" to tell process \"${SETTINGS_APP}\" to keystroke \"Displays\""

found=0
for _ in $(seq 1 40); do
  if osascript -e "tell application \"System Events\" to tell process \"${SETTINGS_APP}\" to (name of entire contents of window 1) contains \"Displays\"" >/dev/null 2>&1; then
    found=1
    break
  fi
  sleep 0.2
done
if [[ "${found}" -ne 1 ]]; then
  echo "${SETTINGS_APP} search did not show Displays" >&2
  exit 1
fi

echo "smoke-macos: TextEdit, Finder, and ${SETTINGS_APP} ok"
