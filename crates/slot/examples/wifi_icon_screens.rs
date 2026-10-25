//! Renders the carousel with the Wi-Fi icon in each of its six states (0.5.0, Compact Arcs)
//! through the real app and compositor, at 720 x 480, for review. The Wi-Fi thread is a
//! stand-in with fixed answers; the scene is the README's (same carts, labels and clock).
//!
//! Run with: cargo run -p slot --no-default-features --example wifi_icon_screens -- OUT_DIR

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use slot::app::{App, STAR_PX};
use slot::net::Net;
use slot_gfx::{Compositor, HeadlessSurface, OUT_H, OUT_W};
use slot_net::{Link, Signal, Status};
use slot_store::{write_slot_state, SlotState};
use slot_ui::{cart_face, cart_shadow, star_face, wifi_icon_face, word_face, WifiIcon};

/// Name, Wi-Fi running, link, reading, the icon expected.
type Case = (&'static str, bool, Link, Option<Signal>, Option<WifiIcon>);

#[derive(Clone)]
struct Fake(Arc<Mutex<(bool, Status)>>);

impl Net for Fake {
    fn running(&self) -> bool {
        self.0.lock().unwrap().0
    }
    fn status(&self) -> Option<Status> {
        Some(self.0.lock().unwrap().1.clone())
    }
}

fn load_faces(app: &mut App, c: &mut Compositor) {
    let row = app
        .carts()
        .map(|cart| {
            let f = cart_face(cart);
            c.create_texture(f.w, f.h, &f.rgba)
        })
        .collect();
    app.set_faces(row);
    for want in app.face_wants() {
        let cart = app.face_cart(want.shelf, want.index).unwrap().clone();
        let f = cart_face(&cart);
        let tex = c.create_texture(f.w, f.h, &f.rgba);
        app.set_cart_face(want.shelf, want.index, tex);
    }
}

fn main() {
    let out = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "wifi-icon".into()),
    );
    std::fs::create_dir_all(&out).unwrap();
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let pack = repo.join("artwork/gba-originals");
    let shots = repo.join(".github/resources/screenshots/advance");
    let root = tempfile::tempdir().unwrap();
    for dir in ["Games/GBA", "Labels/GBA", "Config"] {
        std::fs::create_dir_all(root.path().join(dir)).unwrap();
    }
    let names = [
        "Kirby - Nightmare in Dream Land",
        "Pokemon - Unbound",
        "Pokemon - Emerald Version (USA, Europe)",
    ];
    for name in names {
        let mut rom = vec![0u8; 256];
        rom[0xa0..0xa4].copy_from_slice(b"DEMO");
        std::fs::write(root.path().join(format!("Games/GBA/{name}.gba")), rom).unwrap();
        std::fs::copy(
            if name == names[1] {
                shots.join("sources/pokemon-unbound-label.png")
            } else {
                pack.join(format!("{name}.png"))
            },
            root.path().join(format!("Labels/GBA/{name}.png")),
        )
        .unwrap();
    }
    write_slot_state(
        root.path(),
        &SlotState {
            clock_set: true,
            wifi: true,
            ..Default::default()
        },
    )
    .unwrap();
    let surface = HeadlessSurface::new().expect("headless rendering");
    let mut c = Compositor::new(&surface).unwrap();
    let mut app = App::boot(root.path());
    load_faces(&mut app, &mut c);
    let shadow = cart_shadow();
    let tex = c.create_texture(shadow.w, shadow.h, &shadow.rgba);
    app.set_cart_shadow(tex);
    let star = star_face(STAR_PX);
    let tex = c.create_texture(star.w, star.h, &star.rgba);
    app.set_star_face(tex, star.w, star.h);
    let clock = word_face("12:30");
    let tex = c.create_texture(clock.w, clock.h, &clock.rgba);
    app.set_shelf_clock_face(tex, clock.w);
    let wifi = WifiIcon::ALL.map(|i| {
        let f = wifi_icon_face(i);
        c.create_texture(f.w, f.h, &f.rgba)
    });
    app.set_wifi_faces(wifi);
    let net = Fake(Arc::new(Mutex::new((true, Status::default()))));
    app.set_net(Box::new(net.clone()));
    let connected = Link::Connected {
        ssid: b"DropBearNet".to_vec(),
        addr: Some("192.168.4.162".parse().unwrap()),
    };
    let reading = |dbm| {
        Some(Signal {
            dbm,
            at: Instant::now(),
        })
    };
    let states: [Case; 6] = [
        (
            "1-weak",
            true,
            connected.clone(),
            reading(-78),
            Some(WifiIcon::Weak),
        ),
        (
            "2-moderate",
            true,
            connected.clone(),
            reading(-66),
            Some(WifiIcon::Moderate),
        ),
        (
            "3-strong",
            true,
            connected.clone(),
            reading(-48),
            Some(WifiIcon::Strong),
        ),
        (
            "4-disconnected",
            true,
            Link::Connecting(None),
            None,
            Some(WifiIcon::NotConnected),
        ),
        (
            "5-unknown",
            true,
            connected.clone(),
            None,
            Some(WifiIcon::Unknown),
        ),
        ("6-wifi-off", false, Link::Off, None, None),
    ];
    for (name, running, link, signal, want) in states {
        {
            let mut s = net.0.lock().unwrap();
            s.0 = running;
            s.1.link = link;
            s.1.signal = signal;
        }
        for _ in 0..200 {
            app.update(0.01);
        }
        assert_eq!(app.wifi_icon(), want, "{name}");
        let mut draw = Vec::new();
        app.draw(&mut draw);
        c.begin_frame();
        c.draw_list(&draw);
        let pixels = c.read_frame();
        let path = out.join(format!("{name}.png"));
        let mut enc = png::Encoder::new(std::fs::File::create(&path).unwrap(), OUT_W, OUT_H);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()
            .unwrap()
            .write_image_data(&pixels)
            .unwrap();
        println!("{}", path.display());
    }
}
