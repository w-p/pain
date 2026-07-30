//! Draws a pane's character grid: glyphs rasterized via `cosmic-text`, packed
//! into a GPU texture atlas, and drawn as instanced quads via `wgpu`.

mod atlas;
mod glyph;

use bytemuck::{Pod, Zeroable};

pub use glyph::{
    GlyphRasterizer, RasterizedGlyph, ShapedGlyph, first_installed_family, monospace_font_families, system_ui_font_data,
};

/// Measures a font's cell size at `font_size_px` in `font_family` (`""` or
/// `"monospace"` for the system default): the advance width of a
/// representative glyph, and a line height of `1.25x` the font size. Both
/// are rounded to whole pixels so grid positions land on the pixel grid —
/// see the position-rounding in [`GridRenderer::render`] for why that
/// matters.
pub fn measure_cell(font_size_px: f32, font_family: &str) -> (f32, f32) {
    let mut rasterizer = GlyphRasterizer::new();
    let width = rasterizer.advance_width('M', font_size_px, font_family).unwrap_or(font_size_px * 0.6);
    (width.round(), (font_size_px * 1.25).round())
}

const QUAD_CORNERS: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct QuadVertex {
    corner: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    pos: [f32; 2],
    size: [f32; 2],
    uv_origin: [f32; 2],
    uv_size: [f32; 2],
    color: [f32; 4],
    /// 1.0 to sample the color atlas and use the glyph's own colors, 0.0 to
    /// sample the coverage atlas and tint by `color`. A float rather than a
    /// `u32` flag so the vertex format stays uniformly `Float32*` — there is
    /// exactly one bit of information here and nowhere to grow.
    colored: f32,
}

/// Screen effects to draw over the finished grid. All strengths are 0.0–1.0,
/// and all-zero means nothing is drawn at all.
///
/// These are deliberately *static* — no time input, nothing animated. That is
/// what makes them free: they add one draw call to a frame that was going to
/// be rendered anyway, so an idle terminal still renders nothing and still
/// sleeps. Anything animated (phosphor decay, flicker, rain) would force
/// continuous redraw and belongs behind its own separate opt-in.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Effects {
    pub scanlines: f32,
    pub vignette: f32,
    /// Strength of the drifting mains-hum bar, 0.0–1.0.
    ///
    /// **The only animated effect.** Everything else here is static and so
    /// costs nothing on an idle terminal; this one requires the caller to
    /// keep rendering. See `hum_phase`.
    pub hum: f32,
    /// Where the hum bar currently sits, 0.0–1.0, wrapping.
    ///
    /// A phase rather than a timestamp, computed by the caller. Handing the
    /// shader raw elapsed seconds would lose precision in `f32` after a few
    /// hours of uptime; a value that always stays inside one cycle never
    /// does.
    pub hum_phase: f32,
    /// Physical pixels per light/dark scanline cycle. Scaled by the caller to
    /// the display, so scanlines look the same density on a HiDPI screen as
    /// on a 1x one.
    pub scanline_period: f32,
    /// The theme's foreground, in sRGB — the phosphor color the vignette's
    /// ambient lift is tinted with. See `effects.wgsl`.
    pub glow_color: [f32; 3],
}

impl Effects {
    /// Whether there is anything to draw. The renderer skips the whole pass
    /// when there isn't, so the default path costs nothing.
    pub fn is_empty(&self) -> bool {
        self.scanlines <= 0.0 && self.vignette <= 0.0 && self.hum <= 0.0
    }

    /// Whether anything here needs the frame redrawn to keep moving. Only the
    /// hum bar does; the caller uses this to decide whether the terminal may
    /// go back to sleep.
    pub fn is_animated(&self) -> bool {
        self.hum > 0.0
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct EffectsUniform {
    screen_size: [f32; 2],
    scanline_strength: f32,
    scanline_period: f32,
    vignette_strength: f32,
    opacity: f32,
    // These two occupy what was padding. WGSL aligns a `vec3<f32>` to 16
    // bytes, so `glow_color` sits at offset 32 either way — the eight bytes
    // before it were dead space, and filling them keeps the uniform at the
    // same size. The mismatch this padding originally fixed is rejected by
    // wgpu only at draw time ("bound with size 40 where the shader expects
    // 48"); the assertion below turns it into a build failure instead.
    hum_strength: f32,
    hum_phase: f32,
    glow_color: [f32; 3],
    _padding: f32,
}

/// The layout the shader's `Effects` struct expects. A field added to either
/// side without the other fails here rather than at the first draw.
const _: () = assert!(std::mem::size_of::<EffectsUniform>() == 48);

/// A region the screen effects cover — a pane's *content* rect, excluding its
/// title bar. One instance per area; see `effects.wgsl` for why this isn't
/// simply the whole window.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct EffectArea {
    pos: [f32; 2],
    size: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    screen_size: [f32; 2],
}

/// One character to draw. `x`/`y` are the absolute pixel position of the
/// cell's top-left corner — callers (which know about pane layout, if any)
/// are responsible for offsetting by a pane's screen position; the renderer
/// itself has no notion of panes.
pub struct GlyphCell {
    pub x: f32,
    pub y: f32,
    pub c: char,
    pub color: [f32; 4],
}

/// A run of characters to shape and draw together, so the font can apply
/// ligatures across them. The ligature-mode counterpart to [`GlyphCell`].
///
/// `x`/`y` are the absolute pixel position of the run's first cell. Glyphs
/// within the run are then placed by the font's own advances rather than by
/// cell arithmetic — so the caller must only group cells where ligating is
/// actually correct: one color, and no cursor sitting inside the run.
pub struct GlyphRun {
    pub x: f32,
    pub y: f32,
    pub text: String,
    pub color: [f32; 4],
}

/// A solid-filled rectangle: the cursor, a divider, or any other chrome
/// drawn without a glyph. Sampled from a reserved 1x1 opaque texel in the
/// atlas, so it goes through the same instanced draw as glyphs.
pub struct SolidRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub color: [f32; 4],
}

/// Draws glyphs and solid rects (cursors, dividers) via one instanced pass.
pub struct GridRenderer {
    pipeline: wgpu::RenderPipeline,
    quad_vbo: wgpu::Buffer,
    instance_vbo: wgpu::Buffer,
    instance_capacity: usize,
    globals_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    atlas: atlas::GlyphAtlas,
    /// Separate pipeline for the fullscreen effects overlay: it takes only
    /// the quad vertex buffer and its own uniform, not the grid's instance
    /// buffer or texture bindings.
    effects_pipeline: wgpu::RenderPipeline,
    effects_buffer: wgpu::Buffer,
    effects_bind_group: wgpu::BindGroup,
    effects_area_vbo: wgpu::Buffer,
    effects_area_capacity: usize,
}

impl GridRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let atlas = atlas::GlyphAtlas::new(device, queue);

        // Nearest, not linear: glyph quads are always drawn at the exact
        // pixel size they were rasterized at (see the position-rounding in
        // `render`), so there is no scaling for linear filtering to smooth —
        // only a risk of it bleeding into the next glyph packed edge-to-edge
        // in the atlas.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("glyph-sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("grid-bind-group-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // Color glyphs (emoji) live in their own RGBA texture — see
                // `atlas::COLOR_ATLAS_SIZE`. Both are bound for every draw
                // and the fragment shader picks per instance, rather than
                // splitting into two pipelines and two passes for what is
                // usually a handful of glyphs.
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("grid-bind-group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: globals_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&atlas.view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&atlas.color_view) },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("grid-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("grid-pipeline-layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("grid-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<QuadVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x2],
                    },
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Instance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &wgpu::vertex_attr_array![
                            1 => Float32x2,
                            2 => Float32x2,
                            3 => Float32x2,
                            4 => Float32x2,
                            5 => Float32x4,
                            6 => Float32,
                        ],
                    },
                ],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    // Premultiplied, not straight: `fs_main` outputs
                    // premultiplied color (RGB already scaled by its own
                    // effective alpha), which this blend mode expects —
                    // see the shader's own comment for why (Windows'
                    // DirectComposition swapchains, used for window
                    // transparency, only accept premultiplied content).
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let quad_vbo = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("quad-vbo"),
            size: (QUAD_CORNERS.len() * std::mem::size_of::<QuadVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let quad_data: Vec<QuadVertex> = QUAD_CORNERS.into_iter().map(|corner| QuadVertex { corner }).collect();
        queue.write_buffer(&quad_vbo, 0, bytemuck::cast_slice(&quad_data));

        let instance_capacity = 65536;
        let instance_vbo = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instance-vbo"),
            size: (instance_capacity * std::mem::size_of::<Instance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Effects overlay: its own shader, uniform and pipeline, sharing only
        // the quad vertex buffer. Kept separate rather than folded into the
        // grid pipeline because it needs neither the instance buffer nor the
        // atlas bindings, and because a pass that draws nothing should cost
        // nothing to skip.
        let effects_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("effects-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("effects.wgsl").into()),
        });

        let effects_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("effects-uniform"),
            size: std::mem::size_of::<EffectsUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let effects_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("effects-bind-group-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let effects_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("effects-bind-group"),
            layout: &effects_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: effects_buffer.as_entire_binding() }],
        });

        let effects_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("effects-pipeline"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("effects-pipeline-layout"),
                bind_group_layouts: &[Some(&effects_layout)],
                immediate_size: 0,
            })),
            vertex: wgpu::VertexState {
                module: &effects_shader,
                entry_point: Some("vs_main"),
                buffers: &[
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<QuadVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x2],
                    },
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<EffectArea>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &wgpu::vertex_attr_array![1 => Float32x2, 2 => Float32x2],
                    },
                ],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &effects_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    // Same premultiplied blending as the grid — the overlay
                    // emits black at an alpha, which composites to a straight
                    // darkening of whatever is already there.
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        // One area per visible pane, which is far more than any real layout.
        let effects_area_capacity = 256;
        let effects_area_vbo = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("effects-area-vbo"),
            size: (effects_area_capacity * std::mem::size_of::<EffectArea>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            quad_vbo,
            instance_vbo,
            instance_capacity,
            globals_buffer,
            bind_group,
            atlas,
            effects_pipeline,
            effects_buffer,
            effects_bind_group,
            effects_area_vbo,
            effects_area_capacity,
        }
    }

    /// Clears `view` to `background` and draws `rects` (cursors, dividers)
    /// followed by `glyphs`, all in absolute pixel coordinates.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        screen_size: (u32, u32),
        font_size_px: f32,
        font_family: &str,
        background: wgpu::Color,
        rects: impl Iterator<Item = SolidRect>,
        glyphs: impl Iterator<Item = GlyphCell>,
        runs: impl Iterator<Item = GlyphRun>,
        effects: Effects,
        // Where the effects apply — the panes' content rects, as
        // `(x, y, width, height)`. Title bars are left out by the caller.
        effect_areas: &[(f32, f32, f32, f32)],
    ) {
        queue.write_buffer(
            &self.globals_buffer,
            0,
            bytemuck::bytes_of(&Globals { screen_size: [screen_size.0 as f32, screen_size.1 as f32] }),
        );

        let effect_areas: Vec<EffectArea> = if effects.is_empty() {
            Vec::new()
        } else {
            effect_areas
                .iter()
                .take(self.effects_area_capacity)
                .map(|&(x, y, width, height)| EffectArea { pos: [x, y], size: [width, height] })
                .collect()
        };
        if !effect_areas.is_empty() {
            queue.write_buffer(
                &self.effects_buffer,
                0,
                bytemuck::bytes_of(&EffectsUniform {
                    screen_size: [screen_size.0 as f32, screen_size.1 as f32],
                    scanline_strength: effects.scanlines,
                    // Guarded: a zero or negative period would divide by zero
                    // in the shader and paint the whole window black.
                    scanline_period: effects.scanline_period.max(1.0),
                    vignette_strength: effects.vignette,
                    opacity: background.a as f32,
                    hum_strength: effects.hum,
                    hum_phase: effects.hum_phase,
                    glow_color: effects.glow_color,
                    _padding: 0.0,
                }),
            );
            queue.write_buffer(&self.effects_area_vbo, 0, bytemuck::cast_slice(&effect_areas));
        }

        let mut instances = Vec::new();

        for rect in rects {
            instances.push(Instance {
                pos: [rect.x.round(), rect.y.round()],
                size: [rect.width, rect.height],
                uv_origin: self.atlas.solid_uv,
                uv_size: [0.0, 0.0],
                color: rect.color,
                colored: 0.0,
            });
        }

        for glyph in glyphs {
            let Some(entry) = self.atlas.entry(queue, glyph.c, font_size_px, font_family) else {
                continue;
            };
            // `top` is the offset from the baseline up to the bitmap's top edge;
            // the baseline itself sits `font_size_px` down from the cell's top.
            let pen_y = glyph.y + font_size_px;
            // Snapped to whole pixels: the atlas has no padding between glyphs
            // and the sampler sees each texel as an exact screen pixel, so any
            // fractional position bleeds neighboring glyphs' edges together.
            instances.push(Instance {
                pos: [(glyph.x + entry.left).round(), (pen_y - entry.top).round()],
                size: [entry.width, entry.height],
                uv_origin: entry.uv_origin,
                uv_size: entry.uv_size,
                color: glyph.color,
                colored: if entry.colored { 1.0 } else { 0.0 },
            });
        }

        for run in runs {
            // The baseline sits `font_size_px` below the run's top edge,
            // same as the per-character path.
            let pen_y = run.y + font_size_px;
            // Collected because `shape_run` borrows the atlas mutably and so
            // does `shaped_entry` — the shaped glyphs are small and there is
            // one run per contiguous stretch of same-colored cells, not one
            // per cell.
            let shaped: Vec<glyph::ShapedGlyph> = self.atlas.shape_run(&run.text, font_size_px, font_family).to_vec();

            for glyph in shaped {
                let Some(entry) = self.atlas.shaped_entry(queue, glyph, font_size_px, font_family) else {
                    continue;
                };
                instances.push(Instance {
                    pos: [(run.x + glyph.x as f32 + entry.left).round(), (pen_y + glyph.y as f32 - entry.top).round()],
                    size: [entry.width, entry.height],
                    uv_origin: entry.uv_origin,
                    uv_size: entry.uv_size,
                    color: run.color,
                    colored: if entry.colored { 1.0 } else { 0.0 },
                });
            }
        }

        if instances.len() > self.instance_capacity {
            instances.truncate(self.instance_capacity);
        }
        if !instances.is_empty() {
            queue.write_buffer(&self.instance_vbo, 0, bytemuck::cast_slice(&instances));
        }

        // `LoadOp::Clear` writes this value directly into the render
        // target — unlike every draw call, it never passes through
        // `fs_main`, so it has to already be premultiplied by hand here to
        // match everything else the pipeline now produces.
        let premultiplied_background = wgpu::Color {
            r: background.r * background.a,
            g: background.g * background.a,
            b: background.b * background.a,
            a: background.a,
        };

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("grid"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(premultiplied_background),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });

            if !instances.is_empty() {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.set_vertex_buffer(0, self.quad_vbo.slice(..));
                pass.set_vertex_buffer(1, self.instance_vbo.slice(..));
                pass.draw(0..4, 0..instances.len() as u32);
            }

            // Over the finished grid, and still inside this pass — so the
            // egui chrome, which renders in its own pass afterwards, stays
            // crisp and unshaded. A warped or scanlined settings panel would
            // be unusable, and the chrome isn't part of the illusion.
            if !effect_areas.is_empty() {
                pass.set_pipeline(&self.effects_pipeline);
                pass.set_bind_group(0, &self.effects_bind_group, &[]);
                pass.set_vertex_buffer(0, self.quad_vbo.slice(..));
                pass.set_vertex_buffer(1, self.effects_area_vbo.slice(..));
                pass.draw(0..4, 0..effect_areas.len() as u32);
            }
        }

        queue.submit(Some(encoder.finish()));
    }
}
