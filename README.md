# Radio Swiss Classic Player

A tiny desktop player for [Radio Swiss Classic](https://www.radioswissclassic.ch/en/) for Windows and macOS, built with Electron.

## Features

- Compact single-column window (~300 x 450): logo, currently playing title and artist, play/stop button, volume slider
- Language selector (DE / FR / IT): switches the stream host language and localizes track titles
- Live track metadata via the SRG SSR playlist API, refreshed every 20 seconds
- Play/stop streaming via the official SRG SSR stream (AAC 96 kbps)
- Volume level is remembered between sessions

## Install

### Windows

Download `RadioSwissClassicPlayer-Setup-x.y.z.exe` from the [releases page](../../releases) and run it, it installs per-user with no admin rights required.

### macOS

Download `RadioSwissClassicPlayer-x.y.z-universal.dmg` from the [releases page](../../releases) (works on both Intel and Apple Silicon Macs), open it and drag the app to `Applications`.

The app is not code-signed with a developer certificate, so the first launch requires right-clicking the app and choosing **Open** (or `xattr -dr com.apple.quarantine /Applications/Radio\ Swiss\ Classic\ Player.app` after moving it out of the DMG).

## Development

```bash
npm install
npm start
```

## Build the installers

```bash
npm run build:win   # NSIS installer -> dist/
npm run build:mac   # universal DMG -> dist/
```

Pushing a `v*` tag also builds both installers on GitHub Actions and attaches them to a GitHub release.

## Project structure

```
main.js       Electron main process, window creation, SRG SSR playlist API polling
preload.js    Secure bridge exposing the track updates to the renderer
renderer.js   Stream playback, language selection and UI logic
index.html    App markup
style.css     App styling
assets/       Logo and app icon
```

## Credits

- Streams and track metadata: [SRG SSR](https://www.srgssr.ch/en) - Radio Swiss Classic
- This project is fan-made and not affiliated with SRG SSR. if you wish me to remove this repo please reach out in issues and i will promptly remove this, sorry !

## License

[GPL-3.0](LICENSE)
