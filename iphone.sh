#!/bin/sh
# PhysioPulse — build and open the staff iPhone app in Xcode (Mac only).
# Run from this folder:  sh iphone.sh
# Full instructions: claude/DEVELOPMENT.md › iPhone app.
set -e
cd "$(dirname "$0")"

say() { printf '\n▸ %s\n' "$1"; }

[ "$(uname)" = "Darwin" ] || { echo "This only works on a Mac."; exit 1; }
xcode-select -p >/dev/null 2>&1 && command -v xcodebuild >/dev/null || {
  echo "Install Xcode from the App Store, open it once, accept the licence, then run this again."; exit 1; }
command -v node >/dev/null || { echo "Install Node.js 20 or newer (https://nodejs.org), then run this again."; exit 1; }
if ! command -v cargo >/dev/null; then
  say "Installing Rust (rustup)…"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  . "$HOME/.cargo/env"
fi

say "Rust targets for the iPhone"
rustup target add aarch64-apple-ios aarch64-apple-ios-sim

say "Packages (npm install)"
npm install

if ! command -v pod >/dev/null; then
  say "Installing CocoaPods (Homebrew)"
  command -v brew >/dev/null || { echo "Install Homebrew first (https://brew.sh), then run this again."; exit 1; }
  brew install cocoapods
fi

# A half-made project (an earlier init that stopped) is removed and made again.
if ! ls src-tauri/gen/apple/*.xcodeproj >/dev/null 2>&1; then
  say "Creating the Xcode project (tauri ios init)"
  rm -rf src-tauri/gen/apple
  npx tauri ios init
fi

say "Starting the app server and opening Xcode (leave this Terminal window open)"
echo "In Xcode:"
echo "  1. Target PhysioPulse_iOS › Signing & Capabilities › Team = your Apple ID (Personal Team)."
echo "     If Xcode says the bundle id is taken, change it to e.g. app.physiopulse.filipe (only here)."
echo "  2. Check target › Info has Face ID and Microphone usage descriptions (from src-tauri/Info.ios.plist)."
echo "  3. Plug in the iPhone, choose it at the top, press ▶."
echo "  4. On the iPhone: Settings › General › VPN & Device Management › trust your Apple ID;"
echo "     turn on Developer Mode if asked (Settings › Privacy & Security), restart."
echo "  The iPhone and the Mac must be on the same Wi-Fi; on the iPhone allow 'Local Network' when asked."
echo "  Free account: press ▶ again every 7 days to keep it opening."
npx tauri ios dev --open --host
