//! Renders the Network screens (0.5.0 phases 1b and 2) through the real menu logic
//! (`slot::net_menu`) and drawing code (`slot_ui::net_face`), at the RG SP's 720 x 480, for
//! review. The network thread and Syncthing's supervisor are stand-ins with fixed answers.
//!
//! Run with: cargo run -p slot --no-default-features --example network_screens -- OUT_DIR

use std::path::{Path, PathBuf};

use slot::net::Net;
use slot::net_menu::NetMenu;
use slot_input::{Action, Btn};
use slot_net::sync::{Service, Shown, SyncStatus};
use slot_net::{Link, Status};
use slot_ui::{net_face, NetView};

#[derive(Clone)]
struct Fake {
    running: bool,
    link: Link,
    sync: Option<SyncStatus>,
    seen: Vec<slot_net::Seen>,
    saved: Vec<Vec<u8>>,
}

impl Net for Fake {
    fn running(&self) -> bool {
        self.running
    }
    fn status(&self) -> Option<Status> {
        Some(Status {
            link: self.link.clone(),
            seen: self.seen.clone(),
            saved: self.saved.clone(),
            ..Status::default()
        })
    }
    fn sync_status(&self) -> Option<SyncStatus> {
        self.sync.clone()
    }
}

fn connected() -> Link {
    Link::Connected {
        ssid: b"DropBearNet".to_vec(),
        addr: Some("192.168.4.162".parse().unwrap()),
    }
}

fn running(password: Shown) -> Option<SyncStatus> {
    Some(SyncStatus {
        service: Service::Running,
        password,
        ..SyncStatus::default()
    })
}

fn save(view: &NetView, path: &Path) {
    let face = net_face(view);
    let file = std::fs::File::create(path).expect("create");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), face.w, face.h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().expect("header");
    w.write_image_data(&face.rgba).expect("write");
    println!("{}", path.display());
}

/// Opens the menu, presses `keys`, and returns what it shows.
fn shot(net: &mut Fake, wifi: bool, sync: bool, keys: &[Btn]) -> NetView {
    let mut m = NetMenu::new();
    for k in keys {
        m.input(Action::GbaDown(*k), net, wifi, sync);
    }
    m.view(net, wifi, sync)
}

fn main() {
    let out = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "network-screens".into()),
    );
    std::fs::create_dir_all(&out).expect("output folder");
    let pw = Shown::Password("abacus zoom ablaze quarry dimple".into());

    let mut off = Fake {
        running: false,
        link: Link::Off,
        sync: None,
        seen: vec![],
        saved: vec![],
    };
    save(
        &shot(&mut off, false, false, &[]),
        &out.join("10-menu-wifi-off.png"),
    );

    let mut on_sync_off = Fake {
        running: true,
        link: connected(),
        sync: Some(SyncStatus::default()),
        seen: vec![],
        saved: vec![],
    };
    save(
        &shot(&mut on_sync_off, true, false, &[Btn::Down, Btn::Down]),
        &out.join("11-menu-syncthing-off.png"),
    );

    let mut on = Fake {
        running: true,
        link: connected(),
        sync: running(pw.clone()),
        seen: vec![],
        saved: vec![],
    };
    save(
        &shot(&mut on, true, true, &[Btn::Down, Btn::Down]),
        &out.join("12-menu-syncthing-on-running.png"),
    );
    save(
        &shot(&mut on, true, true, &[Btn::Down, Btn::Down, Btn::Down]),
        &out.join("13-menu-web-interface-row.png"),
    );
    let mut err = on.clone();
    err.sync.as_mut().unwrap().service = Service::Error;
    save(
        &shot(&mut err, true, true, &[Btn::Down, Btn::Down]),
        &out.join("14-menu-service-error.png"),
    );
    let web = [Btn::Down, Btn::Down, Btn::Down, Btn::A];
    save(
        &shot(&mut on, true, true, &web),
        &out.join("15-web-interface.png"),
    );
    let mut changed = Fake {
        sync: running(Shown::Changed),
        ..on.clone()
    };
    save(
        &shot(&mut changed, true, true, &web),
        &out.join("16-web-interface-changed.png"),
    );
    let mut offline = Fake {
        link: Link::Connecting(None),
        ..on.clone()
    };
    save(
        &shot(&mut offline, true, true, &web),
        &out.join("17-web-interface-not-connected.png"),
    );
    let mut starting = on.clone();
    starting.sync = Some(SyncStatus {
        starting: true,
        password: pw,
        ..SyncStatus::default()
    });
    save(
        &shot(&mut starting, true, true, &web),
        &out.join("18-web-interface-starting.png"),
    );
    // The network list: names, signal and lock centred in their rows.
    let mut list = on.clone();
    list.saved = vec![b"DropBearNet".to_vec(), b"Office".to_vec()];
    list.seen = vec![
        slot_net::Seen {
            ssid: b"DropBearNet".to_vec(),
            signal: -45,
            security: slot_net::Security::Wpa2,
        },
        slot_net::Seen {
            ssid: b"Neighbour".to_vec(),
            signal: -63,
            security: slot_net::Security::Wpa2,
        },
        slot_net::Seen {
            ssid: b"Cafe".to_vec(),
            signal: -71,
            security: slot_net::Security::Open,
        },
    ];
    save(
        &shot(&mut list, true, true, &[Btn::Down, Btn::A]),
        &out.join("20-networks-list.png"),
    );
    // The widest possible passphrase in pixels: the five widest EFF words, measured in the
    // label font at 24 px (the longest by letters is narrower).
    let longest = "mammogram womanhood amendment endowment commodore".to_string();
    let words = slot_net::sync::creds::wordlist();
    assert!(longest.split(' ').all(|w| words.contains(&w)));
    println!(
        "longest passphrase: {longest} ({} characters)",
        longest.len()
    );
    let mut long = on.clone();
    long.sync = running(Shown::Password(longest));
    save(
        &shot(&mut long, true, true, &web),
        &out.join("21-web-interface-longest.png"),
    );
    let mut reset = web.to_vec();
    reset.push(Btn::Y);
    save(
        &shot(&mut on, true, true, &reset),
        &out.join("19-reset-confirm.png"),
    );
}
