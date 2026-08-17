const ZURICH_TZ = 'Europe/Zurich'

function decodeEntities(str) {
  return str
    .replace(/&quot;/g, '"')
    .replace(/&#0?39;/g, "'")
    .replace(/&apos;/g, "'")
    .replace(/&amp;/g, '&')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&nbsp;/g, ' ')
}

function parseRows(html) {
  const rows = []
  const chunks = html.split(/<div class="row"/).slice(1)
  for (const chunk of chunks) {
    const time = chunk.match(/class="date-sm"[^>]*>([^<]+)</)
    const title = chunk.match(/class="md-font"[^>]*>(?:<!---->)?([^<]+)</)
    const artist = chunk.match(/class="sm-font"[^>]*>([^<]+)</)
    if (!time || !title) continue
    const m = time[1].trim().match(/^(\d{1,2}):(\d{2})$/)
    if (!m) continue
    rows.push({
      minutes: parseInt(m[1], 10) * 60 + parseInt(m[2], 10),
      title: decodeEntities(title[1].trim()),
      artist: artist ? decodeEntities(artist[1].trim()) : ''
    })
  }
  return rows
}

function zurichMinutesNow() {
  const parts = new Intl.DateTimeFormat('en-GB', {
    timeZone: ZURICH_TZ,
    hour: '2-digit',
    minute: '2-digit',
    hour12: false
  }).format(new Date())
  const [h, m] = parts.split(':').map((n) => parseInt(n, 10))
  return ((h % 24) * 60 + m + 1440) % 1440
}

function parseCurrentTrack(html) {
  const rows = parseRows(html)
  if (rows.length === 0) return null

  const now = zurichMinutesNow()
  let current = null
  for (const row of rows) {
    if (row.minutes <= now) {
      if (!current || row.minutes >= current.minutes) current = row
    }
  }
  if (!current) current = rows[rows.length - 1]
  return { title: current.title, artist: current.artist }
}

module.exports = { parseCurrentTrack }
