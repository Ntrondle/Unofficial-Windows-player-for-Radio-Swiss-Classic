# Radio Swiss Classic Player

A compact, unofficial player for [Radio Swiss Classic](https://www.radioswissclassic.ch/en/).
macOS uses a native Rust app with AppKit controls and AVPlayer streaming. Windows
keeps the existing Electron player.

## Features

- Compact window with the station logo, current title and artist
- Play/stop, buffering and playback error status, volume slider
- DE / FR / IT streams and localized track titles
- Live metadata from SRG SSR, refreshed every 20 seconds
- Official SRG SSR AAC 96 kbps streams
- Language and volume remembered between sessions
- Original compact dark layout, centered track text, language pills and round play/stop control

## Native macOS app

The Rust version contains no Electron, Chromium, webview, JavaScript runtime, or
microphone/camera capture code. Playback uses Apple's AVPlayer. Its bundle has no
microphone/camera usage descriptions or capture entitlements. Listening to radio
does not require microphone permission.

Download `RadioSwissClassic-native-universal.zip` from the [releases page](../../releases),
unzip it and move **Radio Swiss Classic.app** to Applications. Universal release
builds support Apple Silicon and Intel Macs running macOS 11 or later. Local builds
default to the current Mac's architecture. Builds are ad-hoc signed, without Apple
notarization or a Developer ID certificate; downloaded builds may require approval
in macOS Privacy & Security.

The native app can run alongside the old **Radio Swiss Classic Player.app**.
It has its own preferences; the old Electron volume/language settings are not
imported. A new installation chooses DE/FR/IT from the system's preferred language,
falling back to DE. Closing the window quits the app and stops the stream; ⌘Q also
quits. Playback starts only when Play is pressed.

### Build locally

Install [Rust](https://rustup.rs/) and Apple's Xcode Command Line Tools, then:

```bash
cargo run                         # development
scripts/build-macos.sh             # release .app for this Mac in dist/
scripts/build-macos.sh --universal # release .app for Apple Silicon + Intel
open "dist/Radio Swiss Classic.app"
```

Validation:

```bash
cargo fmt --check
cargo clippy --locked -- -D warnings
cargo run -- --smoke-test
```

The smoke test opens a real native window and silently plays each of the three
streams, checks live metadata for each language, stops playback and exits after
26 seconds. It requires internet access and a logged-in macOS desktop. It returns
a nonzero status if any stream fails to reach AVPlayer's playing state, metadata
fails for any language, or stopping fails. Smoke-test language changes do not save
preferences. This verifies playback state; it does not establish audible output.

`src/macos.rs` owns the native UI and playback. Metadata runs on one background
worker with bounded queues, a response-size limit and a network timeout. Stale
results after a language change are discarded. There is no continuously rendered
web page. Memory use depends on macOS, playback state and shared system services;
measure the release build, not the compiler or a debug build.

The metadata client uses macOS's shared libcurl and native TLS support, rather
than bundling an async networking runtime and a separate TLS implementation.
Startup autoreleased objects are drained before the event loop starts; labels
are laid out only when metadata changes. AVPlayer uses a preferred five-second
forward buffer. The app still relies on macOS audio and windowing services.

A local 26-second playback test on Apple Silicon / macOS 15.7.1 sampled a physical
memory footprint of 23.5–25.6 MiB (25.8 MiB peak). The earlier Rust build sampled
23.8–24.9 MiB (25.0 MiB peak), so this does **not** demonstrate a meaningful physical
RAM reduction. Resident memory (RSS) is a different metric and includes shared
library pages; the previously quoted 60–80 MB was RSS. Neither measurement is the
sum of all system audio services. These are short-run observations, not a memory
limit or a guarantee. The universal app bundle is now approximately 1.6 MB on disk.

To inspect the currently running app on macOS, use `vmmap -summary <pid>` for its
physical footprint and `ps -o rss= -p <pid>` for resident memory in KiB. The final
universal bundle was compiled for Intel and Apple Silicon; runtime checks were
performed on Apple Silicon.

## Windows / legacy Electron player

Download `RadioSwissClassicPlayer-Setup-x.y.z.exe` from the [releases page](../../releases)
and run it. The per-user installer does not need admin rights.

```bash
npm ci
npm start
npm run build:win
npm run build:mac # legacy Electron macOS build, not the native Rust app
```

The Electron source explicitly denies permission requests and permission checks.
No microphone access is needed for HTML audio playback either. The previous macOS
Electron bundle included generic microphone and camera permission descriptions;
those descriptions alone do not prove recording or explain the exact trigger of
a permission prompt.

Tagging a `v*` release builds the Windows installer and native macOS universal ZIP
with GitHub Actions. The Electron sources remain in `main.js`, `preload.js`,
`renderer.js`, `index.html`, and `style.css`.

## Credits and license

Streams and metadata: [SRG SSR](https://www.srgssr.ch/en), Radio Swiss Classic.
This project is fan-made and is not affiliated with SRG SSR. Please reach out in
issues if you wish this repository to be removed.

[GPL-3.0](LICENSE).
