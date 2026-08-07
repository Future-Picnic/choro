#!/bin/zsh
# Rebuilds Choro in an external Terminal, then replaces and relaunches the app.
set -euo pipefail

SCRIPT_PATH="${0:A}"
REPO_ROOT="${SCRIPT_PATH:h:h}"
APP_NAME="Choro"
BUNDLE_ID="com.ritmus.myide"
BUILT_BUNDLE="$REPO_ROOT/target/release/bundle/$APP_NAME.app"
INSTALL_BUNDLE="/Applications/$APP_NAME.app"
INSTALLED_EXECUTABLE="$INSTALL_BUNDLE/Contents/MacOS/choro"

usage() {
  cat <<EOF
Usage: ${SCRIPT_PATH:t} [--help]

Opens a separate macOS Terminal window and, after confirmation:
  1. Builds a fresh Choro.app without touching the installed app.
  2. Quits the installed Choro app.
  3. Replaces /Applications/Choro.app with the fresh build.
  4. Opens the newly installed Choro app.
EOF
}

installed_choro_pids() {
  ps -axo pid=,command= | awk -v executable="$INSTALLED_EXECUTABLE" \
    '$2 == executable { print $1 }'
}

wait_for_choro_to_exit() {
  local attempts=0
  while [[ -n "$(installed_choro_pids)" && "$attempts" -lt 20 ]]; do
    sleep 1
    (( attempts += 1 ))
  done

  [[ -z "$(installed_choro_pids)" ]]
}

run_rebuild() {
  if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "Error: this script only supports macOS." >&2
    return 1
  fi

  if [[ ! -x "$REPO_ROOT/scripts/bundle.sh" ]]; then
    echo "Error: bundle script is missing or not executable:" >&2
    echo "  $REPO_ROOT/scripts/bundle.sh" >&2
    return 1
  fi

  echo "Choro rebuild and reinstall"
  echo
  echo "Repository: $REPO_ROOT"
  echo "Installed app: $INSTALL_BUNDLE"
  echo
  echo "This will replace the generated Choro.app and the installed Choro.app."
  echo "The running app stays open while the build runs and quits only after a successful build."
  echo
  read "reply?Continue? [y/N] "
  if [[ ! "$reply" =~ '^[Yy]$' ]]; then
    echo "Cancelled; no files were changed by this script."
    return 0
  fi

  echo
  echo "==> Building Choro.app"
  (
    cd "$REPO_ROOT"
    CHORO_INSTALL_TO_APPLICATIONS=0 ./scripts/bundle.sh
  )

  if [[ ! -x "$BUILT_BUNDLE/Contents/MacOS/choro" ]]; then
    echo "Error: the build completed without producing a valid Choro.app." >&2
    return 1
  fi

  echo
  echo "==> Quitting the installed Choro app"
  if [[ -n "$(installed_choro_pids)" ]]; then
    osascript -e "tell application id \"$BUNDLE_ID\" to quit" >/dev/null 2>&1 || true
    if ! wait_for_choro_to_exit; then
      echo "Error: Choro did not quit after 20 seconds." >&2
      echo "The fresh build is ready at: $BUILT_BUNDLE" >&2
      echo "Close Choro manually, then run this script again." >&2
      return 1
    fi
  else
    echo "Choro is not currently running."
  fi

  local install_work_dir
  install_work_dir="$(mktemp -d "/Applications/.choro-reinstall.XXXXXX")"
  local staged_bundle="$install_work_dir/$APP_NAME.app"
  local backup_bundle="$install_work_dir/$APP_NAME.previous.app"
  local old_app_moved=0

  rollback_install() {
    local exit_status=$?
    trap - EXIT INT TERM

    if [[ "$old_app_moved" -eq 1 && -d "$backup_bundle" ]]; then
      echo "Restoring the previous Choro.app after an install error..." >&2
      if [[ -e "$INSTALL_BUNDLE" ]]; then
        rm -rf "$INSTALL_BUNDLE"
      fi
      mv "$backup_bundle" "$INSTALL_BUNDLE"
    fi

    rm -rf "$install_work_dir"
    return "$exit_status"
  }
  trap rollback_install EXIT INT TERM

  echo
  echo "==> Installing $INSTALL_BUNDLE"
  ditto "$BUILT_BUNDLE" "$staged_bundle"

  if [[ -e "$INSTALL_BUNDLE" ]]; then
    mv "$INSTALL_BUNDLE" "$backup_bundle"
    old_app_moved=1
  fi
  mv "$staged_bundle" "$INSTALL_BUNDLE"

  echo "==> Opening the rebuilt Choro app"
  open "$INSTALL_BUNDLE"

  old_app_moved=0
  rm -rf "$install_work_dir"
  trap - EXIT INT TERM

  echo
  echo "Done: rebuilt, installed, and reopened $INSTALL_BUNDLE"
}

case "${1:-}" in
  --help|-h)
    usage
    ;;
  --external-worker)
    run_rebuild
    ;;
  "")
    if [[ "$(uname -s)" != "Darwin" ]]; then
      echo "Error: this script only supports macOS." >&2
      exit 1
    fi

    osascript - "$SCRIPT_PATH" <<'APPLESCRIPT'
on run argv
  set scriptPath to item 1 of argv
  tell application "Terminal"
    activate
    do script (quoted form of scriptPath & " --external-worker")
  end tell
end run
APPLESCRIPT
    echo "Opened the Choro rebuild in an external Terminal window."
    ;;
  *)
    echo "Unknown option: $1" >&2
    usage >&2
    exit 2
    ;;
esac
