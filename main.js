const { app, BrowserWindow, net, ipcMain } = require('electron')
const path = require('path')

const GQL_ENDPOINT = 'https://ssatr.playlist-api.deliver.media/graphql'
const CHANNELS = {
  de: { id: '0191e9e4-ffc8-782b-8ace-6604e0d6f2dc', field: 'TitleDE' },
  fr: { id: '0191e9e5-213d-705e-b520-cee967358e6f', field: 'TitleFR' },
  it: { id: '0191e9e5-3db3-7deb-ae10-48c24845852b', field: 'TitleIT' }
}
const POLL_MS = 20000

app.disableHardwareAcceleration()
app.commandLine.appendSwitch('disable-features', 'AudioServiceOutOfProcess')
app.commandLine.appendSwitch('in-process-gpu')

let win = null
let lang = 'de'
let fetchGen = 0

async function fetchTrack() {
  const chan = CHANNELS[lang]
  if (!chan) return
  const gen = ++fetchGen
  const query = `query ($chan: String) { channel(id: $chan) { playingnow { current { metadata { artist title: ${chan.field} } } } } }`
  try {
    const res = await net.fetch(GQL_ENDPOINT, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ query, variables: { chan: chan.id } })
    })
    if (!res.ok || gen !== fetchGen) return
    const json = await res.json()
    const meta = json?.data?.channel?.playingnow?.current?.metadata
    if (meta && gen === fetchGen && win && !win.isDestroyed()) {
      win.webContents.send('track', { title: meta.title || '', artist: meta.artist || '' })
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
      sandbox: true,
      spellcheck: false,
      devTools: false,
      preload: path.join(__dirname, 'preload.js')
    }
  })
  win.loadFile('index.html')
}

ipcMain.on('set-lang', (_event, newLang) => {
  if (!CHANNELS[newLang] || newLang === lang) return
  lang = newLang
  fetchTrack()
})

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
