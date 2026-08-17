# Radio Swiss Classic Player

A tiny Windows desktop player for [Radio Swiss Classic](https://www.radioswissclassic.ch/en/), built with Electron.

## Features

- Compact single-column window (~300 x 450): logo, currently playing title and artist, play/stop button, volume slider
- Live track metadata, refreshed every 20 seconds from the Radio Swiss Classic playlist
- Play/stop streaming via the official SRG SSR stream (AAC 96 kbps)
- Volume level is remembered between sessions

## Install

Download `RadioSwissClassicPlayer-Setup-x.y.z.exe` from the [releases page](../../releases) and run it, it installs per-user with no admin rights required.

## Development

```bash
npm install
npm start
```

## Build the installer

```bash
npm run build
```

The NSIS installer is written to `dist/`.

## Project structure

```
main.js       Electron main process, window creation, metadata polling
preload.js    Secure bridge exposing the track updates to the renderer
renderer.js   Stream playback and UI logic
track.js      Playlist parser, current-track selection (Europe/Zurich)
index.html    App markup
style.css     App styling
assets/       Logo and app icon
```

## Credits

- Streams and track metadata: [SRG SSR](https://www.srgssr.ch/en) - Radio Swiss Classic
- This project is fan-made and not affiliated with SRG SSR. if you wish me to remove this repo please reach out in issues and i will promptly remove this, sorry !

## License

[GPL-3.0](LICENSE)
