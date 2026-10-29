//! Renders the 0.5.0 README and website screenshots of the carousel and the Network screens
//! through the real app, menu logic (`slot::net_menu`) and drawing code, at the RG SP's
//! 720 x 480. The network thread and Syncthing's supervisor are stand-ins with fixed
//! answers, and every name, address and password shown is an invented example: the network
//! "Home Wi-Fi", the address 192.168.1.42 and the passphrase below are not real
//! credentials. Each RG SP makes its own random passphrase at first setup.
//!
//! The in-game menu screenshots come from `crates/slot/tests/render_in_game_menu.rs`; see
//! `.github/resources/screenshots/advance/0.5.0/README.md` for how each image was made.
//!
//! Run with:
//! cargo run -p slot --no-default-features --example readme_050_screenshots [-- OUT_DIR]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use slot::app::{App, STAR_PX};
use slot::net::Net;
use slot::net_menu::NetMenu;
use slot_gfx::{Compositor, HeadlessSurface, OUT_H, OUT_W};
use slot_input::{Action, Btn};
use slot_net::sync::{Service, Shown, SyncStatus};
use slot_net::{Link, Security, Seen, Signal, Status};
use slot_store::{write_slot_state, SlotState};
use slot_ui::{
    cart_face, cart_shadow, net_face, star_face, wifi_icon_face, word_face, NetView, WifiIcon,
};

/// An invented example, made of five words from Syncthing's passphrase word list.
const EXAMPLE_PASSPHRASE: &str = "lantern velvet canyon harbor pebble";
const EXAMPLE_NETWORK: &[u8] = b"Home Wi-Fi";
const EXAMPLE_ADDRESS: &str = "192.168.1.42";

#[derive(Clone)]
struct Fake {
    running: bool,
    status: Status,
    sync: Option<SyncStatus>,
}

impl Net for Fake {
    fn running(&self) -> bool {
        self.running
    }
    fn status(&self) -> Option<Status> {
        Some(self.status.clone())
    }
    fn sync_status(&self) -> Option<SyncStatus> {
        self.sync.clone()
    }
}

/// The carousel's stand-in, shared so the app can keep reading it.
#[derive(Clone)]
struct Shared(Arc<Mutex<Fake>>);

impl Net for Shared {
    fn running(&self) -> bool {
        self.0.lock().unwrap().running()
    }
    fn status(&self) -> Option<Status> {
        self.0.lock().unwrap().status()
    }
    fn sync_status(&self) -> Option<SyncStatus> {
        self.0.lock().unwrap().sync_status()
    }
}

fn connected() -> Link {
    Link::Connected {
        ssid: EXAMPLE_NETWORK.to_vec(),
        addr: Some(EXAMPLE_ADDRESS.parse().unwrap()),
    }
}

fn write_png(pixels: &[u8], w: u32, h: u32, path: &Path) {
    let file = std::fs::File::create(path).expect("create png");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .expect("png header")
        .write_image_data(pixels)
        .expect("png data");
    println!("Wrote {}", path.display());
}

fn save_menu(view: &NetView, path: &Path) {
    let face = net_face(view);
    assert_eq!((face.w, face.h), (OUT_W, OUT_H), "full-screen menu");
    write_png(&face.rgba, face.w, face.h, path);
}

/// Opens the Network menu, presses `keys`, and returns what it shows.
fn menu(net: &mut Fake, sync_on: bool, keys: &[Btn]) -> NetView {
    let wifi_on = net.running;
    let mut m = NetMenu::new();
    for k in keys {
        m.input(Action::GbaDown(*k), net, wifi_on, sync_on);
    }
    m.view(net, wifi_on, sync_on)
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

/// The README's carousel scene (the same carts, labels, favourites and clock as
/// `readme_screenshots`), with Wi-Fi On and connected, so the carousel shows its Wi-Fi icon.
fn carousel(out: &Path, net: &Fake) {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let pack = repo.join("artwork/gba-originals");
    let sources = repo.join(".github/resources/screenshots/advance/sources");
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
                sources.join("pokemon-unbound-label.png")
            } else {
                pack.join(format!("{name}.png"))
            },
            root.path().join(format!("Labels/GBA/{name}.png")),
        )
        .unwrap();
    }
    std::fs::write(
        root.path().join("Config/favourites.txt"),
        format!("GBA/{}\nGBA/{}\n", names[1], names[2]),
    )
    .unwrap();
    // Played last, so the carousel opens on it, as in `readme_screenshots`.
    std::fs::write(
        root.path().join("Config/recents.txt"),
        format!("GBA/{}\nGBA/{}\nGBA/{}\n", names[1], names[0], names[2]),
    )
    .unwrap();
    write_slot_state(
        root.path(),
        &SlotState {
            clock_set: true,
            wifi: true,
            syncthing: true,
            ..Default::default()
        },
    )
    .unwrap();
    let surface =
        HeadlessSurface::new().expect("Headless rendering is required for screenshot export");
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
    app.set_net(Box::new(Shared(Arc::new(Mutex::new(net.clone())))));
    for _ in 0..200 {
        app.update(0.01);
    }
    let text = app.slot_text().unwrap();
    let face = word_face(&text);
    let tex = c.create_texture(face.w, face.h, &face.rgba);
    app.set_shelf_platform_face(tex, face.w);
    for _ in 0..30 {
        app.update(0.01);
    }
    assert_eq!(app.wifi_icon(), Some(WifiIcon::Strong), "Wi-Fi icon");
    let mut draw = Vec::new();
    app.draw(&mut draw);
    c.begin_frame();
    c.draw_list(&draw);
    let pixels = c.read_frame();
    write_png(&pixels, OUT_W, OUT_H, &out.join("carousel-wifi.png"));
}

fn main() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| repo.join(".github/resources/screenshots/advance/0.5.0"));
    std::fs::create_dir_all(&out).expect("output folder");
    let words = slot_net::sync::creds::wordlist();
    assert!(
        EXAMPLE_PASSPHRASE.split(' ').all(|w| words.contains(&w)),
        "the example passphrase must look like a real one"
    );

    let mut on = Fake {
        running: true,
        status: Status {
            link: connected(),
            signal: Some(Signal {
                dbm: -48,
                at: Instant::now(),
            }),
            ..Status::default()
        },
        sync: Some(SyncStatus {
            service: Service::Running,
            password: Shown::Password(EXAMPLE_PASSPHRASE.into()),
            ..SyncStatus::default()
        }),
    };

    carousel(&out, &on);

    // Network: Wi-Fi On and connected, Syncthing On and running, cursor on Syncthing.
    save_menu(
        &menu(&mut on, true, &[Btn::Down, Btn::Down]),
        &out.join("network-syncthing-on.png"),
    );
    // Web Interface: the address, user and (example) password the RG SP shows.
    save_menu(
        &menu(&mut on, true, &[Btn::Down, Btn::Down, Btn::Down, Btn::A]),
        &out.join("syncthing-web-interface.png"),
    );
    // Network with Wi-Fi Off, the default: Syncthing greyed out.
    let mut off = Fake {
        running: false,
        status: Status::default(),
        sync: None,
    };
    save_menu(
        &menu(&mut off, false, &[]),
        &out.join("network-wifi-off.png"),
    );
    // Networks list (website): saved and nearby networks, all invented names.
    let mut list = on.clone();
    list.status.saved = vec![EXAMPLE_NETWORK.to_vec()];
    list.status.seen = vec![
        Seen {
            ssid: EXAMPLE_NETWORK.to_vec(),
            signal: -45,
            security: Security::Wpa2,
        },
        Seen {
            ssid: b"Upstairs".to_vec(),
            signal: -63,
            security: Security::Wpa2,
        },
        Seen {
            ssid: b"Library Guest".to_vec(),
            signal: -71,
            security: Security::Open,
        },
    ];
    save_menu(
        &menu(&mut list, true, &[Btn::Down, Btn::A]),
        &out.join("networks-list.png"),
    );
}
