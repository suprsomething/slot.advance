mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::core_lock;
use slot::app::Phase;
use slot::audio::{AudioSink, StubSink};
use slot::core::{apply_core_options, open_core_for};
use slot::emu::{CoreState, EmuHandle};
use slot::persist::Snapshot;
use slot::session::Session;
use slot_input::{Btn, RawEvent};
use slot_retro::{ButtonMask, LibretroCore, RetroCore};
use slot_store::{Core, Platform};

fn rom(name: &str) -> PathBuf {
    let p = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::write(&p, common::gba_rom()).expect("write rom");
    p
}

fn is_mgba(core: &mut dyn RetroCore, rom: &Path) -> bool {
    core.load(rom).expect("core refused the test rom");
    core.run_frame(ButtonMask::default());
    core.serialize().expect("core gave up no state").len() > 100_000
}

#[test]
fn the_vendored_core_is_preferred_over_the_mock() {
    let _g = core_lock();
    let Some(dylib) = common::vendored_core() else {
        return;
    };
    let d = common::tmp_root_with_real_carts(&[]);
    let mut core = open_core_for(d.path(), Core::Mgba, "auto", false, &[dylib]);
    assert!(
        is_mgba(core.as_mut(), &rom("preferred.gba")),
        "the mock ran with a vendored core sitting right there"
    );
}

#[test]
fn a_missing_core_falls_back_to_the_mock_rather_than_failing() {
    let d = common::tmp_root_with_real_carts(&[]);
    let mut core = open_core_for(
        d.path(),
        Core::Mgba,
        "auto",
        false,
        &[PathBuf::from("no/such/core.dylib")],
    );
    assert!(!is_mgba(core.as_mut(), &rom("missing.gba")));
}

#[test]
fn a_rom_the_real_core_refuses_reports_failed() {
    let _g = core_lock();
    let Some(dylib) = common::vendored_core() else {
        return;
    };
    let d = common::tmp_root_with_carts(&["Broken"]);
    let emu = EmuHandle::spawn(
        open_core_for(d.path(), Core::Mgba, "auto", false, &[dylib]),
        d.path().join("Games/GBA/Broken.gba"),
        StubSink::new().ring(),
        None,
        None,
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while emu.state() == CoreState::Loading {
        assert!(Instant::now() < deadline, "the core never settled");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(emu.state(), CoreState::Failed);
}

#[test]
fn a_cart_the_real_core_refuses_comes_back_out_of_the_slot() {
    let _g = core_lock();
    let Some(dylib) = common::vendored_core() else {
        return;
    };
    std::env::set_var("SLOT_CORE", dylib);
    let d = common::tmp_root_with_carts(&["Broken"]);
    common::clocked(d.path());
    let mut s = Session::boot(d.path().to_path_buf());
    s.feed([RawEvent::Down(Btn::A)], 16);
    s.update(1.0 / 60.0);
    assert!(matches!(s.app().phase(), Phase::Inserting { .. }));

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut now = 16;
    while !matches!(s.app().phase(), Phase::Shelf) {
        assert!(
            Instant::now() < deadline,
            "the refused cart never came back out"
        );
        now += 16;
        s.feed([], now);
        s.update(1.0 / 60.0);
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// A tiny GBA program on a 128 KB flash chip: it switches the chip to its
/// second bank at once, waits for A, then programs 0x42 into address 0 of
/// whichever bank the chip is on. The game code B24E is in mGBA's override
/// table as 128 KB flash with no other hardware, which is all it is used for.
fn flash1m_bank_rom() -> Vec<u8> {
    const CODE: [u32; 25] = [
        0xe3a0040e, // mov r0, #0x0E000000      flash base
        0xe2801c55, // add r1, r0, #0x5500
        0xe2811055, // add r1, r1, #0x55        0x0E005555
        0xe2802c2a, // add r2, r0, #0x2A00
        0xe28220aa, // add r2, r2, #0xAA        0x0E002AAA
        0xe3a030aa, // mov r3, #0xAA
        0xe3a04055, // mov r4, #0x55
        0xe5c13000, // strb r3, [r1]
        0xe5c24000, // strb r4, [r2]
        0xe3a050b0, // mov r5, #0xB0            switch bank
        0xe5c15000, // strb r5, [r1]
        0xe3a05001, // mov r5, #1
        0xe5c05000, // strb r5, [r0]            bank 1
        0xe3a06301, // mov r6, #0x04000000
        0xe2866e13, // add r6, r6, #0x130       KEYINPUT
        0xe1d670b0, // wait: ldrh r7, [r6]
        0xe3170001, // tst r7, #1               A held reads 0
        0x1afffffc, // bne wait
        0xe5c13000, // strb r3, [r1]
        0xe5c24000, // strb r4, [r2]
        0xe3a050a0, // mov r5, #0xA0            program byte
        0xe5c15000, // strb r5, [r1]
        0xe3a05042, // mov r5, #0x42
        0xe5c05000, // strb r5, [r0]            0x42 at address 0 of the current bank
        0xeafffffe, // done: b done
    ];
    let mut rom = common::gba_rom();
    rom[0xa0..0xac].copy_from_slice(b"FLASH BANK\0\0");
    rom[0xac..0xb0].copy_from_slice(b"B24E");
    rom[0xbd] = 0;
    let sum = rom[0xa0..0xbd].iter().fold(0u8, |a, b| a.wrapping_add(*b));
    rom[0xbd] = 0u8.wrapping_sub(sum).wrapping_sub(0x19);
    rom[0xc0..0x200].fill(0);
    for (i, w) in CODE.iter().enumerate() {
        let o = 0xc0 + i * 4;
        rom[o..o + 4].copy_from_slice(&w.to_le_bytes());
    }
    rom
}

#[test]
fn on_mgba_a_state_load_keeps_a_128k_flash_game_on_the_bank_it_was_on() {
    let _g = core_lock();
    let Some(dylib) = common::vendored_core() else {
        eprintln!("no vendored mGBA core, skipping");
        return;
    };
    let rom = Path::new(env!("CARGO_TARGET_TMPDIR")).join("flash1m-bank.gba");
    std::fs::write(&rom, flash1m_bank_rom()).expect("write rom");
    let save_after_write = |reload: bool| {
        let mut core =
            LibretroCore::open(&dylib).expect("vendored core is present but would not open");
        apply_core_options(&mut core, Core::Mgba, "auto", false, false, None);
        core.load(&rom).expect("mGBA refused the rom");
        for _ in 0..30 {
            core.run_frame(ButtonMask::default());
            let _ = core.take_audio();
        }
        if reload {
            let state = core.serialize().expect("mGBA would not save");
            core.unserialize(&state)
                .expect("mGBA would not load its own state");
        }
        for frame in 0..30 {
            let held = if frame < 10 { ButtonMask::A } else { 0 };
            core.run_frame(ButtonMask(held));
            let _ = core.take_audio();
        }
        core.save_ram().expect("mGBA gave no save")
    };
    let played = save_after_write(false);
    assert_eq!(
        played.len(),
        0x20000,
        "mGBA did not treat the test rom as 128 KB flash, so this test proves nothing"
    );
    assert_eq!(
        (played[0], played[0x10000]),
        (0xff, 0x42),
        "without a state load the byte should land in the second bank, so this test proves nothing"
    );
    let reloaded = save_after_write(true);
    assert_eq!(
        (reloaded[0], reloaded[0x10000]),
        (0xff, 0x42),
        "after a state load the game's write landed in the wrong bank \
         (bank 0 byte {:#04x}, bank 1 byte {:#04x})",
        reloaded[0],
        reloaded[0x10000]
    );
}

/// A tiny Game Boy program on an MBC2 cartridge (type 0x06, MBC2 with
/// battery): it enables the cart's built-in RAM, reads it, writes 5 to its
/// first cell and stops.
fn mbc2_rom() -> Vec<u8> {
    const CODE: [u8; 13] = [
        0x3e, 0x0a, // ld a, 0x0A
        0xea, 0x00, 0x00, // ld (0x0000), a       enable cart RAM
        0xfa, 0x00, 0xa0, // ld a, (0xA000)       first read of save RAM
        0x3e, 0x05, // ld a, 5
        0xea, 0x00, 0xa0, // ld (0xA000), a       write 5
    ];
    let mut rom = vec![0u8; 0x8000];
    rom[0x100..0x104].copy_from_slice(&[0x00, 0xc3, 0x50, 0x01]); // nop; jp 0x0150
    rom[0x104..0x108].copy_from_slice(&[0xce, 0xed, 0x66, 0x66]); // start of the header logo
    rom[0x134..0x13d].copy_from_slice(b"MBC2 TEST");
    rom[0x147] = 0x06;
    rom[0x150..0x150 + CODE.len()].copy_from_slice(&CODE);
    rom[0x150 + CODE.len()..0x150 + CODE.len() + 2].copy_from_slice(&[0x18, 0xfe]); // jr -2
    rom[0x14d] = rom[0x134..0x14d]
        .iter()
        .fold(0u8, |a, b| a.wrapping_sub(*b).wrapping_sub(1));
    rom
}

const MBC2_CHILD: &str = "SLOT_TEST_MBC2_CHILD";

#[test]
fn on_mgba_an_mbc2_game_reading_its_save_ram_neither_crashes_nor_loses_the_write() {
    let Some(dylib) = common::vendored_core() else {
        eprintln!("no vendored mGBA core, skipping");
        return;
    };
    if std::env::var_os(MBC2_CHILD).is_some() {
        let rom = Path::new(env!("CARGO_TARGET_TMPDIR")).join("mbc2.gb");
        std::fs::write(&rom, mbc2_rom()).expect("write rom");
        let mut core =
            LibretroCore::open(&dylib).expect("vendored core is present but would not open");
        apply_core_options(&mut core, Core::Mgba, "auto", false, false, None);
        core.load(&rom).expect("mGBA refused the rom");
        for _ in 0..60 {
            core.run_frame(ButtonMask::default());
            let _ = core.take_audio();
        }
        let save = core.save_ram().expect("mGBA gave no save");
        assert_eq!(
            save.first().map(|b| b & 0x0f),
            Some(5),
            "the game's write to its MBC2 RAM never reached the save"
        );
        return;
    }
    // The defect is a crash inside the core, which would take the whole test
    // binary down, so the core runs in a child copy of this test.
    let out = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "on_mgba_an_mbc2_game_reading_its_save_ram_neither_crashes_nor_loses_the_write",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(MBC2_CHILD, "1")
        .output()
        .expect("could not run the child test");
    assert!(
        out.status.success(),
        "the MBC2 game took the core down or lost its write ({}):\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn on_mgba_a_zero_byte_resume_on_the_card_starts_the_game_instead_of_crashing() {
    let _g = core_lock();
    let Some(dylib) = common::vendored_core() else {
        eprintln!("no vendored mGBA core, skipping");
        return;
    };
    let d = common::tmp_root_with_real_carts(&["Emerald"]);
    slot::persist::flush(
        d.path(),
        Platform::Gba,
        Core::Mgba,
        "Emerald",
        Some(&[0xA5u8; 64]),
        None,
    )
    .expect("write a resume");
    let dir = d.path().join("States/GBA/mgba/Emerald");
    let resume_file = std::fs::read_dir(&dir)
        .expect("the states directory")
        .map(|e| e.expect("entry").path())
        .find(|p| p.file_stem().and_then(|s| s.to_str()) == Some("resume"))
        .expect("flush wrote no resume file");
    std::fs::write(&resume_file, []).expect("empty the resume");
    let resume = slot::persist::read_resume(d.path(), Platform::Gba, Core::Mgba, "Emerald");
    assert_eq!(
        resume.as_deref(),
        Some(&[][..]),
        "the card's zero-byte resume did not reach the core, so this test proves nothing"
    );

    let core = open_core_for(d.path(), Core::Mgba, "auto", false, &[dylib]);
    let emu = EmuHandle::spawn(
        core,
        d.path().join("Games/GBA/Emerald.gba"),
        StubSink::new().ring(),
        None,
        resume,
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while emu.state() == CoreState::Loading {
        assert!(Instant::now() < deadline, "the core never settled");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        emu.state(),
        CoreState::Ready,
        "the game did not start after the empty resume was refused"
    );
    assert!(
        !emu.snapshot().resume_trusted(),
        "the empty resume was taken as good, so the next save could overwrite it"
    );
}
