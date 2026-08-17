const STREAMS = {
  de: 'https://stream.srg-ssr.ch/m/rsc_de/aacp_96',
  fr: 'https://stream.srg-ssr.ch/m/rsc_fr/aacp_96',
  it: 'https://stream.srg-ssr.ch/m/rsc_it/aacp_96'
}
const LANGS = Object.keys(STREAMS)

const audio = new Audio()
audio.preload = 'none'

const btn = document.getElementById('play-stop')
const iconPlay = document.getElementById('icon-play')
const iconStop = document.getElementById('icon-stop')
const titleEl = document.getElementById('track-title')
const artistEl = document.getElementById('track-artist')
const volume = document.getElementById('volume')
const langBtns = Array.from(document.querySelectorAll('.lang-btn'))

let playing = false
let lang = localStorage.getItem('rsc-lang')
if (!LANGS.includes(lang)) {
  const nav = (navigator.language || 'de').slice(0, 2).toLowerCase()
  lang = LANGS.includes(nav) ? nav : 'de'
}

const savedVolume = parseFloat(localStorage.getItem('rsc-volume'))
audio.volume = Number.isFinite(savedVolume) ? savedVolume : 0.8
volume.value = Math.round(audio.volume * 100)

function setPlaying(state) {
  playing = state
  iconPlay.classList.toggle('hidden', state)
  iconStop.classList.toggle('hidden', !state)
  btn.setAttribute('aria-label', state ? 'Stop' : 'Play')
}

function updateLangUI() {
  for (const b of langBtns) b.classList.toggle('active', b.dataset.lang === lang)
}

function startStream() {
  audio.src = STREAMS[lang]
  btn.classList.add('buffering')
  audio
    .play()
    .then(() => {
      btn.classList.remove('buffering')
      setPlaying(true)
    })
    .catch(() => {
      btn.classList.remove('buffering')
      setPlaying(false)
      titleEl.textContent = 'Stream unavailable'
      artistEl.textContent = ''
    })
}

function stopStream() {
  audio.pause()
  audio.removeAttribute('src')
  audio.load()
  btn.classList.remove('buffering')
  setPlaying(false)
}

btn.addEventListener('click', () => {
  if (playing) stopStream()
  else startStream()
})

for (const b of langBtns) {
  b.addEventListener('click', () => {
    if (b.dataset.lang === lang) return
    lang = b.dataset.lang
    localStorage.setItem('rsc-lang', lang)
    updateLangUI()
    window.api.setLang(lang)
    titleEl.textContent = '\u2026'
    artistEl.textContent = ''
    if (playing) {
      audio.pause()
      startStream()
    }
  })
}

audio.addEventListener('waiting', () => btn.classList.add('buffering'))
audio.addEventListener('playing', () => {
  btn.classList.remove('buffering')
  setPlaying(true)
})

volume.addEventListener('input', () => {
  audio.volume = volume.value / 100
  localStorage.setItem('rsc-volume', String(audio.volume))
})

updateLangUI()
window.api.setLang(lang)

window.api.onTrack((track) => {
  if (track.title) titleEl.textContent = track.title
  if (track.artist !== null) artistEl.textContent = track.artist || ''
})
