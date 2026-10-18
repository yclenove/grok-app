#!/usr/bin/env bash
# Invoked only by the macOS signing step. Never enable xtrace here.
set -euo pipefail
state="$(target/debug/xtask release --signing-status --target "$GCU_TARGET")"
if [[ "$state" == ad-hoc ]]; then
  echo 'Apple certificate absent: keeping identifier-only ad-hoc helper; no notarization.'
  exit 0
fi
sign_dir="$(mktemp -d "$RUNNER_TEMP/gcu-sign.XXXXXX")"
keychain="$sign_dir/release.keychain-db"
cleanup() {
  security delete-keychain "$keychain" >/dev/null 2>&1 || true
  rm -rf "$sign_dir"
}
trap cleanup EXIT
keychain_password="$(openssl rand -hex 32)"
printf '%s' "$APPLE_CERTIFICATE" | base64 --decode > "$sign_dir/cert.p12"
printf '%s' "$APPLE_API_KEY_P8" > "$sign_dir/AuthKey.p8"
chmod 600 "$sign_dir/cert.p12" "$sign_dir/AuthKey.p8"
security create-keychain -p "$keychain_password" "$keychain"
security set-keychain-settings -lut 1800 "$keychain"
security unlock-keychain -p "$keychain_password" "$keychain"
security import "$sign_dir/cert.p12" -k "$keychain" -P "$APPLE_CERTIFICATE_PASSWORD" -T /usr/bin/codesign >/dev/null
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$keychain_password" "$keychain" >/dev/null
app='target/cu-stage/payload/Grok Computer Use.app'
# Replace the identifier-only ad-hoc requirement with Apple's default Developer ID
# requirement (Apple anchor + certificate OU/Team ID + bundle identifier). No -r.
codesign --force --sign "$APPLE_SIGNING_IDENTITY" --keychain "$keychain" --options runtime --timestamp "$app" > "$sign_dir/codesign.log" 2>&1 || { echo 'Developer ID signing failed' >&2; exit 1; }
codesign --verify --deep --strict "$app"
# Verify the actual certificate Team ID without printing certificate details.
actual_team="$(codesign -dvv "$app" 2>&1 | sed -n 's/^TeamIdentifier=//p')"
[[ "$actual_team" == "$APPLE_TEAM_ID" ]] || { echo 'Developer ID team mismatch' >&2; exit 1; }
requirement="$(codesign -d -r- "$app" 2>&1)"
[[ "$requirement" == *'anchor apple generic'* && "$requirement" == *'certificate leaf'* ]] || { echo 'Developer ID designated requirement missing' >&2; exit 1; }
ditto -c -k --keepParent "$app" "$sign_dir/notarize.zip"
xcrun notarytool submit "$sign_dir/notarize.zip" --key "$sign_dir/AuthKey.p8" --key-id "$APPLE_API_KEY" --issuer "$APPLE_API_ISSUER" --wait --timeout 20m > "$sign_dir/notary.log" 2>&1 || { echo 'Notarization failed; no package will be published.' >&2; exit 1; }
xcrun stapler staple "$app"
xcrun stapler validate "$app"
# Same signed executable in both paths: audit-token helper/CLI identity must agree.
cp "$app/Contents/MacOS/gcu" target/cu-stage/payload/gcu
cmp "$app/Contents/MacOS/gcu" target/cu-stage/payload/gcu
