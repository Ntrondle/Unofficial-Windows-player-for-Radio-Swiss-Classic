//! Win32 UI, WinHTTP metadata and Media Foundation playback. No webview or codec bundle.
use crate::radio::{parse_track, request_body, Track, CHANNELS, ENDPOINT, MAX_METADATA_BYTES};
use ::windows::{
    core::{implement, w, Error, Result, PCWSTR},
    Win32::{
        Foundation::*,
        Globalization::GetUserDefaultUILanguage,
        Graphics::{Gdi::*, Imaging::*},
        Media::MediaFoundation::*,
        Networking::WinHttp::*,
        System::{
            Com::*,
            LibraryLoader::GetModuleHandleW,
            Registry::*,
            SystemServices::{SS_CENTER, SS_ENDELLIPSIS, SS_NOPREFIX},
        },
        UI::{Controls::*, HiDpi::*, WindowsAndMessaging::*},
    },
};
use std::cell::RefCell;
use std::ffi::c_void;
use std::mem::size_of;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc::{self, Receiver, SyncSender},
    Arc,
};
use std::time::{Duration, Instant};

const BG: COLORREF = COLORREF(0x0016110e);
const ACCENT: COLORREF = COLORREF(0x00db9e08);
const WHITE: COLORREF = COLORREF(0x00f7f6f5);
const DIM: COLORREF = COLORREF(0x00d3cdc9);
const MUTED: COLORREF = COLORREF(0x00a6a09a);
const PREFS: PCWSTR = w!("Software\\RadioSwissClassicPlayer");
const PLAY: usize = 10;
const LANGUAGE: usize = 20;
const VOLUME: usize = 30;
// TBM_GETPOS is the trackbar's WM_USER message (not exposed by windows-rs).
const TBM_GETPOS: u32 = WM_USER;

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
fn px(value: f64, dpi: u32) -> i32 {
    (value * dpi as f64 / 96.).round() as i32
}

struct Http(*mut c_void);
impl Http {
    fn checked(handle: *mut c_void) -> Result<Self> {
        if handle.is_null() {
            Err(Error::from_thread())
        } else {
            Ok(Self(handle))
        }
    }
}
impl Drop for Http {
    fn drop(&mut self) {
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}

unsafe fn fetch_track(session: &Http, language: usize) -> std::result::Result<Track, String> {
    let request = request_body(language)?;
    let (host, path) = ENDPOINT
        .strip_prefix("https://")
        .and_then(|s| s.split_once('/'))
        .ok_or("Invalid metadata endpoint")?;
    let host = wide(host);
    let path = wide(&format!("/{path}"));
    let load = || -> Result<Vec<u8>> {
        let connection = Http::checked(WinHttpConnect(session.0, PCWSTR(host.as_ptr()), 443, 0))?;
        let handle = Http::checked(WinHttpOpenRequest(
            connection.0,
            w!("POST"),
            PCWSTR(path.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        ))?;
        let headers = wide("Content-Type: application/json\r\n");
        WinHttpSendRequest(
            handle.0,
            Some(&headers[..headers.len() - 1]),
            Some(request.as_ptr().cast()),
            request.len() as u32,
            request.len() as u32,
            0,
        )?;
        WinHttpReceiveResponse(handle.0, std::ptr::null_mut())?;
        let mut status = 0u32;
        let mut length = size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            handle.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some((&mut status as *mut u32).cast()),
            &mut length,
            std::ptr::null_mut(),
        )?;
        if status != 200 {
            return Err(Error::new(E_FAIL, format!("HTTP {status}")));
        }
        let mut body = Vec::with_capacity(1024);
        loop {
            let mut buffer = [0u8; 4096];
            let mut count = 0u32;
            WinHttpReadData(
                handle.0,
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                &mut count,
            )?;
            if count == 0 {
                return Ok(body);
            }
            if body.len() + count as usize > MAX_METADATA_BYTES {
                return Err(Error::new(E_FAIL, "Metadata response too large"));
            }
            body.extend_from_slice(&buffer[..count as usize]);
        }
    };
    parse_track(&load().map_err(|e| e.to_string())?)
}

struct Update {
    language: usize,
    result: std::result::Result<Track, String>,
}
fn metadata_worker(initial: usize) -> (SyncSender<usize>, Receiver<Update>) {
    let (commands, receiver) = mpsc::sync_channel(1);
    let (sender, updates) = mpsc::sync_channel(1);
    std::thread::spawn(move || unsafe {
        let session = Http::checked(WinHttpOpen(
            w!("RadioSwissClassic/2.0"),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        ));
        let mut language = initial;
        loop {
            let result = match &session {
                Ok(session) => {
                    let _ = WinHttpSetTimeouts(session.0, 10000, 10000, 10000, 10000);
                    fetch_track(session, language)
                }
                Err(error) => Err(error.to_string()),
            };
            let _ = sender.try_send(Update { language, result });
            match receiver.recv_timeout(Duration::from_secs(20)) {
                Ok(next) => language = next,
                Err(mpsc::RecvTimeoutError::Timeout) => (),
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            while let Ok(next) = receiver.try_recv() {
                language = next;
            }
        }
    });
    (commands, updates)
}

#[derive(Debug)]
enum Playback {
    Playing,
    Buffering,
    Failed(String),
}
struct PlaybackUpdate {
    generation: usize,
    state: Playback,
}

#[implement(IMFPMediaPlayerCallback)]
struct Callback {
    generation: usize,
    active: Arc<AtomicUsize>,
    events: SyncSender<PlaybackUpdate>,
}
impl IMFPMediaPlayerCallback_Impl for Callback_Impl {
    fn OnMediaPlayerEvent(&self, header: *const MFP_EVENT_HEADER) {
        // MF owns the event and its COM references for the duration of this callback.
        unsafe {
            if header.is_null() || self.active.load(Ordering::Acquire) != self.generation {
                return;
            }
            let header_ref = &*header;
            let result = if header_ref.hrEvent.is_err() {
                Err(Error::from_hresult(header_ref.hrEvent))
            } else {
                match header_ref.eEventType {
                    MFP_EVENT_TYPE_MEDIAITEM_CREATED => {
                        let event = &*header.cast::<MFP_MEDIAITEM_CREATED_EVENT>();
                        match (header_ref.pMediaPlayer.as_ref(), event.pMediaItem.as_ref()) {
                            (Some(player), Some(item)) => player.SetMediaItem(item),
                            _ => Err(Error::new(E_FAIL, "Media item unavailable")),
                        }
                    }
                    MFP_EVENT_TYPE_MEDIAITEM_SET => header_ref
                        .pMediaPlayer
                        .as_ref()
                        .map(|p| p.Play())
                        .unwrap_or(Ok(())),
                    MFP_EVENT_TYPE_PLAY => {
                        self.send(Playback::Playing);
                        Ok(())
                    }
                    MFP_EVENT_TYPE_PLAYBACK_ENDED => {
                        Err(Error::new(E_FAIL, "The live stream ended"))
                    }
                    MFP_EVENT_TYPE_MF => {
                        let event = &*header.cast::<MFP_MF_EVENT>();
                        if event.MFEventType == MEBufferingStarted.0 as u32 {
                            self.send(Playback::Buffering);
                        }
                        if event.MFEventType == MEBufferingStopped.0 as u32 {
                            self.send(Playback::Playing);
                        }
                        Ok(())
                    }
                    _ => Ok(()),
                }
            };
            if let Err(error) = result {
                self.send(Playback::Failed(error.to_string()));
            }
        }
    }
}
impl Callback_Impl {
    fn send(&self, state: Playback) {
        let _ = self.events.try_send(PlaybackUpdate {
            generation: self.generation,
            state,
        });
    }
}

struct Font(HFONT);
impl Drop for Font {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.0.into());
        }
    }
}
unsafe fn font(points: f64, weight: i32, dpi: u32) -> Font {
    Font(CreateFontW(
        -(points * dpi as f64 / 72.).round() as i32,
        0,
        0,
        0,
        weight,
        0,
        0,
        0,
        DEFAULT_CHARSET,
        OUT_DEFAULT_PRECIS,
        CLIP_DEFAULT_PRECIS,
        CLEARTYPE_QUALITY,
        (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
        w!("Segoe UI"),
    ))
}
struct Logo {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
}
unsafe fn logo() -> Result<Logo> {
    let factory: IWICImagingFactory =
        CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
    let stream = factory.CreateStream()?;
    stream.InitializeFromMemory(include_bytes!("../assets/logo.png"))?;
    let decoder =
        factory.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnLoad)?;
    let frame = decoder.GetFrame(0)?;
    let converter = factory.CreateFormatConverter()?;
    converter.Initialize(
        &frame,
        &GUID_WICPixelFormat32bppBGRA,
        WICBitmapDitherTypeNone,
        None,
        0.,
        WICBitmapPaletteTypeCustom,
    )?;
    let (mut width, mut height) = (0, 0);
    converter.GetSize(&mut width, &mut height)?;
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    converter.CopyPixels(std::ptr::null(), width * 4, &mut pixels)?;
    for pixel in pixels.as_chunks_mut::<4>().0 {
        let alpha = pixel[3] as u32;
        for (channel, background) in [22u32, 17, 14].into_iter().enumerate() {
            pixel[channel] =
                ((pixel[channel] as u32 * alpha + background * (255 - alpha)) / 255) as u8;
        }
        pixel[3] = 255;
    }
    Ok(Logo {
        pixels,
        width,
        height,
    })
}

unsafe fn pref(name: PCWSTR) -> Option<u32> {
    let mut value = 0u32;
    let mut bytes = 4;
    RegGetValueW(
        HKEY_CURRENT_USER,
        PREFS,
        name,
        RRF_RT_REG_DWORD,
        None,
        Some((&mut value as *mut u32).cast()),
        Some(&mut bytes),
    )
    .is_ok()
    .then_some(value)
}
unsafe fn save_pref(name: PCWSTR, value: u32) {
    let mut key = HKEY::default();
    if RegCreateKeyW(HKEY_CURRENT_USER, PREFS, &mut key).is_ok() {
        let _ = RegSetKeyValueW(
            key,
            PCWSTR::null(),
            name,
            REG_DWORD.0,
            Some((&value as *const u32).cast()),
            4,
        );
        let _ = RegCloseKey(key);
    }
}
unsafe fn text(hwnd: HWND, value: &str) {
    let _ = SetWindowTextW(hwnd, PCWSTR(wide(value).as_ptr()));
}

struct State {
    hwnd: HWND,
    dpi: u32,
    background: HBRUSH,
    fonts: Vec<Font>,
    logo: Logo,
    heading: HWND,
    title: HWND,
    artist: HWND,
    status: HWND,
    buttons: [HWND; 3],
    play: HWND,
    slider: HWND,
    language: usize,
    volume: u32,
    requested: bool,
    fallback: bool,
    player: Option<IMFPMediaPlayer>,
    active: Arc<AtomicUsize>,
    events: Receiver<PlaybackUpdate>,
    event_tx: SyncSender<PlaybackUpdate>,
    commands: SyncSender<usize>,
    updates: Receiver<Update>,
    pending_language: Option<usize>,
    smoke: Option<Instant>,
    self_test: bool,
    stage: usize,
    metadata_ok: [bool; 3],
    playback_ok: [bool; 3],
    failures: Vec<String>,
}
impl State {
    unsafe fn stop(&mut self) {
        self.active.fetch_add(1, Ordering::AcqRel);
        if let Some(player) = self.player.take() {
            let _ = player.Shutdown();
        }
        self.requested = false;
        text(self.play, "Play");
        text(self.status, "");
        let _ = InvalidateRect(Some(self.play), None, false);
    }
    unsafe fn start(&mut self, fallback: bool) {
        self.stop();
        self.fallback = fallback;
        let generation = self.active.fetch_add(1, Ordering::AcqRel) + 1;
        let callback: IMFPMediaPlayerCallback = Callback {
            generation,
            active: self.active.clone(),
            events: self.event_tx.clone(),
        }
        .into();
        let result = (|| -> Result<()> {
            let mut player = None;
            MFPCreateMediaPlayer(
                PCWSTR::null(),
                false,
                MFP_OPTION_FREE_THREADED_CALLBACK,
                &callback,
                None,
                Some(&mut player),
            )?;
            let player = player.ok_or_else(|| Error::new(E_FAIL, "Media player unavailable"))?;
            player.SetVolume(if self.smoke.is_some() {
                0.
            } else {
                self.volume as f32 / 1000.
            })?;
            let format = if fallback { "mp3/128" } else { "aac/96" };
            let url = wide(&format!(
                "https://stream.srg-ssr.ch/srgssr/rsc_{}/{format}",
                CHANNELS[self.language].0
            ));
            self.player = Some(player);
            self.player.as_ref().unwrap().CreateMediaItemFromURL(
                PCWSTR(url.as_ptr()),
                false,
                generation,
                None,
            )?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.requested = true;
                text(self.play, "Stop");
                text(self.status, "Connecting…");
            }
            Err(error) => {
                self.stop();
                self.failures.push(error.to_string());
                text(
                    self.status,
                    "Audio unavailable · check Windows Media features",
                );
            }
        }
        let _ = InvalidateRect(Some(self.play), None, false);
    }
    unsafe fn change_language(&mut self, language: usize) {
        if language >= 3 || language == self.language {
            return;
        }
        self.language = language;
        if self.smoke.is_none() {
            save_pref(w!("language"), language as u32);
        }
        self.pending_language = Some(language);
        text(self.title, "…");
        text(self.artist, "");
        for button in self.buttons {
            let _ = InvalidateRect(Some(button), None, false);
        }
        if self.requested {
            self.start(false);
        }
    }
    unsafe fn layout(&mut self) {
        let dpi = self.dpi;
        self.fonts = vec![
            font(8.25, 400, dpi),
            font(11.25, 600, dpi),
            font(9.75, 400, dpi),
            font(7.5, 400, dpi),
            font(7.5, 600, dpi),
        ];
        for (hwnd, index) in [
            (self.heading, 0),
            (self.title, 1),
            (self.artist, 2),
            (self.status, 3),
            (self.buttons[0], 4),
            (self.buttons[1], 4),
            (self.buttons[2], 4),
        ] {
            SendMessageW(
                hwnd,
                WM_SETFONT,
                Some(WPARAM(self.fonts[index].0 .0 as usize)),
                Some(LPARAM(1)),
            );
        }
        for (hwnd, x, y, w, h) in [
            (self.heading, 18., 194., 264., 16.),
            (self.title, 18., 218., 264., 40.),
            (self.artist, 18., 263., 264., 18.),
            (self.status, 12., 320.5, 276., 14.),
            (self.play, 123., 341.5, 54., 54.),
            (self.slider, 38., 412., 224., 24.),
            (self.buttons[0], 93., 293., 34., 22.),
            (self.buttons[1], 133., 293., 34., 22.),
            (self.buttons[2], 173., 293., 34., 22.),
        ] {
            let _ = MoveWindow(hwnd, px(x, dpi), px(y, dpi), px(w, dpi), px(h, dpi), true);
        }
        self.layout_track();
        let _ = InvalidateRect(Some(self.hwnd), None, true);
    }
    unsafe fn layout_track(&self) {
        let dc = GetDC(Some(self.hwnd));
        let old = SelectObject(dc, self.fonts[1].0.into());
        let length = GetWindowTextLengthW(self.title) as usize;
        let mut value = vec![0u16; length + 1];
        GetWindowTextW(self.title, &mut value);
        let mut bounds = RECT {
            right: px(264., self.dpi),
            ..Default::default()
        };
        DrawTextW(
            dc,
            &mut value[..length],
            &mut bounds,
            DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX,
        );
        let height = bounds.bottom.clamp(px(20., self.dpi), px(42., self.dpi));
        let _ = SelectObject(dc, old);
        ReleaseDC(Some(self.hwnd), dc);
        let _ = MoveWindow(
            self.title,
            px(18., self.dpi),
            px(218., self.dpi),
            px(264., self.dpi),
            height,
            true,
        );
        let _ = MoveWindow(
            self.artist,
            px(18., self.dpi),
            px(218., self.dpi) + height + px(4., self.dpi),
            px(264., self.dpi),
            px(18., self.dpi),
            true,
        );
    }
    unsafe fn tick(&mut self) {
        if let Some(language) = self.pending_language {
            if self.commands.try_send(language).is_ok() {
                self.pending_language = None;
            }
        }
        while let Ok(update) = self.updates.try_recv() {
            if update.language != self.language {
                continue;
            }
            match update.result {
                Ok(track) => {
                    text(
                        self.title,
                        track
                            .title
                            .as_deref()
                            .filter(|s| !s.is_empty())
                            .unwrap_or("Radio Swiss Classic"),
                    );
                    text(self.artist, track.artist.as_deref().unwrap_or(""));
                    self.metadata_ok[self.language] = true;
                }
                Err(error) => {
                    text(self.artist, "Track information unavailable · retrying");
                    if self.smoke.is_some() {
                        self.failures.push(error);
                    }
                }
            }
            self.layout_track();
        }
        while let Ok(event) = self.events.try_recv() {
            if event.generation != self.active.load(Ordering::Acquire) || !self.requested {
                continue;
            }
            match event.state {
                Playback::Playing => {
                    text(self.status, "");
                    self.playback_ok[self.language] = true;
                }
                Playback::Buffering => text(self.status, "Buffering…"),
                Playback::Failed(error) => {
                    self.failures.push(error);
                    if !self.fallback {
                        self.start(true);
                    } else {
                        self.stop();
                        text(self.status, "Stream unavailable · press Play to retry");
                    }
                }
            }
        }
        if let Some(start) = self.smoke {
            let elapsed = start.elapsed().as_secs();
            if self.stage == 0 && elapsed >= 10 {
                self.stage = 1;
                self.change_language(1);
                if !self.self_test && !self.requested {
                    self.start(false);
                }
            } else if self.stage == 1 && elapsed >= 20 {
                self.stage = 2;
                self.change_language(2);
                if !self.self_test && !self.requested {
                    self.start(false);
                }
            } else if elapsed >= 32 {
                self.stop();
                let success = self.metadata_ok.into_iter().all(|ok| ok)
                    && (self.self_test || self.playback_ok.into_iter().all(|ok| ok));
                let report = serde_json::json!({"success":success,"metadata":self.metadata_ok,"playback":self.playback_ok,"mode":if self.self_test {"ui-and-metadata"} else {"playback"},"diagnostics":self.failures});
                let _ = std::fs::write(
                    "windows-smoke-report.json",
                    serde_json::to_vec_pretty(&report).unwrap(),
                );
                let _ = DestroyWindow(self.hwnd);
                PostQuitMessage(if success { 0 } else { 1 });
            }
        }
    }
}
impl Drop for State {
    fn drop(&mut self) {
        unsafe {
            self.stop();
            let _ = DeleteObject(self.background.into());
        }
    }
}

unsafe fn child(
    parent: HWND,
    class: PCWSTR,
    label: &str,
    style: WINDOW_STYLE,
    id: usize,
) -> Result<HWND> {
    CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        class,
        PCWSTR(wide(label).as_ptr()),
        WS_CHILD | WS_VISIBLE | style,
        0,
        0,
        0,
        0,
        Some(parent),
        Some(HMENU(id as *mut c_void)),
        Some(GetModuleHandleW(None)?.into()),
        None,
    )
}
unsafe fn paint_button(state: &State, item: &DRAWITEMSTRUCT) {
    let dc = item.hDC;
    let bounds = item.rcItem;
    FillRect(dc, &bounds, state.background);
    let is_play = item.CtlID as usize == PLAY;
    let active = is_play || item.CtlID as usize == LANGUAGE + state.language;
    let fill = CreateSolidBrush(if active { ACCENT } else { BG });
    let outline = CreatePen(
        PS_SOLID,
        px(1., state.dpi),
        if active { ACCENT } else { COLORREF(0x0040352e) },
    );
    let old_brush = SelectObject(dc, fill.into());
    let old_pen = SelectObject(dc, outline.into());
    let radius = if is_play {
        px(54., state.dpi)
    } else {
        px(22., state.dpi)
    };
    let _ = RoundRect(
        dc,
        bounds.left,
        bounds.top,
        bounds.right,
        bounds.bottom,
        radius,
        radius,
    );
    if is_play {
        let white = CreateSolidBrush(WHITE);
        let old = SelectObject(dc, white.into());
        let pen = CreatePen(PS_SOLID, 1, WHITE);
        let previous = SelectObject(dc, pen.into());
        let cx = (bounds.left + bounds.right) / 2;
        let cy = (bounds.top + bounds.bottom) / 2;
        if state.requested {
            let s = px(7., state.dpi);
            let _ = RoundRect(
                dc,
                cx - s,
                cy - s,
                cx + s,
                cy + s,
                px(2., state.dpi),
                px(2., state.dpi),
            );
        } else {
            let _ = Polygon(
                dc,
                &[
                    POINT {
                        x: cx - px(5., state.dpi),
                        y: cy - px(9., state.dpi),
                    },
                    POINT {
                        x: cx + px(10., state.dpi),
                        y: cy,
                    },
                    POINT {
                        x: cx - px(5., state.dpi),
                        y: cy + px(9., state.dpi),
                    },
                ],
            );
        }
        SelectObject(dc, previous);
        SelectObject(dc, old);
        let _ = DeleteObject(pen.into());
        let _ = DeleteObject(white.into());
    } else {
        let old = SelectObject(dc, state.fonts[4].0.into());
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, if active { WHITE } else { MUTED });
        let mut value = wide(&CHANNELS[item.CtlID as usize - LANGUAGE].0.to_uppercase());
        value.pop();
        let mut rect = bounds;
        DrawTextW(
            dc,
            &mut value,
            &mut rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
        SelectObject(dc, old);
    }
    if item.itemState.0 & ODS_FOCUS.0 != 0 {
        let _ = DrawFocusRect(
            dc,
            &RECT {
                left: bounds.left + 3,
                top: bounds.top + 3,
                right: bounds.right - 3,
                bottom: bounds.bottom - 3,
            },
        );
    }
    SelectObject(dc, old_pen);
    SelectObject(dc, old_brush);
    let _ = DeleteObject(outline.into());
    let _ = DeleteObject(fill.into());
}

unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if message == WM_DESTROY {
        let _ = KillTimer(Some(hwnd), 1);
        PostQuitMessage(0);
        return LRESULT(0);
    }
    let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const RefCell<Box<State>>;
    if pointer.is_null() {
        return DefWindowProcW(hwnd, message, wp, lp);
    }
    // Synchronous child-control messages can reenter this procedure.
    let Ok(mut state) = (&*pointer).try_borrow_mut() else {
        return DefWindowProcW(hwnd, message, wp, lp);
    };
    match message {
        WM_COMMAND => {
            if ((wp.0 >> 16) & 0xffff) != BN_CLICKED as usize {
                return LRESULT(0);
            }
            let id = wp.0 & 0xffff;
            if id == PLAY {
                if state.requested {
                    state.stop();
                } else {
                    state.start(false);
                }
            } else if (LANGUAGE..LANGUAGE + 3).contains(&id) {
                state.change_language(id - LANGUAGE);
            }
            LRESULT(0)
        }
        WM_HSCROLL => {
            state.volume = SendMessageW(state.slider, TBM_GETPOS, None, None)
                .0
                .clamp(0, 1000) as u32;
            if let Some(player) = &state.player {
                let _ = player.SetVolume(state.volume as f32 / 1000.);
            }
            if state.smoke.is_none() {
                save_pref(w!("volume"), state.volume);
            }
            LRESULT(0)
        }
        WM_DRAWITEM => {
            paint_button(&state, &*(lp.0 as *const DRAWITEMSTRUCT));
            LRESULT(1)
        }
        WM_CTLCOLORSTATIC => {
            let dc = HDC(wp.0 as *mut c_void);
            let control = HWND(lp.0 as *mut c_void);
            SetBkColor(dc, BG);
            SetTextColor(
                dc,
                if control == state.heading {
                    ACCENT
                } else if control == state.artist {
                    DIM
                } else if control == state.status {
                    MUTED
                } else {
                    WHITE
                },
            );
            LRESULT(state.background.0 as isize)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let mut paint = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut paint);
            FillRect(dc, &paint.rcPaint, state.background);
            let logo = &state.logo;
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: logo.width as i32,
                    biHeight: -(logo.height as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            SetStretchBltMode(dc, HALFTONE);
            StretchDIBits(
                dc,
                px(91., state.dpi),
                px(26., state.dpi),
                px(118., state.dpi),
                px(136., state.dpi),
                0,
                0,
                logo.width as i32,
                logo.height as i32,
                Some(logo.pixels.as_ptr().cast()),
                &info,
                DIB_RGB_COLORS,
                SRCCOPY,
            );
            let _ = EndPaint(hwnd, &paint);
            LRESULT(0)
        }
        WM_DPICHANGED => {
            state.dpi = (wp.0 & 0xffff) as u32;
            let bounds = &*(lp.0 as *const RECT);
            let _ = SetWindowPos(
                hwnd,
                None,
                bounds.left,
                bounds.top,
                bounds.right - bounds.left,
                bounds.bottom - bounds.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            state.layout();
            LRESULT(0)
        }
        WM_TIMER => {
            state.tick();
            LRESULT(0)
        }
        WM_CLOSE => {
            state.stop();
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            let _ = KillTimer(Some(hwnd), 1);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, message, wp, lp),
    }
}

unsafe fn app() -> Result<i32> {
    CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
    // Explicitly initialize network-capable Media Foundation for audio playback.
    let media = MFStartup(MF_VERSION, MFSTARTUP_FULL);
    let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    let controls = INITCOMMONCONTROLSEX {
        dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_BAR_CLASSES,
    };
    InitCommonControlsEx(&controls).ok()?;
    let instance = GetModuleHandleW(None)?;
    let background = CreateSolidBrush(BG);
    let class = w!("RadioSwissClassicNative");
    let icon = LoadIconW(
        Some(instance.into()),
        PCWSTR(std::ptr::without_provenance(1)),
    )
    .or_else(|_| LoadIconW(None, IDI_APPLICATION))?;
    let window_class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: instance.into(),
        lpszClassName: class,
        hbrBackground: background,
        hIcon: icon,
        hCursor: LoadCursorW(None, IDC_ARROW)?,
        ..Default::default()
    };
    if RegisterClassW(&window_class) == 0 {
        return Err(Error::from_thread());
    }
    let dpi = GetDpiForSystem();
    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
    let mut bounds = RECT {
        right: px(300., dpi),
        bottom: px(450., dpi),
        ..Default::default()
    };
    AdjustWindowRectExForDpi(&mut bounds, style, false, WINDOW_EX_STYLE::default(), dpi)?;
    let hwnd = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        class,
        w!("Radio Swiss Classic"),
        style,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        bounds.right - bounds.left,
        bounds.bottom - bounds.top,
        None,
        None,
        Some(instance.into()),
        None,
    )?;
    let language = pref(w!("language")).filter(|v| *v < 3).unwrap_or_else(|| {
        match GetUserDefaultUILanguage() & 0x3ff {
            0x0c => 1,
            0x10 => 2,
            _ => 0,
        }
    }) as usize;
    let volume = pref(w!("volume")).unwrap_or(800).min(1000);
    let heading = child(
        hwnd,
        w!("STATIC"),
        "C U R R E N T L Y  P L A Y I N G",
        WINDOW_STYLE(SS_CENTER.0 | SS_NOPREFIX.0),
        100,
    )?;
    let title = child(
        hwnd,
        w!("STATIC"),
        "…",
        WINDOW_STYLE(SS_CENTER.0 | SS_NOPREFIX.0),
        101,
    )?;
    let artist = child(
        hwnd,
        w!("STATIC"),
        "",
        WINDOW_STYLE(SS_CENTER.0 | SS_NOPREFIX.0 | SS_ENDELLIPSIS.0),
        102,
    )?;
    let status = child(
        hwnd,
        w!("STATIC"),
        "",
        WINDOW_STYLE(SS_CENTER.0 | SS_NOPREFIX.0 | SS_ENDELLIPSIS.0),
        103,
    )?;
    let buttons = [
        child(
            hwnd,
            w!("BUTTON"),
            "DE",
            WINDOW_STYLE(BS_OWNERDRAW as u32) | WS_TABSTOP,
            LANGUAGE,
        )?,
        child(
            hwnd,
            w!("BUTTON"),
            "FR",
            WINDOW_STYLE(BS_OWNERDRAW as u32) | WS_TABSTOP,
            LANGUAGE + 1,
        )?,
        child(
            hwnd,
            w!("BUTTON"),
            "IT",
            WINDOW_STYLE(BS_OWNERDRAW as u32) | WS_TABSTOP,
            LANGUAGE + 2,
        )?,
    ];
    let play = child(
        hwnd,
        w!("BUTTON"),
        "Play",
        WINDOW_STYLE(BS_OWNERDRAW as u32) | WS_TABSTOP,
        PLAY,
    )?;
    let slider = child(
        hwnd,
        TRACKBAR_CLASSW,
        "Volume",
        WINDOW_STYLE(TBS_HORZ | TBS_NOTICKS) | WS_TABSTOP,
        VOLUME,
    )?;
    SendMessageW(
        slider,
        TBM_SETRANGE,
        Some(WPARAM(1)),
        Some(LPARAM(1000isize << 16)),
    );
    SendMessageW(
        slider,
        TBM_SETPOS,
        Some(WPARAM(1)),
        Some(LPARAM(volume as isize)),
    );
    let (commands, updates) = metadata_worker(language);
    let (event_tx, events) = mpsc::sync_channel(8);
    let self_test = std::env::args().any(|s| s == "--self-test");
    let smoke = self_test || std::env::args().any(|s| s == "--smoke-test");
    let mut state = Box::new(State {
        hwnd,
        dpi,
        background,
        fonts: Vec::new(),
        logo: logo()?,
        heading,
        title,
        artist,
        status,
        buttons,
        play,
        slider,
        language,
        volume,
        requested: false,
        fallback: false,
        player: None,
        active: Arc::new(AtomicUsize::new(0)),
        events,
        event_tx,
        commands,
        updates,
        pending_language: None,
        smoke: smoke.then(Instant::now),
        self_test,
        stage: 0,
        metadata_ok: [false; 3],
        playback_ok: [false; 3],
        failures: Vec::new(),
    });
    state.layout();
    if let Err(error) = media {
        state.failures.push(error.to_string());
        text(status, "Windows Media Feature Pack may be required");
    }
    if smoke {
        state.change_language(0);
        if !self_test {
            state.start(false);
        }
    }
    let state = RefCell::new(state);
    SetWindowLongPtrW(
        hwnd,
        GWLP_USERDATA,
        (&state as *const RefCell<Box<State>>) as isize,
    );
    SetTimer(Some(hwnd), 1, 500, None);
    let _ = ShowWindow(hwnd, SW_SHOW);
    let mut message = MSG::default();
    loop {
        let result = GetMessageW(&mut message, None, 0, 0).0;
        if result == 0 {
            break;
        }
        if result < 0 {
            return Err(Error::from_thread());
        }
        if !IsDialogMessageW(hwnd, &message).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
    drop(state);
    let _ = MFShutdown();
    CoUninitialize();
    Ok(message.wParam.0 as i32)
}
pub fn run() {
    let code = unsafe {
        match app() {
            Ok(code) => code,
            Err(error) => {
                if std::env::args().any(|arg| arg == "--self-test" || arg == "--smoke-test") {
                    let report =
                        serde_json::json!({"success":false,"startup_error":error.to_string()});
                    let _ = std::fs::write(
                        "windows-smoke-report.json",
                        serde_json::to_vec_pretty(&report).unwrap(),
                    );
                    std::process::exit(1);
                }
                let message = wide(&format!("Could not start Radio Swiss Classic:\n{error}"));
                MessageBoxW(
                    None,
                    PCWSTR(message.as_ptr()),
                    w!("Radio Swiss Classic"),
                    MB_OK | MB_ICONERROR,
                );
                1
            }
        }
    };
    std::process::exit(code);
}
