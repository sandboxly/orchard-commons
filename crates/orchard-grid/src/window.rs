//! Drawing into a winit window: owns the wgpu surface and keeps it sized.
//! Event handling stays with the caller (the app's UI), which redraws only on change.

use std::sync::Arc;

use winit::window::Window;

use crate::GridError;
use crate::fonts::{FontConfig, FontLibrary};
use crate::frame::Frame;
use crate::renderer::{GridRenderer, PrepareStats};

/// What [`WindowRenderer::render`] did with a frame.
#[derive(Debug)]
pub enum Rendered {
    /// Drawn and presented.
    Shown(PrepareStats),
    /// The window can't be seen (covered, minimized, on another Space):
    /// nothing was drawn. Draw again when it shows (winit's
    /// `WindowEvent::Occluded(false)`), not right away: on macOS this answer
    /// comes at once, so redrawing on it would spin.
    Hidden,
    /// The surface was reconfigured or timed out: request another redraw.
    Retry,
}

/// What windows draw with: the wgpu instance, adapter, device and queue.
/// An app's windows share one ([`WindowRenderer::with_gpu`]) and keep it
/// until the app ends: with NVIDIA's driver on Wayland, destroying one
/// window's own GPU context wipes the driver's Wayland functions for every
/// other window, and the next frame there crashes. Clones share it.
#[derive(Clone, Debug)]
pub struct Gpu {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

pub struct WindowRenderer {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: GridRenderer,
    gpu: Gpu,
}

impl WindowRenderer {
    /// Set up GPU drawing for `window` with the system fonts. Blocks briefly
    /// while the adapter and device open.
    pub fn new(window: Arc<Window>, font: FontConfig) -> Result<Self, GridError> {
        Self::with_library(window, FontLibrary::system(), font)
    }

    pub fn with_library(
        window: Arc<Window>,
        library: Arc<FontLibrary>,
        font: FontConfig,
    ) -> Result<Self, GridError> {
        let instance = wgpu::Instance::new(
            wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(window.clone())),
        );
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| GridError::Surface(e.to_string()))?;
        let (adapter, device, queue) = crate::headless::open_device(&instance, Some(&surface))?;
        let gpu = Gpu {
            instance,
            adapter,
            device,
            queue,
        };
        Self::with_surface(window, surface, gpu, library, font)
    }

    /// Another window of the app, on the GPU of one it has (see [`Gpu`]).
    pub fn with_gpu(
        window: Arc<Window>,
        gpu: &Gpu,
        library: Arc<FontLibrary>,
        font: FontConfig,
    ) -> Result<Self, GridError> {
        let surface = gpu
            .instance
            .create_surface(window.clone())
            .map_err(|e| GridError::Surface(e.to_string()))?;
        if !gpu.adapter.is_surface_supported(&surface) {
            return Err(GridError::Surface(
                "the GPU can't draw into this window".into(),
            ));
        }
        Self::with_surface(window, surface, gpu.clone(), library, font)
    }

    fn with_surface(
        window: Arc<Window>,
        surface: wgpu::Surface<'static>,
        gpu: Gpu,
        library: Arc<FontLibrary>,
        font: FontConfig,
    ) -> Result<Self, GridError> {
        let caps = surface.get_capabilities(&gpu.adapter);
        // A non-sRGB format: colors are blended in sRGB space, like Qt.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .or_else(|| caps.formats.first().copied())
            .ok_or_else(|| GridError::Surface("surface supports no formats".into()))?;
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            // Opaque when offered: the grid always paints every pixel, and a
            // compositor must not blend the desktop through text edges.
            alpha_mode: if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
                wgpu::CompositeAlphaMode::Opaque
            } else {
                caps.alpha_modes
                    .first()
                    .copied()
                    .unwrap_or(wgpu::CompositeAlphaMode::Auto)
            },
            view_formats: vec![],
            color_space: Default::default(),
        };
        surface.configure(&gpu.device, &config);
        let scale = window.scale_factor() as f32;
        let renderer = GridRenderer::new(
            gpu.device.clone(),
            gpu.queue.clone(),
            format,
            library,
            font,
            scale,
        )?;
        Ok(WindowRenderer {
            window,
            surface,
            config,
            renderer,
            gpu,
        })
    }

    /// The GPU this window draws with, for the app's other windows.
    pub fn gpu(&self) -> &Gpu {
        &self.gpu
    }

    pub fn window(&self) -> &Window {
        &self.window
    }

    pub fn renderer(&self) -> &GridRenderer {
        &self.renderer
    }

    pub fn renderer_mut(&mut self) -> &mut GridRenderer {
        &mut self.renderer
    }

    /// Surface size in physical pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// Grid size (columns, rows) that fits the window.
    pub fn grid_size(&self) -> (u16, u16) {
        self.renderer
            .metrics()
            .grid_size(self.config.width, self.config.height)
    }

    /// Call on `WindowEvent::Resized`.
    pub fn resize(&mut self, width: u32, height: u32) {
        let (w, h) = (width.max(1), height.max(1));
        if (w, h) != (self.config.width, self.config.height) {
            self.config.width = w;
            self.config.height = h;
            self.surface.configure(self.renderer.device(), &self.config);
        }
    }

    /// Call on `WindowEvent::ScaleFactorChanged`: re-rasterizes the font.
    pub fn set_scale_factor(&mut self, scale_factor: f64) -> Result<(), GridError> {
        self.renderer.set_scale_factor(scale_factor as f32)
    }

    /// Draw and present `frame`, unless the window is hidden or its surface
    /// isn't ready (see [`Rendered`]).
    pub fn render(&mut self, frame: &Frame) -> Result<Rendered, GridError> {
        let mut reconfigure = false;
        let texture = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) => t,
            wgpu::CurrentSurfaceTexture::Suboptimal(t) => {
                // Draw this one, reconfigure for the next — after presenting:
                // configuring while a frame is acquired is a wgpu error.
                reconfigure = true;
                t
            }
            wgpu::CurrentSurfaceTexture::Occluded => return Ok(Rendered::Hidden),
            wgpu::CurrentSurfaceTexture::Timeout => return Ok(Rendered::Retry),
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(self.renderer.device(), &self.config);
                return Ok(Rendered::Retry);
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err(GridError::Surface(
                    "validation error acquiring the surface texture".into(),
                ));
            }
        };
        let stats = self
            .renderer
            .prepare(frame, (self.config.width, self.config.height));
        let view = texture.texture.create_view(&Default::default());
        let mut encoder = self
            .renderer
            .device()
            .create_command_encoder(&Default::default());
        self.renderer.render(&mut encoder, &view);
        self.renderer.queue().submit([encoder.finish()]);
        self.window.pre_present_notify();
        self.renderer.queue().present(texture);
        if reconfigure {
            self.surface.configure(self.renderer.device(), &self.config);
        }
        Ok(Rendered::Shown(stats))
    }
}
