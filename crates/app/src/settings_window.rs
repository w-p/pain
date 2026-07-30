//! The settings panel, as its own operating-system window.
//!
//! It used to be an `egui::Window` floating inside the terminal window,
//! which meant it could never leave that window's bounds, competed with the
//! pane grid for space, and had to be shrunk and scrolled to fit whenever
//! the terminal window was small. A real window has none of those problems
//! and can be dragged to another monitor.
//!
//! The window is deliberately ordinary. The terminal window is not: it is
//! created transparent-capable, on Windows with `WS_EX_NOREDIRECTIONBITMAP`
//! and a DirectComposition swapchain requesting `PreMultiplied` alpha (see
//! `main.rs` and `Graphics::new`). None of that applies here — this is an
//! opaque form — and inheriting any of it would mean debugging the same
//! compositor problems twice for no benefit.
//!
//! It shares the GPU device with the terminal window rather than creating
//! its own. Two surfaces on one device is the normal arrangement, and it
//! keeps a single glyph atlas and one renderer allocation.

use std::sync::Arc;

use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

use crate::ui::{BindingRow, SettingsDraft, SettingsOutcome};

/// Initial size, in logical points. Tall enough to show the form down to
/// the Save/Cancel row without scrolling at the default font, narrow enough
/// not to feel like a second terminal.
const DEFAULT_SIZE: (f64, f64) = (460.0, 720.0);

/// Padding between the form and the window edge. The form used to sit
/// inside an `egui::Window`, which supplied this itself.
const WINDOW_MARGIN: i8 = 10;

/// The settings window: its own OS window, surface and egui context, over
/// the terminal window's GPU device.
pub struct SettingsWindow {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    ctx: egui::Context,
    state: egui_winit::State,
    renderer: egui_wgpu::Renderer,
    /// The edits in progress. Owned here rather than in `Graphics` because
    /// the window's lifetime *is* the draft's lifetime: opening the window
    /// starts a draft from the current config, closing it discards one.
    draft: SettingsDraft,
    /// The keybinding list, built once when the window opens.
    ///
    /// Building it runs `Keymap::apply_overrides`, which reports
    /// unparseable lines to stderr — so rebuilding it per frame would turn
    /// one bad config line into an endless stream of warnings. That is why
    /// this used to be cached in `Ui`; owning the window makes the cache
    /// unnecessary, since the window's lifetime is already the right scope.
    binding_rows: Vec<BindingRow>,
}

impl SettingsWindow {
    /// Creates the window and its surface on the terminal's existing
    /// device, seeding the draft from `settings`.
    pub fn new(
        event_loop: &ActiveEventLoop,
        instance: &wgpu::Instance,
        adapter: &wgpu::Adapter,
        device: &wgpu::Device,
        settings: &config::Config,
    ) -> anyhow::Result<Self> {
        let attributes = Window::default_attributes()
            .with_title("pain — Settings")
            .with_window_icon(crate::window_icon())
            .with_inner_size(winit::dpi::LogicalSize::new(DEFAULT_SIZE.0, DEFAULT_SIZE.1));
        let window = Arc::new(event_loop.create_window(attributes)?);

        let surface = instance.create_surface(window.clone())?;
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(adapter, size.width.max(1), size.height.max(1))
            .ok_or_else(|| anyhow::anyhow!("adapter does not support the settings window's surface"))?;
        // Same reasoning as the terminal window's: `get_default_config`
        // picks whatever the backend lists first, and on DX12 that is
        // Mailbox — an uncapped present mode, which for a settings form is
        // pure wasted GPU.
        config.present_mode = wgpu::PresentMode::Fifo;
        surface.configure(device, &config);

        let ctx = crate::ui::chrome_context();
        let state = egui_winit::State::new(ctx.clone(), egui::ViewportId::ROOT, &window, None, None, None);
        let renderer = egui_wgpu::Renderer::new(device, config.format, egui_wgpu::RendererOptions::default());

        Ok(Self {
            window,
            surface,
            config,
            ctx,
            state,
            renderer,
            draft: SettingsDraft::from_config(settings),
            binding_rows: crate::ui::effective_binding_rows(&settings.keybindings),
        })
    }

    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }

    /// The config the in-progress edits would produce on top of `base` —
    /// what the terminal window renders against while the panel is open, so
    /// a font or color change previews immediately rather than only on Save.
    pub fn preview(&self, base: &config::Config) -> config::Config {
        self.draft.apply_to(base)
    }

    /// Feeds an event to egui. Returns whether the window needs repainting.
    pub fn on_window_event(&mut self, event: &WindowEvent) -> bool {
        self.state.on_window_event(&self.window, event).repaint
    }

    pub fn resize(&mut self, device: &wgpu::Device, size: winit::dpi::PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(device, &self.config);
    }

    /// Draws one frame and reports what the user asked for.
    ///
    /// `settings` is the *saved* config the draft is applied on top of, not
    /// the live-previewed one — otherwise each frame's preview would become
    /// the next frame's baseline and the edits would compound.
    pub fn redraw(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, settings: &config::Config) -> SettingsOutcome {
        self.ctx.set_visuals(crate::ui::graphite_visuals(settings.appearance.accent_rgb()));

        let raw_input = self.state.take_egui_input(&self.window);
        let mut outcome = SettingsOutcome::default();
        let draft = &mut self.draft;
        let binding_rows = &self.binding_rows;
        // `run_ui`, matching `Ui::show` — see its own comment for why the
        // `begin_pass`/`end_pass` pair is not equivalent. Here the root
        // `Ui` it hands back *is* the whole window, so the form is drawn
        // straight into it with a margin rather than into a panel.
        let full_output = self.ctx.run_ui(raw_input, |ui| {
            let frame = egui::Frame::central_panel(ui.style()).inner_margin(WINDOW_MARGIN);
            frame.show(ui, |ui| {
                outcome = crate::ui::settings_panel(ui, draft, settings, binding_rows);
            });
        });
        self.state.handle_platform_output(&self.window, full_output.platform_output.clone());

        // Same treatment as the terminal window's `redraw`: a lost or
        // outdated surface is transient (a resize mid-frame, the window
        // moving between monitors), so reconfigure and let the next frame
        // draw it rather than treating it as an error.
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(device, &self.config);
                return outcome;
            }
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => return outcome,
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("settings") });

        let pixels_per_point = egui_winit::pixels_per_point(&self.ctx, &self.window);
        let screen_descriptor =
            egui_wgpu::ScreenDescriptor { size_in_pixels: [self.config.width, self.config.height], pixels_per_point };
        let primitives = self.ctx.tessellate(full_output.shapes, full_output.pixels_per_point);
        for (id, delta) in &full_output.textures_delta.set {
            self.renderer.update_texture(device, queue, *id, delta);
        }
        let buffers = self.renderer.update_buffers(device, queue, &mut encoder, &primitives, &screen_descriptor);
        if !buffers.is_empty() {
            queue.submit(buffers);
        }
        {
            // Cleared rather than loaded: nothing else draws into this
            // window, so there is no terminal grid underneath to preserve
            // the way `Ui::render` has to.
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("settings egui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(panel_clear_color()),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            let mut pass = pass.forget_lifetime();
            self.renderer.render(&mut pass, &primitives, &screen_descriptor);
        }
        for id in &full_output.textures_delta.free {
            self.renderer.free_texture(id);
        }
        queue.submit(Some(encoder.finish()));
        self.window.pre_present_notify();
        frame.present();

        // egui asks to be woken again when something is animating (a
        // hovered button's fade, a text cursor blink). Anything shorter
        // than the next frame means "keep going".
        if full_output.viewport_output.values().any(|viewport| viewport.repaint_delay.is_zero()) {
            self.window.request_redraw();
        }

        outcome
    }
}

/// The settings window's background, matching the panel color the chrome
/// style paints its widgets against — so the frame reads as one surface
/// rather than a panel floating on an unrelated ground.
///
/// Written in linear space because this is a clear value on an sRGB-format
/// surface, which the GPU gamma-encodes on the way to the display. Same
/// conversion the terminal window's clear color needs, and the same reason:
/// see `render/src/shader.wgsl`'s `srgb_to_linear`.
fn panel_clear_color() -> wgpu::Color {
    let channel = |srgb: f64| if srgb <= 0.040_45 { srgb / 12.92 } else { ((srgb + 0.055) / 1.055).powf(2.4) };
    let [r, g, b] = crate::ui::PANEL_BG_RGB;
    wgpu::Color { r: channel(r), g: channel(g), b: channel(b), a: 1.0 }
}
