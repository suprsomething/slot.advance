use std::path::PathBuf;
use std::time::{Duration, Instant};

use slot::frontend::Frontend;
use slot::input::DeviceInput;
use slot::standby::{Standby, Step, STANDBY_REST};
use slot_gfx::{Compositor, FbdevSurface, Surface};
use slot_power::{quiet_hardware, trace_first_frame, DevicePlatform, Quiet};

const CARD: &str = "/mnt/sdcard";

const MIN_FRAME: Duration = Duration::from_millis(12);

const STEP_TIMEOUT: Duration = Duration::from_millis(12);

const MARGIN: Duration = Duration::from_micros(1500);

struct Pacer {
    last_return: Option<Instant>,
    period: Duration,
    work: Duration,
    works: [Duration; WORK_WINDOW],
    next: usize,
}

const WORK_WINDOW: usize = 32;

impl Pacer {
    fn new() -> Self {
        Pacer {
            last_return: None,
            period: Duration::from_micros(16_760),
            work: Duration::from_millis(4),
            works: [Duration::from_millis(4); WORK_WINDOW],
            next: 0,
        }
    }

    fn wait(&self) {
        let Some(last) = self.last_return else {
            return;
        };
        let delay = self.period.saturating_sub(self.work + MARGIN);
        if let Some(left) = (last + delay).checked_duration_since(Instant::now()) {
            std::thread::sleep(left);
        }
    }

    fn swapped(&mut self, work: Duration, blocked: bool) -> bool {
        let now = Instant::now();
        let mut missed = false;
        if let Some(last) = self.last_return {
            let seen = now - last;
            missed = seen > self.period.mul_f64(1.5);
            if blocked && seen > Duration::from_millis(12) && seen < Duration::from_millis(22) {
                self.period = self.period.mul_f64(0.95) + seen.mul_f64(0.05);
            }
        }
        self.works[self.next] = work;
        self.next = (self.next + 1) % WORK_WINDOW;
        self.work = self.works.iter().copied().max().unwrap_or(work);
        self.last_return = Some(now);
        missed
    }

    /// After standing by, the last swap is long past: start pacing afresh rather than
    /// counting the whole doze as one missed frame.
    fn resume(&mut self) {
        self.last_return = None;
    }
}

pub fn run() {
    eprintln!("slot: {}", slot::build_info::Build::current().describe());
    slot::boot_time::mark("start");
    let _ = std::thread::Builder::new()
        .name("slot-fonts".into())
        .spawn(slot_ui::warm_fonts);
    let audio = std::thread::Builder::new()
        .name("slot-audio-open".into())
        .spawn(slot::audio::opened_sink);
    let root = std::env::var_os("SLOT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(CARD));
    let mut surface = match FbdevSurface::new() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("slot: {e}");
            return;
        }
    };
    slot::boot_time::mark("surface");
    let mut compositor = match Compositor::new(&surface) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("slot: {e}");
            return;
        }
    };
    slot::boot_time::mark("compositor");
    let panel = surface.window_size();
    let turned = panel.0 < panel.1;
    let window = if turned { (panel.1, panel.0) } else { panel };
    let ccw = std::env::var("SLOT_ROTATE")
        .map(|v| v.trim().eq_ignore_ascii_case("ccw"))
        .unwrap_or(false);
    eprintln!(
        "slot: panel {}x{}, canvas {}x{}, turned {turned}, ccw {ccw}",
        panel.0, panel.1, window.0, window.1
    );
    if turned {
        if let Err(e) = compositor.set_panel_turn(panel, ccw) {
            eprintln!("slot: {e}");
            return;
        }
    }
    let mut platform = DevicePlatform::new(root.clone());
    eprintln!("slot: {}", platform.report());
    // Hardware sleep first and on its own: the optional quieting below can never stop it.
    platform.arm_sleep();
    let keep_wifi = slot_store::read_slot_state(&root).wifi;
    quiet_hardware(Quiet::device(keep_wifi));
    // Wi-Fi On (0.5.0): the driver stays loaded (above) and saved networks are joined in
    // the background. Wi-Fi Off starts nothing here.
    let net = keep_wifi.then(|| slot_net::NetJobs::spawn(slot_net::wifi::device(&root)));
    platform.trace_boot();
    slot::boot_time::mark("platform");
    let sink = match audio.map(|h| h.join()) {
        Ok(Ok(sink)) => sink,
        _ => slot::audio::opened_sink(),
    };
    slot::boot_time::mark("audio");
    let mut frontend = Frontend::boot_with(Box::new(platform), sink);
    if let Some(jobs) = net {
        slot_net::set_global(&jobs);
        frontend.set_net(Box::new(slot::net::WifiOn::new(jobs)));
    }
    slot::boot_time::mark("frontend");
    frontend.upload_boot_faces(&mut compositor);
    slot::boot_time::mark("faces");
    frontend.upload_bezel(&mut compositor, window);
    slot::boot_time::mark("bezel");
    let mut input = DeviceInput::open(&root);
    slot::boot_time::mark("input");
    let card = root.clone();
    let _ = std::thread::Builder::new()
        .name("slot-bootlogo".into())
        .spawn(move || slot::bootlogo::refresh(&card));
    let mut drawn = false;
    frontend.drive_emulator();
    let mut pacer = Pacer::new();
    let (mut frames, mut missed, mut early) = (0u32, 0u32, 0u32);
    let mut standby = Standby::new();
    loop {
        if standby.step(frontend.standing_by()) == Step::Rest {
            // Dozing behind a dark panel: no emulator tick, no render, no swap. Input, the
            // lid poll and the app's timers (the doze timeout among them) still run.
            std::thread::sleep(STANDBY_REST);
            frontend.advance(&mut input);
            if frontend.restarting() {
                frontend.restart();
            }
            if frontend.powering_off() {
                frontend.poweroff();
                return;
            }
            if frontend.sleep_due() {
                // Suspend to RAM. Returns once awake; the app has already decided whether
                // to carry on, sleep again or shut down.
                frontend.sleep_now();
                input.after_wake();
                pacer.resume();
                continue;
            }
            if !frontend.standing_by() {
                pacer.resume();
            }
            continue;
        }
        pacer.wait();
        let began = Instant::now();
        frontend.advance(&mut input);
        if frontend.restarting() {
            frontend.restart();
        }
        if frontend.powering_off() {
            frontend.poweroff();
            return;
        }
        frontend.step_emulator(pacer.period, STEP_TIMEOUT);
        frontend.render(&mut compositor, window);
        let swap = Instant::now();
        let work = swap - began;
        if let Err(e) = surface.swap() {
            eprintln!("slot: {e}");
            return;
        }
        let swap_took = swap.elapsed();
        slot::latency::swapped(swap_took.as_secs_f64() * 1000.0);
        let dropped = pacer.swapped(work, swap_took > Duration::from_millis(1));
        if slot::latency::tracing() {
            frames += 1;
            missed += u32::from(dropped);
            early += u32::from(swap_took > pacer.period / 2);
            if frames == 600 {
                eprintln!(
                    "slot: pace: 600 frames, {missed} missed a latch, {early} drawn early, work {:.1} ms, period {:.2} ms",
                    pacer.work.as_secs_f64() * 1e3,
                    pacer.period.as_secs_f64() * 1e3
                );
                (frames, missed, early) = (0, 0, 0);
            }
        }
        if !drawn {
            drawn = true;
            slot::boot_time::mark("first frame");
            trace_first_frame();
        }
        if let Some(left) = MIN_FRAME.checked_sub(began.elapsed()) {
            std::thread::sleep(left);
        }
    }
}
