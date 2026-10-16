//! The wgpu renderer: WebGPU where the browser has it, WebGL2 otherwise,
//! from the same code and the same WGSL.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::HtmlCanvasElement;
use wgpu::util::DeviceExt;

use crate::color::Space;
use crate::scene::{Scene, Target, Uniforms};

const TEXTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
/// Offscreen renders: the shader writes sRGB-encoded values itself.
const READBACK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// Exports larger than this are rendered in tiles.
const TILE: u32 = 2048;

struct Photo {
    view: wgpu::TextureView,
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    backend: wgpu::Backend,
    preview_space: Space,
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    shader: wgpu::ShaderModule,
    pipelines: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    mip_pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    photos: [Option<Photo>; 2],
    blank: wgpu::TextureView,
    bind_group: Option<wgpu::BindGroup>,
}

fn js_err(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}

/// Ask a WebGL2 canvas to present in Display P3. Returns whether it took.
fn webgl_display_p3(canvas: &HtmlCanvasElement) -> bool {
    let Ok(Some(ctx)) = canvas.get_context("webgl2") else { return false };
    let key = JsValue::from_str("drawingBufferColorSpace");
    if !js_sys::Reflect::has(&ctx, &key).unwrap_or(false) {
        return false;
    }
    let _ = js_sys::Reflect::set(&ctx, &key, &JsValue::from_str("display-p3"));
    js_sys::Reflect::get(&ctx, &key).ok().and_then(|v| v.as_string()).as_deref() == Some("display-p3")
}

/// Yield to the browser's event loop once.
async fn tick() {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        let global = js_sys::global();
        let set_timeout = js_sys::Reflect::get(&global, &"setTimeout".into())
            .ok()
            .and_then(|f| f.dyn_into::<js_sys::Function>().ok());
        if let Some(f) = set_timeout {
            let _ = f.call2(&global, &resolve, &JsValue::from(0));
        } else {
            let _ = resolve.call0(&JsValue::NULL);
        }
    });
    let _ = JsFuture::from(promise).await;
}

impl Renderer {
    pub async fn new(canvas: HtmlCanvasElement, force_webgl: bool, want_p3: bool) -> Result<Renderer, JsValue> {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = if force_webgl { wgpu::Backends::GL } else { wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL };
        let instance = wgpu::util::new_instance_with_webgpu_detection(desc).await;
        let surface = instance.create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone())).map_err(js_err)?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
                apply_limit_buckets: false,
            })
            .await
            .map_err(js_err)?;
        let backend = adapter.get_info().backend;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("diagonal-blend"),
                required_limits: adapter.limits(),
                ..Default::default()
            })
            .await
            .map_err(js_err)?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| matches!(f, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm))
            .or_else(|| caps.formats.first().copied())
            .ok_or_else(|| JsValue::from_str("canvas has no usable format"))?;
        let p3_capable = caps
            .format_capabilities
            .iter()
            .any(|c| c.format == format && c.color_spaces.contains(wgpu::SurfaceColorSpaces::DISPLAY_P3));
        let width = canvas.width().max(1);
        let height = canvas.height().max(1);
        let mut config = surface.get_default_config(&adapter, width, height).ok_or("no default surface config")?;
        config.format = format;
        config.usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
        config.view_formats = vec![];
        let mut preview_space = Space::Srgb;
        if want_p3 && backend == wgpu::Backend::BrowserWebGpu && p3_capable {
            config.color_space = wgpu::SurfaceColorSpace::DisplayP3;
            preview_space = Space::DisplayP3;
        } else {
            config.color_space = wgpu::SurfaceColorSpace::Srgb;
        }
        surface.configure(&device, &config);
        if want_p3 && backend == wgpu::Backend::Gl && webgl_display_p3(&canvas) {
            preview_space = Space::DisplayP3;
        }

        let shader = device.create_shader_module(wgpu::include_wgsl!("blend.wgsl"));
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let sampler_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blend"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture_entry(1),
                texture_entry(2),
                sampler_entry(3),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blend"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });

        let mip_shader = device.create_shader_module(wgpu::include_wgsl!("mip.wgsl"));
        let mip_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mip"),
            entries: &[texture_entry(0), sampler_entry(1)],
        });
        let mip_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mip"),
            bind_group_layouts: &[Some(&mip_layout)],
            immediate_size: 0,
        });
        let mip_pipeline = Renderer::pipeline(&device, &mip_pipeline_layout, &mip_shader, TEXTURE_FORMAT);

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("photo"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let blank = device
            .create_texture_with_data(
                &queue,
                &wgpu::TextureDescriptor {
                    label: Some("blank"),
                    size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: TEXTURE_FORMAT,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                &[0, 0, 0, 255],
            )
            .create_view(&Default::default());

        let mut r = Renderer {
            device,
            queue,
            surface,
            config,
            backend,
            preview_space,
            layout,
            pipeline_layout,
            shader,
            pipelines: HashMap::new(),
            mip_pipeline,
            sampler,
            uniforms,
            photos: [None, None],
            blank,
            bind_group: None,
        };
        r.ensure_pipeline(format);
        r.ensure_pipeline(READBACK_FORMAT);
        Ok(r)
    }

    fn pipeline(
        device: &wgpu::Device,
        layout: &wgpu::PipelineLayout,
        shader: &wgpu::ShaderModule,
        format: wgpu::TextureFormat,
    ) -> wgpu::RenderPipeline {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("fullscreen"),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        })
    }

    fn ensure_pipeline(&mut self, format: wgpu::TextureFormat) {
        if !self.pipelines.contains_key(&format) {
            let p = Renderer::pipeline(&self.device, &self.pipeline_layout, &self.shader, format);
            self.pipelines.insert(format, p);
        }
    }

    pub fn backend(&self) -> &'static str {
        match self.backend {
            wgpu::Backend::BrowserWebGpu => "webgpu",
            wgpu::Backend::Gl => "webgl2",
            _ => "other",
        }
    }

    pub fn preview_space(&self) -> Space {
        self.preview_space
    }

    pub fn max_texture_size(&self) -> u32 {
        self.device.limits().max_texture_dimension_2d
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        let (w, h) = (width.clamp(1, self.max_texture_size()), height.clamp(1, self.max_texture_size()));
        if w != self.config.width || h != self.config.height {
            self.config.width = w;
            self.config.height = h;
            self.surface.configure(&self.device, &self.config);
        }
    }

    /// Upload RGBA8 pixels (sRGB-encoded values in the photo's own space)
    /// and build the mip chain.
    pub fn set_texture(&mut self, which: usize, rgba: &[u8], width: u32, height: u32) -> Result<(), JsValue> {
        let max = self.max_texture_size();
        if width == 0 || height == 0 || width > max || height > max {
            return Err(JsValue::from_str(&format!("texture {width}x{height} exceeds {max}")));
        }
        if rgba.len() != (width * height * 4) as usize {
            return Err(JsValue::from_str("pixel buffer has the wrong length"));
        }
        let levels = 32 - width.max(height).leading_zeros();
        let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(if which == 0 { "photo A" } else { "photo B" }),
            size,
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TEXTURE_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width * 4), rows_per_image: Some(height) },
            size,
        );
        self.generate_mips(&texture, levels);
        self.photos[which.min(1)] = Some(Photo { view: texture.create_view(&Default::default()) });
        self.bind_group = None;
        Ok(())
    }

    pub fn clear_texture(&mut self, which: usize) {
        self.photos[which.min(1)] = None;
        self.bind_group = None;
    }

    pub fn swap(&mut self) {
        self.photos.swap(0, 1);
        self.bind_group = None;
    }

    fn generate_mips(&self, texture: &wgpu::Texture, levels: u32) {
        let layout = self.mip_pipeline.get_bind_group_layout(0);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let level_view = |level| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: level,
                mip_level_count: Some(1),
                ..Default::default()
            })
        };
        for level in 1..levels {
            let src = level_view(level - 1);
            let dst = level_view(level);
            let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("mip"),
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&src) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mip"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &dst,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.mip_pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit([encoder.finish()]);
    }

    fn bind_group(&mut self) -> wgpu::BindGroup {
        if let Some(b) = &self.bind_group {
            return b.clone();
        }
        let view = |i: usize| self.photos[i].as_ref().map_or(&self.blank, |p| &p.view);
        let b = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blend"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(view(0)) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(view(1)) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        self.bind_group = Some(b.clone());
        b
    }

    fn draw(&mut self, target: &wgpu::TextureView, format: wgpu::TextureFormat, u: &Uniforms) -> wgpu::CommandBuffer {
        self.ensure_pipeline(format);
        self.queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(u));
        let bind = self.bind_group();
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("blend"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipelines[&format]);
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
        encoder.finish()
    }

    /// Draw the preview into the canvas.
    pub fn render(&mut self, scene: &Scene, bg: [f64; 3], time: Option<f64>) {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            _ => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
        };
        let view = frame.texture.create_view(&Default::default());
        let target = Target::whole(self.config.width as f64, self.config.height as f64);
        let u = scene.uniforms(&target, self.preview_space, bg, time, true);
        let cmd = self.draw(&view, self.config.format, &u);
        self.queue.submit([cmd]);
        self.queue.present(frame);
    }

    /// Render `scene` offscreen at `width` x `height` and return RGBA8
    /// pixels, tile by tile so the size is not bounded by texture limits.
    pub async fn render_pixels(
        this: Rc<std::cell::RefCell<Renderer>>,
        scene: Scene,
        width: u32,
        height: u32,
        space: Space,
        bg: [f64; 3],
        time: Option<f64>,
    ) -> Result<Vec<u8>, JsValue> {
        if width == 0 || height == 0 || (width as u64) * (height as u64) > 400_000_000 {
            return Err(JsValue::from_str("export size out of range"));
        }
        let tile = TILE.min(this.borrow().max_texture_size());
        let mut out = vec![0u8; (width * height * 4) as usize];
        for ty in (0..height).step_by(tile as usize) {
            for tx in (0..width).step_by(tile as usize) {
                let (tw, th) = (tile.min(width - tx), tile.min(height - ty));
                let target = Target {
                    width: width as f64,
                    height: height as f64,
                    x: tx as f64,
                    y: ty as f64,
                    tile_w: tw as f64,
                    tile_h: th as f64,
                };
                let u = scene.uniforms(&target, space, bg, time, false);
                let (buffer, padded, device) = {
                    let mut r = this.borrow_mut();
                    r.encode_tile(&u, tw, th)
                };
                Renderer::map(&device, &buffer).await?;
                {
                    let data = buffer.slice(..).get_mapped_range().map_err(js_err)?;
                    let row = (tw * 4) as usize;
                    for y in 0..th as usize {
                        let src = &data[y * padded as usize..y * padded as usize + row];
                        let at = (((ty as usize + y) * width as usize) + tx as usize) * 4;
                        out[at..at + row].copy_from_slice(src);
                    }
                }
                buffer.unmap();
            }
        }
        Ok(out)
    }

    fn encode_tile(&mut self, u: &Uniforms, w: u32, h: u32) -> (wgpu::Buffer, u32, wgpu::Device) {
        let size = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("export tile"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: READBACK_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let draw = self.draw(&view, READBACK_FORMAT, u);
        let padded = (w * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (padded * h) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(h) },
            },
            size,
        );
        self.queue.submit([draw, encoder.finish()]);
        (buffer, padded, self.device.clone())
    }

    async fn map(device: &wgpu::Device, buffer: &wgpu::Buffer) -> Result<(), JsValue> {
        let done: Rc<Cell<Option<bool>>> = Rc::new(Cell::new(None));
        let flag = done.clone();
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| flag.set(Some(r.is_ok())));
        loop {
            let _ = device.poll(wgpu::PollType::Poll);
            match done.get() {
                Some(true) => return Ok(()),
                Some(false) => return Err(JsValue::from_str("could not read the frame back")),
                None => tick().await,
            }
        }
    }
}
