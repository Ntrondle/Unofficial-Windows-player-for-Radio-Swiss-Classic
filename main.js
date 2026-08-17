const { app, BrowserWindow, net } = require('electron')
const path = require('path')
const { parseCurrentTrack } = require('./track')

const META_URL = 'https://www.radioswissclassic.ch/en/'
const POLL_MS = 20000

let win = null

async function fetchTrack() {
  try {
    const res = await net.fetch(META_URL, {
      headers: { 'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)' }
    })
    if (!res.ok) return
    const html = await res.text()
    const track = parseCurrentTrack(html)
    if (track && win && !win.isDestroyed()) {
      win.webContents.send('track', track)
    }
  } catch (_) {
    // network hiccup, next poll will retry
  }
}

function createWindow() {
  win = new BrowserWindow({
    width: 300,
    height: 450,
    useContentSize: true,
    resizable: false,
    maximizable: false,
    fullscreenable: false,
    autoHideMenuBar: true,
    backgroundColor: '#0e1116',
    icon: path.join(__dirname, 'assets', 'logo.png'),
    webPreferences: {
      contextIsolation: true,
      nodeIntegration: false,
      preload: path.join(__dirname, 'preload.js')
    }
  })
  win.loadFile('index.html')
}

app.whenReady().then(() => {
  createWindow()
  fetchTrack()
  setInterval(fetchTrack, POLL_MS)

  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow()
  })
})

app.on('window-all-closed', () => {
  app.quit()
})
