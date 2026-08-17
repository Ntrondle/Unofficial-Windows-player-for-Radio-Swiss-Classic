const STREAM_URL = 'https://stream.srg-ssr.ch/m/rsc_de/aacp_96'

const audio = new Audio()
audio.preload = 'none'

const btn = document.getElementById('play-stop')
const iconPlay = document.getElementById('icon-play')
const iconStop = document.getElementById('icon-stop')
const titleEl = document.getElementById('track-title')
const artistEl = document.getElementById('track-artist')
const volume = document.getElementById('volume')

let playing = false

const savedVolume = parseFloat(localStorage.getItem('rsc-volume'))
audio.volume = Number.isFinite(savedVolume) ? savedVolume : 0.8
volume.value = Math.round(audio.volume * 100)

function setPlaying(state) {
  playing = state
  iconPlay.classList.toggle('hidden', state)
  iconStop.classList.toggle('hidden', !state)
  btn.setAttribute('aria-label', state ? 'Stop' : 'Play')
}

btn.addEventListener('click', () => {
  if (playing) {
    audio.pause()
    audio.removeAttribute('src')
    audio.load()
    btn.classList.remove('buffering')
    setPlaying(false)
  } else {
    audio.src = STREAM_URL
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
})

audio.addEventListener('waiting', () => btn.classList.add('buffering'))
audio.addEventListener('playing', () => {
  btn.classList.remove('buffering')
  setPlaying(true)
})

volume.addEventListener('input', () => {
  audio.volume = volume.value / 100
  localStorage.setItem('rsc-volume', String(audio.volume))
})

window.api.onTrack((track) => {
  if (track.title) titleEl.textContent = track.title
  if (track.artist !== null) artistEl.textContent = track.artist || ''
})
