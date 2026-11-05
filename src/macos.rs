//! AppKit and AVPlayer stay on the main thread; one bounded worker polls metadata.
//! The native boundary uses objc2's retained objects and checked message signatures.
use objc2::rc::{autoreleasepool, Allocated, Retained};
use objc2::runtime::{AnyObject, Bool};
use objc2::{class, define_class, msg_send, sel, DefinedClass, MainThreadOnly};
use objc2_core_graphics::CGColor;
use objc2_foundation::{MainThreadMarker, NSObject, NSPoint, NSRect, NSSize, NSString};
use serde::Deserialize;
use std::cell::RefCell;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::{Duration, Instant};

#[link(name = "AppKit", kind = "framework")]
extern "C" {}
#[link(name = "AVFoundation", kind = "framework")]
extern "C" {}

type Obj = Retained<AnyObject>;
const ENDPOINT: &str = "https://ssatr.playlist-api.deliver.media/graphql";
const CHANNELS: [(&str, &str, &str); 3] = [
    ("de", "0191e9e4-ffc8-782b-8ace-6604e0d6f2dc", "TitleDE"),
    ("fr", "0191e9e5-213d-705e-b520-cee967358e6f", "TitleFR"),
    ("it", "0191e9e5-3db3-7deb-ae10-48c24845852b", "TitleIT"),
];

#[derive(Debug, Default, Deserialize)]
struct Track {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    artist: Option<String>,
}
struct Update {
    language: usize,
    result: Result<Track, String>,
}

fn fetch_track(client: &mut curl::easy::Easy, language: usize) -> Result<Track, String> {
    let (_, channel, field) = CHANNELS[language];
    let query = format!("query ($chan: String) {{ channel(id: $chan) {{ playingnow {{ current {{ metadata {{ artist title: {field} }} }} }} }} }}");
    let body =
        serde_json::to_vec(&serde_json::json!({"query": query, "variables": {"chan": channel}}))
            .map_err(|e| e.to_string())?;
    let mut response = Vec::with_capacity(1024);
    let mut perform = || -> Result<(), curl::Error> {
        client.url(ENDPOINT)?;
        client.post_fields_copy(&body)?;
        client.timeout(Duration::from_secs(10))?;
        client.fail_on_error(true)?;
        client.useragent("RadioSwissClassic/2.0")?;
        let mut headers = curl::easy::List::new();
        headers.append("Content-Type: application/json")?;
        client.http_headers(headers)?;
        let mut transfer = client.transfer();
        transfer.write_function(|data| {
            if response.len() + data.len() > 256 * 1024 {
                return Ok(0);
            }
            response.extend_from_slice(data);
            Ok(data.len())
        })?;
        transfer.perform()
    };
    perform().map_err(|e| e.to_string())?;
    let json: serde_json::Value = serde_json::from_slice(&response).map_err(|e| e.to_string())?;
    let metadata = json
        .pointer("/data/channel/playingnow/current/metadata")
        .filter(|value| value.is_object())
        .ok_or("Track information unavailable")?;
    serde_json::from_value(metadata.clone()).map_err(|e| e.to_string())
}

fn metadata_worker(initial: usize) -> (SyncSender<usize>, Receiver<Update>) {
    let (command_tx, command_rx) = mpsc::sync_channel(1);
    let (update_tx, update_rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        // Reuse macOS's shared libcurl/TLS instead of a private Tokio/rustls runtime.
        let mut client = curl::easy::Easy::new();
        let mut language = initial;
        loop {
            // Results and commands are bounded: no accumulated tasks during outages.
            let _ = update_tx.try_send(Update {
                language,
                result: fetch_track(&mut client, language),
            });
            match command_rx.recv_timeout(Duration::from_secs(20)) {
                Ok(next) => language = next,
                Err(mpsc::RecvTimeoutError::Timeout) => (),
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            while let Ok(next) = command_rx.try_recv() {
                language = next;
            }
        }
    });
    (command_tx, update_rx)
}

struct State {
    app: Obj,
    _window: Obj,
    player: Obj,
    button: Obj,
    languages: [Obj; 3],
    title: Obj,
    artist: Obj,
    status: Obj,
    defaults: Obj,
    language: usize,
    requested: bool,
    last_control: isize,
    pending_language: Option<usize>,
    commands: SyncSender<usize>,
    updates: Receiver<Update>,
    smoke: Option<Instant>,
    smoke_stage: u8,
    smoke_played: [bool; 3],
    smoke_metadata: [bool; 3],
}

impl State {
    unsafe fn stop(&mut self) {
        let _: () = msg_send![&*self.player, pause];
        let _: () = msg_send![&*self.player, replaceCurrentItemWithPlayerItem: std::ptr::null::<AnyObject>()];
        self.requested = false;
        set_playing(&self.button, false);
        set_text(&self.status, "");
    }

    unsafe fn start(&mut self) {
        self.stop();
        let stream = format!(
            "https://stream.srg-ssr.ch/srgssr/rsc_{}/aac/96",
            CHANNELS[self.language].0
        );
        let url: Obj = msg_send![class!(NSURL), URLWithString: &*NSString::from_str(&stream)];
        let item: Obj = msg_send![class!(AVPlayerItem), playerItemWithURL: &*url];
        // A live audio station needs a short forward buffer, not minutes of media.
        let _: () = msg_send![&*item, setPreferredForwardBufferDuration: 5f64];
        let _: () =
            msg_send![&*item, setCanUseNetworkResourcesForLiveStreamingWhilePaused: Bool::NO];
        let _: () = msg_send![&*self.player, replaceCurrentItemWithPlayerItem: &*item];
        let _: () = msg_send![&*self.player, play];
        self.requested = true;
        self.last_control = -1;
        set_playing(&self.button, true);
        set_text(&self.status, "Connecting…");
    }

    unsafe fn change_language(&mut self, next: usize) {
        if next >= CHANNELS.len() || next == self.language {
            return;
        }
        self.language = next;
        paint_languages(&self.languages, next);
        if self.smoke.is_none() {
            let _: () = msg_send![&*self.defaults, setInteger: next as isize, forKey: &*NSString::from_str("language")];
        }
        self.pending_language = Some(next);
        set_text(&self.title, "…");
        set_text(&self.artist, "");
        layout_track(&self.title, &self.artist);
        if self.requested {
            self.start();
        }
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
                    set_text(
                        &self.title,
                        track
                            .title
                            .as_deref()
                            .filter(|s| !s.is_empty())
                            .unwrap_or("Radio Swiss Classic"),
                    );
                    set_text(&self.artist, track.artist.as_deref().unwrap_or(""));
                    layout_track(&self.title, &self.artist);
                    if self.smoke.is_some() {
                        eprintln!("metadata {}: {:?}", CHANNELS[self.language].0, track);
                    }
                    self.smoke_metadata[self.language] = true;
                }
                Err(error) => {
                    set_text(&self.artist, "Track information unavailable · retrying");
                    layout_track(&self.title, &self.artist);
                    if self.smoke.is_some() {
                        eprintln!("metadata: {error}");
                    }
                }
            }
        }
        if self.requested {
            let item: Option<Obj> = msg_send![&*self.player, currentItem];
            if let Some(item) = item {
                let status: isize = msg_send![&*item, status];
                if status == 2 {
                    let error: Option<Obj> = msg_send![&*item, error];
                    if let Some(error) = error {
                        let description: Retained<NSString> =
                            msg_send![&*error, localizedDescription];
                        eprintln!("Playback failed: {description}");
                    }
                    self.stop();
                    set_text(&self.status, "Stream unavailable · press Play to retry");
                } else {
                    let control: isize = msg_send![&*self.player, timeControlStatus];
                    if control != self.last_control {
                        set_text(&self.status, if control == 2 { "" } else { "Buffering…" });
                        self.last_control = control;
                    }
                    if control == 2 {
                        self.smoke_played[self.language] = true;
                    }
                }
            }
        }
        if let Some(start) = self.smoke {
            let elapsed = start.elapsed().as_secs();
            if self.smoke_stage == 0 && elapsed >= 8 {
                self.smoke_stage = 1;
                self.change_language(1);
            } else if self.smoke_stage == 1 && elapsed >= 16 {
                self.smoke_stage = 2;
                self.change_language(2);
            } else if elapsed >= 26 {
                let played = self.smoke_played;
                let metadata = self.smoke_metadata;
                self.stop();
                let item: Option<Obj> = msg_send![&*self.player, currentItem];
                let stopped = item.is_none() && !self.requested;
                eprintln!("smoke: playback={played:?}, metadata={metadata:?}, languages=de/fr/it, stop={stopped}");
                std::process::exit(
                    if played.into_iter().all(|ok| ok)
                        && metadata.into_iter().all(|ok| ok)
                        && stopped
                    {
                        0
                    } else {
                        1
                    },
                );
            }
        }
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = RefCell<Option<State>>]
    struct Controller;

    impl Controller {
        #[unsafe(method(toggle:))]
        fn toggle(&self, _sender: &AnyObject) {
            if let Some(state) = self.ivars().borrow_mut().as_mut() {
                unsafe { if state.requested { state.stop(); } else { state.start(); } }
            }
        }
        #[unsafe(method(language:))]
        fn language(&self, sender: &AnyObject) {
            let next: isize = unsafe { msg_send![sender, tag] };
            if let Some(state) = self.ivars().borrow_mut().as_mut() {
                unsafe { state.change_language(next as usize); }
            }
        }
        #[unsafe(method(volume:))]
        fn volume(&self, sender: &AnyObject) {
            if let Some(state) = self.ivars().borrow_mut().as_mut() {
                unsafe {
                    let value: f64 = msg_send![sender, doubleValue];
                    let _: () = msg_send![&*state.player, setVolume: value as f32];
                    let _: () = msg_send![&*state.defaults, setDouble: value, forKey: &*NSString::from_str("volume")];
                }
            }
        }
        #[unsafe(method(tick:))]
        fn tick(&self, _timer: &AnyObject) {
            if let Some(state) = self.ivars().borrow_mut().as_mut() { unsafe { state.tick(); } }
        }
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _notification: &AnyObject) {
            if let Some(state) = self.ivars().borrow_mut().as_mut() {
                unsafe {
                    state.stop();
                    let _: () = msg_send![&*state.app, terminate: std::ptr::null::<AnyObject>()];
                }
            }
        }
    }
);

unsafe fn set_text(object: &AnyObject, text: &str) {
    let value = NSString::from_str(text);
    let _: () = msg_send![object, setStringValue: &*value];
    let _: () = msg_send![object, setToolTip: &*value];
}

unsafe fn color(red: f64, green: f64, blue: f64) -> Obj {
    msg_send![class!(NSColor), colorWithSRGBRed: red / 255., green: green / 255., blue: blue / 255., alpha: 1f64]
}

unsafe fn set_playing(button: &AnyObject, playing: bool) {
    // Two tiny vector shapes avoid loading the system symbol-font machinery.
    let image: Allocated<AnyObject> = msg_send![class!(NSImage), alloc];
    let image: Obj = msg_send![image, initWithSize: NSSize::new(26., 26.)];
    let _: () = msg_send![&*image, lockFocus];
    let white = color(255., 255., 255.);
    let _: () = msg_send![&*white, setFill];
    let path: Obj = if playing {
        msg_send![class!(NSBezierPath), bezierPathWithRoundedRect: rect(6., 6., 14., 14.), xRadius: 2f64, yRadius: 2f64]
    } else {
        let path: Obj = msg_send![class!(NSBezierPath), bezierPath];
        let _: () = msg_send![&*path, moveToPoint: NSPoint::new(8., 4.)];
        let _: () = msg_send![&*path, lineToPoint: NSPoint::new(23., 13.)];
        let _: () = msg_send![&*path, lineToPoint: NSPoint::new(8., 22.)];
        let _: () = msg_send![&*path, closePath];
        path
    };
    let _: () = msg_send![&*path, fill];
    let _: () = msg_send![&*image, unlockFocus];
    let _: () = msg_send![button, setImage: &*image];
    let _: () = msg_send![button, setImagePosition: 1usize];
    let description = NSString::from_str(if playing { "Stop" } else { "Play" });
    let _: () = msg_send![button, setAccessibilityLabel: &*description];
}

unsafe fn paint_languages(buttons: &[Obj; 3], selected: usize) {
    for (i, button) in buttons.iter().enumerate() {
        let active = i == selected;
        let layer: Obj = msg_send![&**button, layer];
        let background: Obj = if active {
            color(8., 158., 219.)
        } else {
            msg_send![class!(NSColor), clearColor]
        };
        let border = if active {
            color(8., 158., 219.)
        } else {
            color(46., 53., 64.)
        };
        let foreground = if active {
            color(255., 255., 255.)
        } else {
            color(154., 160., 166.)
        };
        let background: *const CGColor = msg_send![&*background, CGColor];
        let border: *const CGColor = msg_send![&*border, CGColor];
        let _: () = msg_send![&*layer, setBackgroundColor: background];
        let _: () = msg_send![&*layer, setBorderColor: border];
        let _: () = msg_send![&**button, setContentTintColor: &*foreground];
        let _: () = msg_send![&**button, setState: if active { 1isize } else { 0isize }];
    }
}

unsafe fn layout_track(title: &AnyObject, artist: &AnyObject) {
    let fitted: NSSize = msg_send![title, sizeThatFits: NSSize::new(264., 1000.)];
    let height = fitted.height.ceil().clamp(20., 42.);
    let _: () = msg_send![title, setFrame: rect(18., 232. - height, 264., height)];
    let _: () = msg_send![artist, setFrame: rect(18., 232. - height - 24., 264., 18.)];
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}
unsafe fn label(content: &AnyObject, text: &str, frame: NSRect, size: f64) -> Obj {
    let label: Obj =
        msg_send![class!(NSTextField), wrappingLabelWithString: &*NSString::from_str(text)];
    let _: () = msg_send![&*label, setFrame: frame];
    let _: () = msg_send![&*label, setAlignment: 1isize];
    let _: () = msg_send![&*label, setPreferredMaxLayoutWidth: frame.size.width];
    let cell: Obj = msg_send![&*label, cell];
    let _: () = msg_send![&*cell, setTruncatesLastVisibleLine: Bool::YES];
    let font: Obj = msg_send![class!(NSFont), systemFontOfSize: size];
    let _: () = msg_send![&*label, setFont: &*font];
    let _: () = msg_send![content, addSubview: &*label];
    label
}
unsafe fn menu(app: &AnyObject) {
    let menu: Obj = msg_send![class!(NSMenu), new];
    let root: Obj = msg_send![class!(NSMenuItem), new];
    let submenu: Obj = msg_send![class!(NSMenu), new];
    let quit: Allocated<AnyObject> = msg_send![class!(NSMenuItem), alloc];
    let quit: Obj = msg_send![quit, initWithTitle: &*NSString::from_str("Quit Radio Swiss Classic"),
        action: sel!(terminate:), keyEquivalent: &*NSString::from_str("q")];
    let _: () = msg_send![&*submenu, addItem: &*quit];
    let _: () = msg_send![&*root, setSubmenu: &*submenu];
    let _: () = msg_send![&*menu, addItem: &*root];
    let _: () = msg_send![app, setMainMenu: &*menu];
}

pub fn run() {
    let mtm = MainThreadMarker::new().expect("AppKit needs the main thread");
    let (app, _controller, timer) = autoreleasepool(|_| unsafe {
        let app: Obj = msg_send![class!(NSApplication), sharedApplication];
        let _: Bool = msg_send![&*app, setActivationPolicy: 0isize];
        menu(&app);
        let window: Allocated<AnyObject> = msg_send![class!(NSWindow), alloc];
        let window: Obj = msg_send![window, initWithContentRect: rect(0., 0., 300., 450.),
            styleMask: 3usize, backing: 2usize, defer: Bool::NO];
        let _: () = msg_send![&*window, setReleasedWhenClosed: Bool::NO];
        let _: () = msg_send![&*window, setTitle: &*NSString::from_str("Radio Swiss Classic")];
        let appearance: Obj = msg_send![class!(NSAppearance), appearanceNamed: &*NSString::from_str("NSAppearanceNameDarkAqua")];
        let _: () = msg_send![&*window, setAppearance: &*appearance];
        let background = color(14., 17., 22.);
        let _: () = msg_send![&*window, setBackgroundColor: &*background];
        let content: Obj = msg_send![&*window, contentView];
        let data = include_bytes!("../assets/logo.png");
        let data: Obj = msg_send![class!(NSData), dataWithBytes: data.as_ptr().cast::<std::ffi::c_void>(), length: data.len()];
        let image: Allocated<AnyObject> = msg_send![class!(NSImage), alloc];
        let image: Obj = msg_send![image, initWithData: &*data];
        let image_view: Allocated<AnyObject> = msg_send![class!(NSImageView), alloc];
        let image_view: Obj = msg_send![image_view, initWithFrame: rect(91., 288., 118., 136.)];
        let _: () = msg_send![&*image_view, setImage: &*image];
        let _: () = msg_send![&*image_view, setImageScaling: 0usize];
        let _: () = msg_send![&*content, addSubview: &*image_view];
        let heading = label(
            &content,
            "C U R R E N T L Y  P L A Y I N G",
            rect(18., 240., 264., 16.),
            10.,
        );
        let accent = color(8., 158., 219.);
        let _: () = msg_send![&*heading, setTextColor: &*accent];
        let title = label(&content, "…", rect(18., 212., 264., 20.), 15.);
        let title_font: Obj = msg_send![class!(NSFont), systemFontOfSize: 15f64, weight: 0.23f64];
        let _: () = msg_send![&*title, setFont: &*title_font];
        let _: () = msg_send![&*title, setMaximumNumberOfLines: 2isize];
        let title_color = color(245., 246., 247.);
        let _: () = msg_send![&*title, setTextColor: &*title_color];
        let artist = label(&content, "", rect(18., 188., 264., 18.), 13.);
        let _: () = msg_send![&*artist, setMaximumNumberOfLines: 1isize];
        let artist_color = color(201., 205., 211.);
        let _: () = msg_send![&*artist, setTextColor: &*artist_color];
        let defaults: Obj = msg_send![class!(NSUserDefaults), standardUserDefaults];
        let saved: isize = msg_send![&*defaults, integerForKey: &*NSString::from_str("language")];
        let has_language: Option<Obj> =
            msg_send![&*defaults, objectForKey: &*NSString::from_str("language")];
        let preferred: Obj = msg_send![class!(NSLocale), preferredLanguages];
        let first: Option<Retained<NSString>> = msg_send![&*preferred, firstObject];
        let locale = first.map(|s| s.to_string()).unwrap_or_default();
        let fallback = CHANNELS
            .iter()
            .position(|(code, _, _)| locale.starts_with(code))
            .unwrap_or(0);
        let language = if has_language.is_some() && (0..3).contains(&saved) {
            saved as usize
        } else {
            fallback
        };
        let existing: Option<Obj> =
            msg_send![&*defaults, objectForKey: &*NSString::from_str("volume")];
        let volume: f64 = if existing.is_some() {
            msg_send![&*defaults, doubleForKey: &*NSString::from_str("volume")]
        } else {
            0.8
        };
        let volume = if volume.is_finite() {
            volume.clamp(0., 1.)
        } else {
            0.8
        };
        let controller = Controller::alloc(mtm).set_ivars(RefCell::new(None));
        let controller: Retained<Controller> = msg_send![super(controller), init];
        let languages = std::array::from_fn(|i| {
            let button: Obj = msg_send![class!(NSButton), buttonWithTitle: &*NSString::from_str(&CHANNELS[i].0.to_uppercase()), target: &*controller, action: sel!(language:)];
            let _: () = msg_send![&*button, setFrame: rect(93. + 40. * i as f64, 135., 34., 22.)];
            let _: () = msg_send![&*button, setTag: i as isize];
            let _: () = msg_send![&*button, setBordered: Bool::NO];
            let font: Obj = msg_send![class!(NSFont), systemFontOfSize: 10f64, weight: 0.23f64];
            let _: () = msg_send![&*button, setFont: &*font];
            let _: () = msg_send![&*button, setWantsLayer: Bool::YES];
            let layer: Obj = msg_send![&*button, layer];
            let _: () = msg_send![&*layer, setCornerRadius: 11f64];
            let _: () = msg_send![&*layer, setBorderWidth: 1f64];
            let _: () = msg_send![&*content, addSubview: &*button];
            button
        });
        paint_languages(&languages, language);
        let button: Obj = msg_send![class!(NSButton), buttonWithTitle: &*NSString::from_str(""), target: &*controller, action: sel!(toggle:)];
        let _: () = msg_send![&*button, setBordered: Bool::NO];
        let _: () = msg_send![&*button, setWantsLayer: Bool::YES];
        let layer: Obj = msg_send![&*button, layer];
        let _: () = msg_send![&*layer, setCornerRadius: 27f64];
        let cg_color: *const CGColor = msg_send![&*accent, CGColor];
        let _: () = msg_send![&*layer, setBackgroundColor: cg_color];
        let white = color(255., 255., 255.);
        let _: () = msg_send![&*button, setContentTintColor: &*white];
        set_playing(&button, false);
        let _: () = msg_send![&*content, addSubview: &*button];
        // Reserve the gap above the circle for connection and error messages.
        let status = label(&content, "", rect(12., 115.5, 276., 14.), 10.);
        let _: () = msg_send![&*status, setMaximumNumberOfLines: 1isize];
        let status_color = color(154., 160., 166.);
        let _: () = msg_send![&*status, setTextColor: &*status_color];
        let slider: Allocated<AnyObject> = msg_send![class!(NSSlider), alloc];
        let slider: Obj = msg_send![slider, initWithFrame: rect(38., 14., 224., 24.)];
        let _: () = msg_send![&*slider, setAccessibilityLabel: &*NSString::from_str("Volume")];
        let _: () = msg_send![&*slider, setMinValue: 0f64];
        let _: () = msg_send![&*slider, setMaxValue: 1f64];
        let _: () = msg_send![&*slider, setDoubleValue: volume];
        let _: () = msg_send![&*slider, setContinuous: Bool::YES];
        let _: () = msg_send![&*slider, setTarget: &*controller];
        let _: () = msg_send![&*slider, setAction: sel!(volume:)];
        let _: () = msg_send![&*content, addSubview: &*slider];
        // Equal visible gaps to the language pills and the slider's track.
        let _: () = msg_send![&*button, setFrame: rect(123., 54.5, 54., 54.)];
        let player: Obj = msg_send![class!(AVPlayer), new];
        let _: () = msg_send![&*player, setVolume: volume as f32];
        let (commands, updates) = metadata_worker(language);
        let smoke = std::env::args().any(|arg| arg == "--smoke-test");
        let mut state = State {
            app: app.clone(),
            _window: window.clone(),
            player,
            button,
            languages,
            title,
            artist,
            status,
            defaults,
            language,
            requested: false,
            last_control: -1,
            pending_language: None,
            commands,
            updates,
            smoke: smoke.then(Instant::now),
            smoke_stage: 0,
            smoke_played: [false; 3],
            smoke_metadata: [false; 3],
        };
        if smoke {
            state.language = 0;
            state.pending_language = Some(0);
            paint_languages(&state.languages, 0);
            let _: () = msg_send![&*state.player, setVolume: 0f32];
            state.start();
        }
        *controller.ivars().borrow_mut() = Some(state);
        let _: () = msg_send![&*window, setDelegate: &*controller];
        let timer: Obj = msg_send![class!(NSTimer), timerWithTimeInterval: 1f64, target: &*controller,
            selector: sel!(tick:), userInfo: std::ptr::null::<AnyObject>(), repeats: Bool::YES];
        let _: () = msg_send![&*timer, setTolerance: 0.2f64];
        let run_loop: Obj = msg_send![class!(NSRunLoop), mainRunLoop];
        let _: () = msg_send![&*run_loop, addTimer: &*timer, forMode: &*NSString::from_str("kCFRunLoopCommonModes")];
        let _: () = msg_send![&*window, center];
        let _: () = msg_send![&*window, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
        let _: () = msg_send![&*app, activateIgnoringOtherApps: Bool::YES];
        (app, controller, timer)
    });
    // Drain startup temporaries before entering AppKit's own event autorelease pools.
    unsafe {
        let _: () = msg_send![&*app, run];
        let _: () = msg_send![&*timer, invalidate];
    }
}
