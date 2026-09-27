//! Headless screenshots of the main window (no upstream counterpart).
//!
//! Renders the real UI with Slint's software renderer into a PNG, without a
//! display: for tests, CI and agents that cannot run Slint's MCP server.
//! `librepcb --screenshot out.png --project X.lpp --tab board` uses it.
//!
//! [`install_platform()`] must run before the first Slint component is
//! created (Slint's platform can be set once per process). Timers (the
//! deferred actions of [`App`](crate::App)) are processed by
//! [`Headless::settle()`].

use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter};
use slint::{PhysicalSize, PlatformError, Rgb8Pixel};

/// Errors of the screenshot mode.
#[derive(Debug, thiserror::Error)]
pub enum ScreenshotError {
    /// The platform could not be installed (a component already exists).
    #[error("Failed to set up the headless platform: {0}")]
    Platform(#[from] slint::platform::SetPlatformError),
    /// Slint error.
    #[error(transparent)]
    Slint(#[from] PlatformError),
    /// PNG encoding or file error.
    #[error("Failed to write the screenshot: {0}")]
    Write(String),
}

struct HeadlessPlatform {
    window: Rc<MinimalSoftwareWindow>,
    start: Instant,
}

impl Platform for HeadlessPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.window.clone())
    }

    fn duration_since_start(&self) -> Duration {
        self.start.elapsed()
    }
}

/// The headless window.
pub struct Headless {
    window: Rc<MinimalSoftwareWindow>,
    size: PhysicalSize,
}

/// Installs the headless platform with a window of `width` × `height`
/// pixels (scale factor 1).
pub fn install_platform(width: u32, height: u32) -> Result<Headless, ScreenshotError> {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(HeadlessPlatform {
        window: window.clone(),
        start: Instant::now(),
    }))?;
    let size = PhysicalSize::new(width, height);
    Ok(Headless { window, size })
}

impl Headless {
    /// Processes timers (deferred actions) and renders until nothing
    /// changes anymore (at most `rounds` frames). Returns the last frame.
    pub fn settle(&self, rounds: usize) -> Vec<Rgb8Pixel> {
        self.window.set_size(self.size);
        let (w, h) = (self.size.width as usize, self.size.height as usize);
        let mut buffer = vec![Rgb8Pixel::new(0, 0, 0); w * h];
        let mut last = buffer.clone();
        for round in 0..rounds {
            // Let zero-duration timers expire.
            std::thread::sleep(Duration::from_millis(2));
            slint::platform::update_timers_and_animations();
            let drawn = self.window.draw_if_needed(|renderer| {
                renderer.render(&mut buffer, w);
            });
            if drawn {
                last.clone_from(&buffer);
            } else if round > 2 && !self.window.has_active_animations() {
                break;
            }
            self.window.request_redraw();
        }
        last
    }

    /// Renders the window and writes it as PNG.
    pub fn save_png(&self, path: &Path) -> Result<(), ScreenshotError> {
        let pixels = self.settle(30);
        write_png(path, self.size.width, self.size.height, &pixels)
    }

    /// The window size in pixels.
    pub fn size(&self) -> PhysicalSize {
        self.size
    }
}

/// Writes RGB pixels as PNG.
pub fn write_png(
    path: &Path,
    width: u32,
    height: u32,
    pixels: &[Rgb8Pixel],
) -> Result<(), ScreenshotError> {
    let file = std::fs::File::create(path).map_err(|e| ScreenshotError::Write(e.to_string()))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let data: Vec<u8> = pixels.iter().flat_map(|p| [p.r, p.g, p.b]).collect();
    encoder
        .write_header()
        .and_then(|mut w| w.write_image_data(&data))
        .map_err(|e| ScreenshotError::Write(e.to_string()))
}
