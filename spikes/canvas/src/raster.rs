//! CPU raster baselines: tiny-skia (current C++ architecture: paint into an
//! offscreen image, hand it to Slint) and vello_cpu (fallback for candidate B).

use crate::scene::{BACKGROUND, Scene, Style, View, layer_color};
use kurbo::{PathEl, Rect};

pub trait Raster {
    fn build(&mut self, scene: &Scene);
    /// Renders into an RGBA8 (premultiplied, opaque background) buffer.
    fn render(&mut self, view: &View, w: u32, h: u32, sf: f64, out: &mut [u8]);
}

fn rgba(argb: u32) -> (u8, u8, u8, u8) {
    ((argb >> 16) as u8, (argb >> 8) as u8, argb as u8, (argb >> 24) as u8)
}

#[derive(Default)]
pub struct TinySkia {
    groups: Vec<(Option<tiny_skia::Path>, u16, Style, f32, Rect)>,
}

impl Raster for TinySkia {
    fn build(&mut self, scene: &Scene) {
        self.groups = scene
            .groups
            .iter()
            .map(|g| {
                let mut pb = tiny_skia::PathBuilder::new();
                for el in g.path(&scene.items).elements() {
                    match *el {
                        PathEl::MoveTo(p) => pb.move_to(p.x as f32, p.y as f32),
                        PathEl::LineTo(p) => pb.line_to(p.x as f32, p.y as f32),
                        PathEl::QuadTo(a, p) => pb.quad_to(a.x as f32, a.y as f32, p.x as f32, p.y as f32),
                        PathEl::CurveTo(a, b, p) => pb.cubic_to(
                            a.x as f32, a.y as f32, b.x as f32, b.y as f32, p.x as f32, p.y as f32,
                        ),
                        PathEl::ClosePath => pb.close(),
                    }
                }
                (pb.finish(), g.layer, g.style, g.stroke_width() as f32, g.bbox)
            })
            .collect();
    }

    fn render(&mut self, view: &View, w: u32, h: u32, sf: f64, out: &mut [u8]) {
        let Some(mut pm) = tiny_skia::PixmapMut::from_bytes(out, w, h) else {
            return;
        };
        let (r, g, b, a) = rgba(BACKGROUND);
        pm.fill(tiny_skia::Color::from_rgba8(r, g, b, a));
        let c = view.affine(sf).as_coeffs();
        let tf = tiny_skia::Transform::from_row(
            c[0] as f32, c[1] as f32, c[2] as f32, c[3] as f32, c[4] as f32, c[5] as f32,
        );
        let visible = view.visible(w as f64 / sf, h as f64 / sf);
        for (path, layer, style, sw, bbox) in &self.groups {
            let Some(path) = path else { continue };
            if !bbox.overlaps(visible) {
                continue;
            }
            let (r, g, b, a) = rgba(layer_color(*layer));
            let mut paint = tiny_skia::Paint::default();
            paint.set_color_rgba8(r, g, b, a);
            paint.anti_alias = true;
            match style {
                Style::Fill => pm.fill_path(path, &paint, tiny_skia::FillRule::Winding, tf, None),
                Style::Stroke(_) => {
                    let stroke = tiny_skia::Stroke {
                        width: *sw,
                        line_cap: tiny_skia::LineCap::Round,
                        line_join: tiny_skia::LineJoin::Round,
                        ..Default::default()
                    };
                    pm.stroke_path(path, &paint, &stroke, tf, None);
                }
            }
        }
    }
}

pub struct VelloCpu {
    groups: Vec<(kurbo::BezPath, u16, Style, f64, Rect)>,
    ctx: Option<vello_cpu::RenderContext>,
    threads: u16,
}

impl VelloCpu {
    pub fn new(threads: u16) -> Self {
        Self {
            groups: Vec::new(),
            ctx: None,
            threads,
        }
    }
}

impl Raster for VelloCpu {
    fn build(&mut self, scene: &Scene) {
        self.groups = scene
            .groups
            .iter()
            .map(|g| (g.path(&scene.items), g.layer, g.style, g.stroke_width(), g.bbox))
            .collect();
    }

    fn render(&mut self, view: &View, w: u32, h: u32, sf: f64, out: &mut [u8]) {
        let (w16, h16) = (w as u16, h as u16);
        let threads = self.threads;
        let ctx = self.ctx.get_or_insert_with(|| {
            vello_cpu::RenderContext::new_with(
                w16,
                h16,
                vello_cpu::RenderSettings {
                    num_threads: threads,
                    ..Default::default()
                },
            )
        });
        if ctx.width() != w16 || ctx.height() != h16 {
            ctx.reset_and_resize(w16, h16);
        } else {
            ctx.reset();
        }
        let visible = view.visible(w as f64 / sf, h as f64 / sf);
        let (r, g, b, a) = rgba(BACKGROUND);
        ctx.set_paint(vello_cpu::color::AlphaColor::from_rgba8(r, g, b, a));
        ctx.fill_rect(&Rect::new(0.0, 0.0, w as f64, h as f64));
        ctx.set_transform(view.affine(sf));
        for (path, layer, style, sw, bbox) in &self.groups {
            if !bbox.overlaps(visible) {
                continue;
            }
            let (r, g, b, a) = rgba(layer_color(*layer));
            ctx.set_paint(vello_cpu::color::AlphaColor::from_rgba8(r, g, b, a));
            match style {
                Style::Fill => ctx.fill_path(path),
                Style::Stroke(_) => {
                    ctx.set_stroke(
                        kurbo::Stroke::new(*sw)
                            .with_caps(kurbo::Cap::Round)
                            .with_join(kurbo::Join::Round),
                    );
                    ctx.stroke_path(path);
                }
            }
        }
        ctx.reset_transform();
        ctx.flush();
        let Some(pm) = vello_cpu::PixmapMut::new(w16, h16, out) else {
            return;
        };
        let mut res = vello_cpu::Resources::new();
        ctx.render(pm, &mut res);
    }
}
