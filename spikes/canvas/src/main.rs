//! Canvas architecture spike for the LibrePCB Rust + Slint rewrite (throwaway).
//!
//! Modes:
//!   transform  A1: Slint `Path` elements (batched per layer/style/tile), pan/zoom via parent transform
//!   viewbox    A2: Slint `Path` elements, pan/zoom via per-path viewbox (works on the software renderer)
//!   tiny-skia  baseline: tiny-skia -> SharedPixelBuffer -> Image (current C++ architecture)
//!   vello-cpu  B fallback: vello_cpu -> SharedPixelBuffer -> Image
//!   gpu29      B: vello 0.10 -> wgpu 29 texture on Slint's device (needs SLINT_BACKEND=winit-skia)
//!   gpu30      B: vello git -> wgpu 30 texture on Slint's device (winit-femtovg-wgpu / winit-skia)

mod board;
mod gpu;
mod raster;
mod scene;
mod sexp;

use anyhow::{Context, Result, bail};
use gpu::GpuCanvas;
use kurbo::{BezPath, PathEl, Point, Vec2};
use raster::Raster;
use scene::{Geom, HIGHLIGHT, Scene, Style, View, layer_color};
use slint::{ComponentHandle, Model, ModelRc, SharedPixelBuffer, VecModel};
use std::cell::RefCell;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

slint::include_modules!();

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Transform,
    Viewbox,
    TinySkia,
    VelloCpu,
    Gpu29,
    Gpu30,
}

impl Mode {
    fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "transform" => Mode::Transform,
            "viewbox" => Mode::Viewbox,
            "tiny-skia" => Mode::TinySkia,
            "vello-cpu" => Mode::VelloCpu,
            "gpu29" => Mode::Gpu29,
            "gpu30" => Mode::Gpu30,
            _ => bail!("unknown mode {s}"),
        })
    }
    fn slint_mode(self) -> i32 {
        match self {
            Mode::Transform => 0,
            Mode::Viewbox => 1,
            _ => 2,
        }
    }
}

struct Args {
    scene: String,
    mode: Mode,
    tiles: usize,
    bench: bool,
    iters: usize,
    out_dir: PathBuf,
    animate: Option<f64>,
    cache_hint: bool,
    cull: bool,
    traces: usize,
    pads: usize,
    vias: usize,
    threads: u16,
    raster: Option<String>,
    png: Option<PathBuf>,
    check_boards: bool,
}

fn parse_args() -> Result<Args> {
    let mut a = Args {
        scene: "gerber".into(),
        mode: Mode::Transform,
        tiles: 8,
        bench: false,
        iters: 30,
        out_dir: PathBuf::from("out"),
        animate: None,
        cache_hint: false,
        cull: true,
        traces: 100_000,
        pads: 16_000,
        vias: 4_000,
        threads: 0,
        raster: None,
        png: None,
        check_boards: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut v = || it.next().context("missing value");
        match k.as_str() {
            "--scene" => a.scene = v()?,
            "--mode" => a.mode = Mode::parse(&v()?)?,
            "--tiles" => a.tiles = v()?.parse()?,
            "--bench" => a.bench = true,
            "--iters" => a.iters = v()?.parse()?,
            "--out-dir" => a.out_dir = v()?.into(),
            "--animate" => a.animate = Some(v()?.parse()?),
            "--cache-hint" => a.cache_hint = true,
            "--no-cull" => a.cull = false,
            "--traces" => a.traces = v()?.parse()?,
            "--pads" => a.pads = v()?.parse()?,
            "--vias" => a.vias = v()?.parse()?,
            "--threads" => a.threads = v()?.parse()?,
            "--raster" => a.raster = Some(v()?),
            "--png" => a.png = Some(v()?.into()),
            "--check-boards" => a.check_boards = true,
            _ => bail!("unknown arg {k}"),
        }
    }
    Ok(a)
}

fn test_data() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../LibrePCB/tests/data")
}

fn load_scene(a: &Args) -> Result<Scene> {
    let items = match a.scene.as_str() {
        "stress" => scene::stress(a.traces, a.pads, a.vias),
        "gerber" => board::load_board(&test_data().join("projects/Gerber Test/boards/default/board.lp"))?,
        p => board::load_board(Path::new(p))?,
    };
    Ok(Scene::new(a.scene.clone(), items, a.tiles))
}

fn rss() -> String {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let get = |k: &str| {
        s.lines()
            .find(|l| l.starts_with(k))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0)
            / 1024
    };
    format!("RSS {} MiB (peak {} MiB)", get("VmRSS:"), get("VmHWM:"))
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn stats(v: &mut [f64]) -> String {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mean = v.iter().sum::<f64>() / v.len() as f64;
    format!("mean {:7.2} ms  p50 {:7.2}  max {:7.2}", mean, v[v.len() / 2], v[v.len() - 1])
}

// ---------------------------------------------------------------------------
// Slint path models
// ---------------------------------------------------------------------------

fn slint_color(argb: u32) -> slint::Color {
    slint::Color::from_argb_u8((argb >> 24) as u8, (argb >> 16) as u8, (argb >> 8) as u8, argb as u8)
}

/// SVG path commands in integer micrometres relative to `origin` (mm).
fn svg(path: &BezPath, origin: Point, out: &mut String) {
    let um = |p: Point| (((p.x - origin.x) * 1000.0).round() as i64, ((p.y - origin.y) * 1000.0).round() as i64);
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                let (x, y) = um(p);
                let _ = write!(out, "M{x} {y}");
            }
            PathEl::LineTo(p) => {
                let (x, y) = um(p);
                let _ = write!(out, "L{x} {y}");
            }
            PathEl::QuadTo(a, p) => {
                let (ax, ay) = um(a);
                let (x, y) = um(p);
                let _ = write!(out, "Q{ax} {ay} {x} {y}");
            }
            PathEl::CurveTo(a, b, p) => {
                let (ax, ay) = um(a);
                let (bx, by) = um(b);
                let (x, y) = um(p);
                let _ = write!(out, "C{ax} {ay} {bx} {by} {x} {y}");
            }
            PathEl::ClosePath => out.push('Z'),
        }
    }
}

fn path_group(path: &BezPath, bbox: kurbo::Rect, style: Style, argb: u32) -> PathGroup {
    let mut commands = String::new();
    svg(path, bbox.origin(), &mut commands);
    let transparent = slint::Color::from_argb_u8(0, 0, 0, 0);
    let (fill, stroke, sw) = match style {
        Style::Fill => (slint_color(argb), transparent, 0.0),
        Style::Stroke(nm) => (transparent, slint_color(argb), nm as f32 / 1000.0),
    };
    PathGroup {
        x: (bbox.x0 * 1000.0) as f32,
        y: (bbox.y0 * 1000.0) as f32,
        w: (bbox.width() * 1000.0) as f32,
        h: (bbox.height() * 1000.0) as f32,
        commands: commands.into(),
        fill,
        stroke,
        stroke_width: sw,
    }
}

fn group_model(scene: &Scene, gi: usize) -> PathGroup {
    let g = &scene.groups[gi];
    path_group(&g.path(&scene.items), g.bbox, g.style, layer_color(g.layer))
}

fn highlight_model(scene: &Scene, idx: usize) -> Vec<PathGroup> {
    let it = &scene.items[idx];
    let mut p = BezPath::new();
    scene::append_item_path(&mut p, it, Style::Fill);
    let bb = it.bbox.inflate(0.05, 0.05);
    match &it.geom {
        Geom::Seg { w, .. } => vec![path_group(&p, bb, Style::Stroke((w * 1e6) as i64), HIGHLIGHT)],
        Geom::Area { fill: true, .. } => vec![path_group(&p, bb, Style::Fill, HIGHLIGHT)],
        Geom::Area { w, .. } => vec![path_group(&p, bb, Style::Stroke((w.max(0.05) * 1e6) as i64), HIGHLIGHT)],
    }
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

struct App {
    scene: Scene,
    mode: Mode,
    view: View,
    cull: bool,
    groups: Rc<VecModel<PathGroup>>,
    raster: Option<Box<dyn Raster>>,
    gpu: Option<Box<dyn GpuCanvas>>,
    drag: Option<(f64, f64, f64, f64, bool)>,
    last_frame: f64,
    selected: Option<String>,
    raster_ms: f64,
}

impl App {
    fn physical(&self, ui: &CanvasWindow) -> (u32, u32, f64) {
        let s = ui.window().size();
        (s.width.max(1), s.height.max(1), ui.window().scale_factor() as f64)
    }

    fn refresh(&mut self, ui: &CanvasWindow) {
        ui.set_zoom((self.view.zoom / 1000.0) as f32);
        ui.set_pan_x(self.view.pan.x as f32);
        ui.set_pan_y(self.view.pan.y as f32);
        let (w, h, sf) = self.physical(ui);
        let t = Instant::now();
        if let Some(r) = self.raster.as_mut() {
            let mut buf = SharedPixelBuffer::<slint::Rgba8Pixel>::new(w, h);
            r.render(&self.view, w, h, sf, buf.make_mut_bytes());
            ui.set_raster(slint::Image::from_rgba8_premultiplied(buf));
        }
        if let Some(g) = self.gpu.as_mut() {
            match g.render(&self.view, w, h, sf, self.cull).and_then(|_| g.image()) {
                Ok(img) => {
                    // Measurement only: include GPU completion in raster_ms.
                    g.wait();
                    ui.set_raster(img)
                }
                Err(e) => eprintln!("gpu render failed: {e:#}"),
            }
        }
        self.raster_ms = ms(t.elapsed());
        ui.set_status(
            format!(
                "{} | mode {:?} | {} groups | zoom {:.2} px/mm | raster/encode {:.1} ms | {}",
                self.scene.name,
                self.mode,
                self.scene.groups.len(),
                self.view.zoom,
                self.raster_ms,
                self.selected.as_deref().unwrap_or("click an item")
            )
            .into(),
        );
    }

    fn click(&mut self, ui: &CanvasWindow, x: f64, y: f64) {
        let p = self.view.to_world(x, y);
        let tol = 2.0 / self.view.zoom;
        let t = Instant::now();
        let hit = self.scene.hit_test(p, tol);
        let dt = t.elapsed();
        match hit {
            Some(i) => {
                let it = &self.scene.items[i];
                let msg = format!("{:?} on {}: {}", it.kind, scene::LAYERS[it.layer as usize].0, it.label);
                println!("hit ({:.3}, {:.3}) mm -> {msg}  [{:.1} us]", p.x, -p.y, dt.as_secs_f64() * 1e6);
                ui.set_overlay(ModelRc::new(VecModel::from(highlight_model(&self.scene, i))));
                self.selected = Some(msg);
            }
            None => {
                println!("hit ({:.3}, {:.3}) mm -> nothing  [{:.1} us]", p.x, -p.y, dt.as_secs_f64() * 1e6);
                ui.set_overlay(ModelRc::new(VecModel::<PathGroup>::default()));
                self.selected = None;
            }
        }
        self.refresh(ui);
    }
}

fn save_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> Result<()> {
    let f = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut enc = png::Encoder::new(f, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(rgba)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Headless raster mode (no Slint): correctness PNGs + raw renderer timings
// ---------------------------------------------------------------------------

fn headless(a: &Args, scene: &Scene, backend: &str) -> Result<()> {
    let (w, h) = (1600u32, 1000u32);
    let full = View::fit(scene.bbox, w as f64, h as f64);
    let views = bench_views(full, w as f64, h as f64);
    // Hit-test cost: R-tree candidates + precise kurbo test, 10k random points.
    {
        let mut seed = 0x1234_5678_9abc_def1u64;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let n = 10_000;
        let t = Instant::now();
        let mut hits = 0;
        for _ in 0..n {
            let p = Point::new(
                scene.bbox.x0 + rnd() * scene.bbox.width(),
                scene.bbox.y0 + rnd() * scene.bbox.height(),
            );
            if scene.hit_test(p, 2.0 / full.zoom).is_some() {
                hits += 1;
            }
        }
        println!(
            "hit test: {:.2} us/query ({hits}/{n} hits, tol 2px @ full view)",
            t.elapsed().as_secs_f64() * 1e6 / n as f64
        );
        // A few known points of the Gerber Test board (file coordinates, y up).
        if a.scene == "gerber" {
            for (x, y) in [(20.0, 42.0), (90.0, 25.0), (21.0, 39.1), (22.0, 53.5), (50.0, 10.0)] {
                let hit = scene.hit_test(Point::new(x, -y), 0.05);
                println!(
                    "  hit ({x}, {y}) -> {}",
                    hit.map(|i| format!("{:?} {}", scene.items[i].kind, scene.items[i].label))
                        .unwrap_or("nothing".into())
                );
            }
        }
    }
    let mut buf = vec![0u8; (w * h * 4) as usize];
    let mut cpu: Option<Box<dyn Raster>> = None;
    let mut gpu: Option<Box<dyn GpuCanvas>> = None;
    let t = Instant::now();
    match backend {
        "tiny-skia" => cpu = Some(Box::new(raster::TinySkia::default())),
        "vello-cpu" => cpu = Some(Box::new(raster::VelloCpu::new(a.threads))),
        "vello-gpu29" => gpu = Some(Box::new(gpu::v29::Gpu::headless()?)),
        "vello-gpu30" => gpu = Some(Box::new(gpu::v30::Gpu::headless()?)),
        _ => bail!("unknown raster backend {backend}"),
    }
    if let Some(c) = cpu.as_mut() {
        c.build(scene);
    }
    if let Some(g) = gpu.as_mut() {
        g.build(scene);
    }
    println!("[{backend}] build {:.1} ms, {}", ms(t.elapsed()), rss());
    for (name, view) in &views {
        let mut times = Vec::new();
        for i in 0..=a.iters {
            let mut v = *view;
            v.pan.x += (i % 2) as f64;
            let t = Instant::now();
            if let Some(c) = cpu.as_mut() {
                c.render(&v, w, h, 1.0, &mut buf);
            }
            if let Some(g) = gpu.as_mut() {
                g.render(&v, w, h, 1.0, a.cull)?;
                g.wait();
            }
            if i > 0 {
                times.push(ms(t.elapsed()));
            }
        }
        println!("[{backend}] {:<7} {}", name, stats(&mut times));
        if let Some(png) = &a.png {
            let out = png.with_file_name(format!(
                "{}-{name}.{}",
                png.file_stem().unwrap().to_string_lossy(),
                png.extension().map(|e| e.to_string_lossy()).unwrap_or("png".into())
            ));
            if let Some(g) = gpu.as_mut() {
                g.render(view, w, h, 1.0, a.cull)?;
                let (_, _, px) = g.readback()?;
                save_png(&out, w, h, &px)?;
            } else {
                cpu.as_mut().unwrap().render(view, w, h, 1.0, &mut buf);
                save_png(&out, w, h, &buf)?;
            }
            println!("  wrote {}", out.display());
        }
    }
    println!("[{backend}] {}", rss());
    Ok(())
}

fn bench_views(full: View, w: f64, h: f64) -> Vec<(&'static str, View)> {
    let mut z8 = full;
    z8.zoom_at(w / 2.0, h / 2.0, 8.0);
    let mut z40 = full;
    z40.zoom_at(w / 2.0, h / 2.0, 40.0);
    vec![("full", full), ("zoom8", z8), ("zoom40", z40)]
}

// ---------------------------------------------------------------------------
// In-window benchmark: every frame is rendered synchronously via take_snapshot()
// (not vsync-limited; includes GPU completion + a readback of the frame).
// ---------------------------------------------------------------------------

fn frame(app: &Rc<RefCell<App>>, ui: &CanvasWindow) -> Result<(f64, SharedPixelBuffer<slint::Rgba8Pixel>)> {
    let t = Instant::now();
    app.borrow_mut().refresh(ui);
    let snap = ui.window().take_snapshot()?;
    Ok((ms(t.elapsed()), snap))
}

fn run_bench(app: &Rc<RefCell<App>>, ui: &CanvasWindow, a: &Args, tag: &str) -> Result<()> {
    std::fs::create_dir_all(&a.out_dir)?;
    let (w, h, sf) = app.borrow().physical(ui);
    let (lw, lh) = (w as f64 / sf, h as f64 / sf);
    let full = View::fit(app.borrow().scene.bbox, lw, lh);
    println!("[{tag}] window {w}x{h} px, scale {sf}, {}", rss());
    {
        app.borrow_mut().view = full;
        let mut stat = Vec::new();
        for _ in 0..a.iters {
            let t = Instant::now();
            ui.window().take_snapshot()?;
            stat.push(ms(t.elapsed()));
        }
        println!("[{tag}] static (no change, full view): {}", stats(&mut stat));
    }
    for (name, view) in bench_views(full, lw, lh) {
        app.borrow_mut().view = view;
        let (first, snap) = frame(app, ui)?;
        save_png(
            &a.out_dir.join(format!("{tag}-{name}.png")),
            snap.width(),
            snap.height(),
            snap.as_bytes(),
        )?;
        let mut pan = Vec::new();
        let mut ours = Vec::new();
        for i in 0..a.iters {
            app.borrow_mut().view.pan += Vec2::new(if i % 2 == 0 { 3.0 } else { -3.0 }, 0.0);
            pan.push(frame(app, ui)?.0);
            ours.push(app.borrow().raster_ms);
        }
        let mut zoom = Vec::new();
        for i in 0..a.iters {
            let f = if i % 2 == 0 { 1.05 } else { 1.0 / 1.05 };
            app.borrow_mut().view.zoom_at(lw / 2.0, lh / 2.0, f);
            zoom.push(frame(app, ui)?.0);
        }
        println!(
            "[{tag}] {name:<7} first {first:7.2} ms | pan: {} | zoom: {} | ours(pan): {}",
            stats(&mut pan),
            stats(&mut zoom),
            stats(&mut ours)
        );
    }
    println!("[{tag}] after frames: {}", rss());

    // Incremental edit: move the largest stroke group's first item and rebuild that group only.
    let gi = {
        let app = app.borrow();
        (0..app.scene.groups.len())
            .filter(|&i| matches!(app.scene.groups[i].style, Style::Stroke(_)))
            .max_by_key(|&i| app.scene.groups[i].items.len())
    };
    if let Some(gi) = gi {
        app.borrow_mut().view = full;
        let mut rebuild = Vec::new();
        let mut frames = Vec::new();
        for i in 0..a.iters.min(20) {
            let t = Instant::now();
            {
                let mut ab = app.borrow_mut();
                let idx = ab.scene.groups[gi].items[0];
                if let Geom::Seg { b, .. } = &mut ab.scene.items[idx].geom {
                    b.x += if i % 2 == 0 { 0.5 } else { -0.5 };
                }
                match ab.mode {
                    Mode::Transform | Mode::Viewbox => {
                        let m = group_model(&ab.scene, gi);
                        ab.groups.set_row_data(gi, m);
                    }
                    Mode::TinySkia | Mode::VelloCpu => {
                        let App { raster, scene, .. } = &mut *ab;
                        raster.as_mut().unwrap().build(scene);
                    }
                    Mode::Gpu29 | Mode::Gpu30 => {
                        let App { gpu, scene, .. } = &mut *ab;
                        gpu.as_mut().unwrap().rebuild_group(scene, gi);
                    }
                }
            }
            rebuild.push(ms(t.elapsed()));
            frames.push(frame(app, ui)?.0);
        }
        let n = app.borrow().scene.groups[gi].items.len();
        println!(
            "[{tag}] edit (group of {n} items): rebuild {} | next frame {}",
            stats(&mut rebuild),
            stats(&mut frames)
        );
    }

    // Baseline: empty scene snapshot cost (renderer + readback overhead).
    {
        let mut ab = app.borrow_mut();
        ab.raster = None;
        ab.gpu = None;
    }
    ui.set_groups(ModelRc::new(VecModel::<PathGroup>::default()));
    ui.set_overlay(ModelRc::new(VecModel::<PathGroup>::default()));
    ui.set_mode(0);
    let mut empty = Vec::new();
    for _ in 0..a.iters {
        app.borrow_mut().view.pan.x += 1.0;
        empty.push(frame(app, ui)?.0);
    }
    println!("[{tag}] empty-scene snapshot overhead: {}", stats(&mut empty));
    Ok(())
}

fn main() -> Result<()> {
    let a = parse_args()?;

    if a.check_boards {
        for entry in walk(&test_data()) {
            if entry.file_name().and_then(|n| n.to_str()) == Some("board.lp") {
                match board::load_board(&entry) {
                    Ok(items) => println!("OK  {:5} items  {}", items.len(), entry.display()),
                    Err(e) => println!("ERR {}: {e:#}", entry.display()),
                }
            }
        }
        return Ok(());
    }

    let t = Instant::now();
    let scene = load_scene(&a)?;
    println!("load+group {:.1} ms: {} | {}", ms(t.elapsed()), scene.stats(), rss());

    if let Some(backend) = &a.raster {
        return headless(&a, &scene, backend);
    }

    let tag = format!(
        "{}-{}-{:?}{}",
        scene.name.replace(['/', ' '], "_"),
        std::env::var("SLINT_BACKEND").unwrap_or_else(|_| "default".into()),
        a.mode,
        if a.cache_hint { "-cachehint" } else { "" }
    )
    .to_lowercase();

    // Pitfall: Slint's default WGPUSettings request `Limits::downlevel_webgl2_defaults()`
    // (no storage buffers in compute), so vello's compute pipelines fail validation on
    // Slint's device. Request default (WebGPU) limits instead.
    match a.mode {
        Mode::Gpu29 => {
            let mut s = slint::wgpu_29::WGPUSettings::default();
            s.device_required_limits = slint::wgpu_29::wgpu::Limits::default();
            slint::BackendSelector::new()
                .require_wgpu_29(slint::wgpu_29::WGPUConfiguration::Automatic(s))
                .select()?
        }
        Mode::Gpu30 => {
            let mut s = slint::wgpu_30::WGPUSettings::default();
            s.device_required_limits = slint::wgpu_30::wgpu::Limits::default();
            slint::BackendSelector::new()
                .require_wgpu_30(slint::wgpu_30::WGPUConfiguration::Automatic(s))
                .select()?
        }
        _ => {}
    }

    let ui = CanvasWindow::new()?;
    ui.set_mode(a.mode.slint_mode());
    ui.set_cache_hint(a.cache_hint);

    // Build the Slint path model (candidate A) – also used for timing numbers.
    let t = Instant::now();
    let groups: Vec<PathGroup> = if matches!(a.mode, Mode::Transform | Mode::Viewbox) {
        (0..scene.groups.len()).map(|gi| group_model(&scene, gi)).collect()
    } else {
        Vec::new()
    };
    let bytes: usize = groups.iter().map(|g| g.commands.len()).sum();
    let groups = Rc::new(VecModel::from(groups));
    ui.set_groups(ModelRc::from(groups.clone()));
    println!(
        "slint model: {} paths, {:.1} MiB of path commands, built in {:.1} ms | {}",
        groups.row_count(),
        bytes as f64 / (1 << 20) as f64,
        ms(t.elapsed()),
        rss()
    );

    let mut raster: Option<Box<dyn Raster>> = match a.mode {
        Mode::TinySkia => Some(Box::new(raster::TinySkia::default())),
        Mode::VelloCpu => Some(Box::new(raster::VelloCpu::new(a.threads))),
        _ => None,
    };
    if let Some(r) = raster.as_mut() {
        let t = Instant::now();
        r.build(&scene);
        println!("raster build {:.1} ms", ms(t.elapsed()));
    }

    let app = Rc::new(RefCell::new(App {
        scene,
        mode: a.mode,
        view: View { zoom: 1.0, pan: Vec2::ZERO },
        cull: a.cull,
        groups,
        raster,
        gpu: None,
        drag: None,
        last_frame: 0.0,
        selected: None,
        raster_ms: 0.0,
    }));

    // Frame stats from the rendering notifier: CPU time between BeforeRendering and
    // AfterRendering, and the period between frames (vsync-limited on screen).
    let fstats: Rc<RefCell<FrameStats>> = Rc::default();
    {
        let app2 = app.clone();
        let weak = ui.as_weak();
        let mode = a.mode;
        let fs = fstats.clone();
        let res = ui.window().set_rendering_notifier(move |state, api| {
            match state {
                slint::RenderingState::BeforeRendering => fs.borrow_mut().before = Some(Instant::now()),
                slint::RenderingState::AfterRendering => {
                    let mut f = fs.borrow_mut();
                    let now = Instant::now();
                    if let Some(b) = f.before.take() {
                        f.render.push(ms(now - b));
                    }
                    if let Some(l) = f.last_after.replace(now) {
                        f.period.push(ms(now - l));
                    }
                }
                _ => {}
            }
            if !matches!(mode, Mode::Gpu29 | Mode::Gpu30) {
                return;
            }
            if let slint::RenderingState::RenderingSetup = state {
                let t = Instant::now();
                let gpu: Option<Result<Box<dyn GpuCanvas>>> = match mode {
                    Mode::Gpu29 => gpu::v29::from_graphics_api(api).map(|r| r.map(|g| Box::new(g) as _)),
                    _ => gpu::v30::from_graphics_api(api).map(|r| r.map(|g| Box::new(g) as _)),
                };
                match gpu {
                    Some(Ok(mut g)) => {
                        g.build(&app2.borrow().scene);
                        println!("gpu: vello on Slint's device ready in {:.1} ms | {}", ms(t.elapsed()), rss());
                        app2.borrow_mut().gpu = Some(g);
                        let (app3, weak) = (app2.clone(), weak.clone());
                        slint::Timer::single_shot(Duration::ZERO, move || {
                            if let Some(ui) = weak.upgrade() {
                                app3.borrow_mut().refresh(&ui);
                            }
                        });
                    }
                    Some(Err(e)) => eprintln!("gpu init failed: {e:#}"),
                    None => eprintln!("rendering setup: unexpected graphics API {api:?}"),
                }
            }
        });
        if let Err(e) = res {
            if matches!(a.mode, Mode::Gpu29 | Mode::Gpu30) {
                bail!("set_rendering_notifier failed: {e:?} (this Slint renderer can't share its wgpu device)");
            }
            eprintln!("note: rendering notifier unsupported by this renderer ({e:?}); no per-frame CPU stats");
        }
    }

    // Interaction.
    {
        let (app2, weak) = (app.clone(), ui.as_weak());
        ui.on_press(move |x, y| {
            let _ = &weak;
            app2.borrow_mut().drag = Some((x as f64, y as f64, x as f64, y as f64, false));
        });
    }
    {
        let (app2, weak) = (app.clone(), ui.as_weak());
        ui.on_drag(move |x, y| {
            let ui = weak.unwrap();
            let mut ab = app2.borrow_mut();
            if let Some((sx, sy, lx, ly, moved)) = ab.drag {
                let (x, y) = (x as f64, y as f64);
                ab.view.pan += Vec2::new(x - lx, y - ly);
                let moved = moved || (x - sx).hypot(y - sy) > 3.0;
                ab.drag = Some((sx, sy, x, y, moved));
                ab.refresh(&ui);
            }
        });
    }
    {
        let (app2, weak) = (app.clone(), ui.as_weak());
        ui.on_release(move |x, y| {
            let ui = weak.unwrap();
            let mut ab = app2.borrow_mut();
            if let Some((_, _, _, _, false)) = ab.drag.take() {
                ab.click(&ui, x as f64, y as f64);
            }
        });
    }
    {
        let (app2, weak) = (app.clone(), ui.as_weak());
        ui.on_wheel(move |x, y, dy| {
            let ui = weak.unwrap();
            let mut ab = app2.borrow_mut();
            ab.view.zoom_at(x as f64, y as f64, 1.0015f64.powf(dy as f64));
            ab.refresh(&ui);
        });
    }

    // Fit view once the window has its real size.
    {
        let (app2, weak) = (app.clone(), ui.as_weak());
        slint::Timer::single_shot(Duration::from_millis(50), move || {
            let ui = weak.unwrap();
            let mut ab = app2.borrow_mut();
            let (w, h, sf) = ab.physical(&ui);
            ab.view = View::fit(ab.scene.bbox, w as f64 / sf, h as f64 / sf);
            ab.refresh(&ui);
        });
    }

    let bench_timer = slint::Timer::default();
    if a.bench {
        let (app2, weak) = (app.clone(), ui.as_weak());
        let a2 = Args { raster: None, png: None, ..a_clone(&a) };
        let tag2 = tag.clone();
        bench_timer.start(slint::TimerMode::SingleShot, Duration::from_millis(1500), move || {
            let ui = weak.unwrap();
            if matches!(a2.mode, Mode::Gpu29 | Mode::Gpu30) && app2.borrow().gpu.is_none() {
                eprintln!("[{tag2}] gpu canvas not initialised, aborting bench");
                let _ = slint::quit_event_loop();
                return;
            }
            if let Err(e) = run_bench(&app2, &ui, &a2, &tag2) {
                eprintln!("[{tag2}] bench failed: {e:#}");
            }
            let _ = slint::quit_event_loop();
        });
    }

    let anim_timer = slint::Timer::default();
    if let Some(secs) = a.animate {
        // Phases: full view, 8x, 40x; in each the view pans continuously.
        // Every view update waits until the previous one was presented (if the
        // renderer supports the notifier), so "period" is the real on-screen frame time.
        let (app2, weak) = (app.clone(), ui.as_weak());
        let fs = fstats.clone();
        let tag2 = tag.clone();
        let start = Instant::now();
        let mut phase_idx = usize::MAX;
        let mut seen = usize::MAX;
        let mut updates = 0usize;
        let mut ours: Vec<f64> = Vec::new();
        anim_timer.start(slint::TimerMode::Repeated, Duration::from_millis(1), move || {
            let ui = weak.unwrap();
            let el = start.elapsed().as_secs_f64();
            let warm = 1.0;
            let idx = if el < warm { usize::MAX } else { ((el - warm) / secs) as usize };
            if idx != phase_idx {
                if phase_idx != usize::MAX {
                    let mut f = fs.borrow_mut();
                    let name = ["full", "zoom8", "zoom40"][phase_idx];
                    // drop the first frame of a phase (view jump)
                    let skip = |v: &mut Vec<f64>| if v.len() > 2 { v.drain(..1); };
                    skip(&mut f.period);
                    skip(&mut f.render);
                    println!(
                        "[{tag2}] animate {name:<7} {} updates in {secs}s | frame period {} | render CPU {} | our raster/encode {}",
                        updates,
                        if f.period.is_empty() { "n/a".into() } else { stats(&mut f.period) },
                        if f.render.is_empty() { "n/a".into() } else { stats(&mut f.render) },
                        if ours.is_empty() { "n/a".into() } else { stats(&mut ours) },
                    );
                }
                let mut f = fs.borrow_mut();
                f.frames_base += f.period.len();
                f.period.clear();
                f.render.clear();
                ours.clear();
                updates = 0;
                seen = usize::MAX;
                phase_idx = idx;
                if idx != usize::MAX && idx >= 3 {
                    let _ = slint::quit_event_loop();
                    return;
                }
            }
            if idx == usize::MAX {
                return;
            }
            let frames = fs.borrow().period.len() + fs.borrow().frames_base;
            if frames == seen && fs.borrow().last_after.is_some() {
                return;
            }
            seen = frames;
            let mut ab = app2.borrow_mut();
            let (w, h, sf) = ab.physical(&ui);
            let (lw, lh) = (w as f64 / sf, h as f64 / sf);
            let full = View::fit(ab.scene.bbox, lw, lh);
            let mut v = bench_views(full, lw, lh)[idx].1;
            let t = el - warm;
            v.pan += Vec2::new((t * 2.0).sin() * 60.0, (t * 1.5).cos() * 40.0);
            ab.view = v;
            ab.refresh(&ui);
            ours.push(ab.raster_ms);
            updates += 1;
        });
    }

    ui.run()?;
    Ok(())
}

#[derive(Default)]
struct FrameStats {
    before: Option<Instant>,
    last_after: Option<Instant>,
    render: Vec<f64>,
    period: Vec<f64>,
    frames_base: usize,
}

fn a_clone(a: &Args) -> Args {
    Args {
        scene: a.scene.clone(),
        mode: a.mode,
        tiles: a.tiles,
        bench: a.bench,
        iters: a.iters,
        out_dir: a.out_dir.clone(),
        animate: a.animate,
        cache_hint: a.cache_hint,
        cull: a.cull,
        traces: a.traces,
        pads: a.pads,
        vias: a.vias,
        threads: a.threads,
        raster: a.raster.clone(),
        png: a.png.clone(),
        check_boards: a.check_boards,
    }
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(&p));
            } else {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}
