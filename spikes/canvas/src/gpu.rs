//! Candidate B: vello renders the board into a wgpu texture on Slint's own
//! device; the texture is handed to Slint as a `slint::Image` (zero copy).
//!
//! Instantiated twice: vello 0.10 (crates.io, wgpu 29, pairs with Slint's
//! skia-wgpu29 renderer) and vello git main (wgpu 30, pairs with Slint's
//! femtovg-wgpu and skia-wgpu30 renderers).

use crate::scene::{Scene, Style, View, layer_color};

pub trait GpuCanvas {
    /// Encode one retained vello fragment per group (tile batch).
    fn build(&mut self, scene: &Scene);
    fn rebuild_group(&mut self, scene: &Scene, gi: usize);
    /// Render to the texture and wrap it into a slint::Image (no copy).
    fn render(&mut self, view: &View, w: u32, h: u32, sf: f64, cull: bool) -> anyhow::Result<()>;
    fn image(&self) -> anyhow::Result<slint::Image>;
    /// Block until the GPU is idle (for timing).
    fn wait(&self);
    /// Read the texture back (headless PNG output only).
    fn readback(&self) -> anyhow::Result<(u32, u32, Vec<u8>)>;
}

fn color<C>(argb: u32, f: impl Fn(u8, u8, u8, u8) -> C) -> C {
    f((argb >> 16) as u8, (argb >> 8) as u8, argb as u8, (argb >> 24) as u8)
}

macro_rules! gpu_impl {
    ($modname:ident, $vello:ident, $slint_wgpu:path, $variant:ident, $unmap:expr) => {
        pub mod $modname {
            use super::*;
            use $slint_wgpu as slint_wgpu;
            use $vello as vello;
            use vello::kurbo::{Affine, Rect};
            use vello::peniko::{Color, Fill};
            use vello::wgpu;

            pub struct Gpu {
                pub device: wgpu::Device,
                pub queue: wgpu::Queue,
                renderer: vello::Renderer,
                frags: Vec<vello::Scene>,
                bboxes: Vec<Rect>,
                master: vello::Scene,
                target: Option<(wgpu::Texture, wgpu::TextureView, u32, u32)>,
            }

            impl Gpu {
                pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> anyhow::Result<Self> {
                    let renderer = vello::Renderer::new(
                        &device,
                        vello::RendererOptions {
                            use_cpu: false,
                            antialiasing_support: vello::AaSupport::area_only(),
                            num_init_threads: None,
                            pipeline_cache: None,
                        },
                    )
                    .map_err(|e| anyhow::anyhow!("vello init: {e:?}"))?;
                    Ok(Self {
                        device,
                        queue,
                        renderer,
                        frags: Vec::new(),
                        bboxes: Vec::new(),
                        master: vello::Scene::new(),
                        target: None,
                    })
                }

                /// Own device for headless rendering (no Slint).
                pub fn headless() -> anyhow::Result<Self> {
                    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
                    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                        power_preference: wgpu::PowerPreference::HighPerformance,
                        ..Default::default()
                    }))?;
                    eprintln!("wgpu adapter: {:?}", adapter.get_info());
                    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;
                    Self::new(device, queue)
                }

                fn encode(&self, scene: &Scene, gi: usize) -> vello::Scene {
                    let g = &scene.groups[gi];
                    let mut frag = vello::Scene::new();
                    let c = color(layer_color(g.layer), Color::from_rgba8);
                    // kurbo types are shared (same kurbo 0.13) so no conversion needed.
                    let path = g.path(&scene.items);
                    match g.style {
                        Style::Fill => frag.fill(Fill::NonZero, Affine::IDENTITY, c, None, &path),
                        Style::Stroke(_) => {
                            let st = vello::kurbo::Stroke::new(g.stroke_width())
                                .with_caps(vello::kurbo::Cap::Round)
                                .with_join(vello::kurbo::Join::Round);
                            frag.stroke(&st, Affine::IDENTITY, c, None, &path);
                        }
                    }
                    frag
                }
            }

            impl GpuCanvas for Gpu {
                fn build(&mut self, scene: &Scene) {
                    self.frags = (0..scene.groups.len()).map(|gi| self.encode(scene, gi)).collect();
                    self.bboxes = scene.groups.iter().map(|g| g.bbox).collect();
                }

                fn rebuild_group(&mut self, scene: &Scene, gi: usize) {
                    self.frags[gi] = self.encode(scene, gi);
                    self.bboxes[gi] = scene.groups[gi].bbox;
                }

                fn render(&mut self, view: &View, w: u32, h: u32, sf: f64, cull: bool) -> anyhow::Result<()> {
                    if self.target.as_ref().map(|t| (t.2, t.3)) != Some((w, h)) {
                        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
                            label: Some("canvas"),
                            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                            mip_level_count: 1,
                            sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format: wgpu::TextureFormat::Rgba8Unorm,
                            usage: wgpu::TextureUsages::STORAGE_BINDING
                                | wgpu::TextureUsages::TEXTURE_BINDING
                                | wgpu::TextureUsages::RENDER_ATTACHMENT
                                | wgpu::TextureUsages::COPY_SRC,
                            view_formats: &[],
                        });
                        let v = tex.create_view(&wgpu::TextureViewDescriptor::default());
                        self.target = Some((tex, v, w, h));
                    }
                    let tf = view.affine(sf);
                    let visible = view.visible(w as f64 / sf, h as f64 / sf);
                    self.master.reset();
                    for (frag, bb) in self.frags.iter().zip(&self.bboxes) {
                        if !cull || bb.overlaps(visible) {
                            self.master.append(frag, Some(tf));
                        }
                    }
                    let (_, view_tex, _, _) = self.target.as_ref().unwrap();
                    self.renderer
                        .render_to_texture(
                            &self.device,
                            &self.queue,
                            &self.master,
                            view_tex,
                            &vello::RenderParams {
                                base_color: color(crate::scene::BACKGROUND, Color::from_rgba8),
                                width: w,
                                height: h,
                                antialiasing_method: vello::AaConfig::Area,
                            },
                        )
                        .map_err(|e| anyhow::anyhow!("vello render: {e:?}"))?;
                    Ok(())
                }

                fn image(&self) -> anyhow::Result<slint::Image> {
                    let tex = self.target.as_ref().map(|t| t.0.clone()).ok_or_else(|| anyhow::anyhow!("no target"))?;
                    let _ = std::marker::PhantomData::<slint_wgpu::WGPUConfiguration>;
                    Ok(slint::Image::try_from(tex)?)
                }

                fn wait(&self) {
                    let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
                }

                fn readback(&self) -> anyhow::Result<(u32, u32, Vec<u8>)> {
                    let (tex, _, w, h) = self.target.as_ref().ok_or_else(|| anyhow::anyhow!("no target"))?;
                    let (w, h) = (*w, *h);
                    let stride = (w * 4).div_ceil(256) * 256;
                    let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: None,
                        size: (stride * h) as u64,
                        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                        mapped_at_creation: false,
                    });
                    let mut enc = self.device.create_command_encoder(&Default::default());
                    enc.copy_texture_to_buffer(
                        tex.as_image_copy(),
                        wgpu::TexelCopyBufferInfo {
                            buffer: &buf,
                            layout: wgpu::TexelCopyBufferLayout {
                                offset: 0,
                                bytes_per_row: Some(stride),
                                rows_per_image: None,
                            },
                        },
                        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                    );
                    self.queue.submit([enc.finish()]);
                    buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
                    self.wait();
                    // wgpu 30 made get_mapped_range fallible.
                    let data = ($unmap)(buf.slice(..).get_mapped_range());
                    let mut out = Vec::with_capacity((w * h * 4) as usize);
                    for row in 0..h {
                        let s = (row * stride) as usize;
                        out.extend_from_slice(&data[s..s + (w * 4) as usize]);
                    }
                    Ok((w, h, out))
                }
            }

            /// Hook into Slint: capture Slint's device/queue at RenderingSetup.
            pub fn from_graphics_api(api: &slint::GraphicsAPI<'_>) -> Option<anyhow::Result<Gpu>> {
                match api {
                    slint::GraphicsAPI::$variant { device, queue, .. } => {
                        Some(Gpu::new(device.clone(), queue.clone()))
                    }
                    _ => None,
                }
            }
        }
    };
}

gpu_impl!(v29, vello29, slint::wgpu_29, WGPU29, |v| v);
gpu_impl!(v30, vello30, slint::wgpu_30, WGPU30, |v: Result<_, _>| v.expect("map buffer"));
