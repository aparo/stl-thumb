extern crate cgmath;
extern crate image;
extern crate libc;
#[macro_use]
extern crate log;
extern crate mint;

pub mod config;
mod fxaa;
pub mod info;
mod mesh;

use bytemuck::{Pod, Zeroable};
use cgmath::EuclideanSpace;
use config::{AAMethod, Config};
use image::{ImageEncoder, ImageFormat};
use libc::c_char;
use mesh::Mesh;
use pollster::block_on;
use std::error::Error;
use std::ffi::CStr;
use std::sync::Arc;
use std::{cell::RefCell, io, slice, thread};
use wgpu::util::DeviceExt;
use winit::dpi::PhysicalSize;
use winit::event::{Event, WindowEvent};
use winit::event_loop::EventLoop;
use winit::window::WindowAttributes;

const CAM_FOV_DEG: f32 = 30.0;
const CAM_POSITION: cgmath::Point3<f32> = cgmath::Point3 { x: 2.0, y: -4.0, z: 2.0 };

// Converts cgmath::perspective output (z in [-1,1]) to wgpu NDC (z in [0,1]).
// Column-major arguments: (c0r0, c0r1, c0r2, c0r3, c1r0, ...)
#[rustfmt::skip]
const OPENGL_TO_WGPU: cgmath::Matrix4<f32> = cgmath::Matrix4::new(
    1.0, 0.0, 0.0, 0.0,
    0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, 0.5, 0.0,
    0.0, 0.0, 0.5, 1.0,
);

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct ModelUniforms {
    modelview:      [[f32; 4]; 4],
    perspective:    [[f32; 4]; 4],
    u_light:        [f32; 3],
    _pad0:          f32,
    ambient_color:  [f32; 3],
    _pad1:          f32,
    diffuse_color:  [f32; 3],
    _pad2:          f32,
    specular_color: [f32; 3],
    _pad3:          f32,
}

thread_local! {
    static EVENT_LOOP: RefCell<Option<EventLoop<()>>> = RefCell::new(None);
}

fn create_event_loop_once() -> EventLoop<()> {
    #[cfg(target_os = "linux")]
    {
        use winit::platform::x11::EventLoopBuilderExtX11;
        if let Ok(el) = EventLoop::builder().with_any_thread(true).build() {
            return el;
        }
    }
    #[cfg(target_os = "linux")]
    {
        use winit::platform::wayland::EventLoopBuilderExtWayland;
        if let Ok(el) = EventLoop::builder().with_any_thread(true).build() {
            return el;
        }
    }
    EventLoop::builder()
        .build()
        .expect("Failed to create event loop")
}

fn create_depth_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
}

fn build_model_pipeline(
    device: &wgpu::Device,
    color_format: wgpu::TextureFormat,
) -> (wgpu::RenderPipeline, wgpu::BindGroupLayout) {
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&bind_group_layout)],
        immediate_size: 0,
    });

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(include_str!("shaders/model.wgsl").into()),
    });

    let pos_layout = wgpu::VertexBufferLayout {
        array_stride: 12,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[wgpu::VertexAttribute {
            offset: 0,
            shader_location: 0,
            format: wgpu::VertexFormat::Float32x3,
        }],
    };
    let norm_layout = wgpu::VertexBufferLayout {
        array_stride: 12,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[wgpu::VertexAttribute {
            offset: 0,
            shader_location: 1,
            format: wgpu::VertexFormat::Float32x3,
        }],
    };

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[Some(pos_layout), Some(norm_layout)],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: color_format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });

    (pipeline, bind_group_layout)
}

fn build_model_uniforms_for_camera(
    config: &Config,
    mesh: &Mesh,
    cam_pos: cgmath::Point3<f32>,
    cam_up: cgmath::Vector3<f32>,
    tile_w: u32,
    tile_h: u32,
) -> ModelUniforms {
    let transform_matrix = mesh.scale_and_center();
    let view_matrix = cgmath::Matrix4::look_at_rh(cam_pos, cgmath::Point3::origin(), cam_up);
    let perspective_matrix = OPENGL_TO_WGPU
        * cgmath::perspective(
            cgmath::Deg(CAM_FOV_DEG),
            tile_w as f32 / tile_h as f32,
            0.1,
            1024.0,
        );
    ModelUniforms {
        modelview:      (view_matrix * transform_matrix).into(),
        perspective:    perspective_matrix.into(),
        u_light:        [-1.1, 0.4, 1.0],
        _pad0:          0.0,
        ambient_color:  config.material.ambient,
        _pad1:          0.0,
        diffuse_color:  config.material.diffuse,
        _pad2:          0.0,
        specular_color: config.material.specular,
        _pad3:          0.0,
    }
}

fn build_model_uniforms(config: &Config, mesh: &Mesh) -> ModelUniforms {
    build_model_uniforms_for_camera(
        config,
        mesh,
        CAM_POSITION,
        cgmath::Vector3::unit_z(),
        config.width,
        config.height,
    )
}

fn model_render_pass(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    bind_group: &wgpu::BindGroup,
    vertex_buf: &wgpu::Buffer,
    normal_buf: &wgpu::Buffer,
    target_view: &wgpu::TextureView,
    depth_view: &wgpu::TextureView,
    background: (f32, f32, f32, f32),
    vertex_count: u32,
) {
    let (r, g, b, a) = background;
    let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: None,
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target_view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color {
                    r: r as f64,
                    g: g as f64,
                    b: b as f64,
                    a: a as f64,
                }),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: depth_view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Discard,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    rpass.set_pipeline(pipeline);
    rpass.set_bind_group(0, bind_group, &[]);
    rpass.set_vertex_buffer(0, vertex_buf.slice(..));
    rpass.set_vertex_buffer(1, normal_buf.slice(..));
    rpass.draw(0..vertex_count, 0..1);
}

fn create_headless_device() -> (wgpu::Device, wgpu::Queue) {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .expect("No suitable wgpu adapter found");
    info!("Adapter: {:?}", adapter.get_info());
    block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: None,
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .expect("Failed to create wgpu device")
}

/// Draw 2-pixel gray separator lines at the tile boundaries of a 2×2 grid.
fn draw_separator_lines(img: &mut image::RgbaImage, tile_w: u32, tile_h: u32) {
    let color = image::Rgba([180u8, 180, 180, 255]);
    for dx in 0..2u32 {
        let x = tile_w + dx;
        if x < img.width() {
            for y in 0..img.height() {
                img.put_pixel(x, y, color);
            }
        }
    }
    for dy in 0..2u32 {
        let y = tile_h + dy;
        if y < img.height() {
            for x in 0..img.width() {
                img.put_pixel(x, y, color);
            }
        }
    }
}

/// Draw a centered view-name label at the top of a grid tile using an 8×8 bitmap
/// font scaled 2× so it reads comfortably at typical thumbnail resolutions.
fn draw_tile_label(img: &mut image::RgbaImage, text: &str, tile_x: u32, tile_y: u32, tile_w: u32) {
    use font8x8::UnicodeFonts;

    const SCALE: u32 = 2;
    const CHAR_W: u32 = 8 * SCALE;
    const CHAR_H: u32 = 8 * SCALE;
    const PAD: u32 = 3;
    let bar_h = CHAR_H + PAD * 2;

    // Darken the strip behind the text to ensure contrast on any background.
    for py in tile_y..tile_y + bar_h {
        for px in tile_x..tile_x + tile_w {
            if px < img.width() && py < img.height() {
                let p = img.get_pixel_mut(px, py);
                p[0] = (p[0] as u32 * 35 / 100) as u8;
                p[1] = (p[1] as u32 * 35 / 100) as u8;
                p[2] = (p[2] as u32 * 35 / 100) as u8;
                // Ensure the bar is visible even on a fully-transparent background.
                p[3] = p[3].max(200);
            }
        }
    }

    // Center the text block horizontally inside the tile.
    let text_chars = text.chars().count() as u32;
    let text_px_w = text_chars * CHAR_W;
    let text_x = tile_x + tile_w.saturating_sub(text_px_w) / 2;
    let text_y = tile_y + PAD;
    let white = image::Rgba([255u8, 255, 255, 255]);

    for (i, ch) in text.chars().enumerate() {
        if let Some(glyph) = font8x8::BASIC_FONTS.get(ch) {
            for (row, &bits) in glyph.iter().enumerate() {
                for col in 0u32..8 {
                    if bits & (1u8 << col) != 0 {
                        for sy in 0..SCALE {
                            for sx in 0..SCALE {
                                let px = text_x + i as u32 * CHAR_W + col * SCALE + sx;
                                let py = text_y + row as u32 * SCALE + sy;
                                if px < img.width() && py < img.height() {
                                    img.put_pixel(px, py, white);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Render a single view into raw RGBA pixels using the given camera uniforms.
fn render_tile(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &wgpu::RenderPipeline,
    model_bgl: &wgpu::BindGroupLayout,
    vertex_buf: &wgpu::Buffer,
    normal_buf: &wgpu::Buffer,
    fxaa: &fxaa::FxaaSystem,
    uniforms: &ModelUniforms,
    tile_w: u32,
    tile_h: u32,
    background: (f32, f32, f32, f32),
    fxaa_enable: bool,
    vertex_count: u32,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::bytes_of(uniforms),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: model_bgl,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform_buf.as_entire_binding() }],
    });

    let extent = wgpu::Extent3d { width: tile_w, height: tile_h, depth_or_array_layers: 1 };
    let output_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let output_view = output_tex.create_view(&Default::default());
    let depth_tex = create_depth_texture(device, tile_w, tile_h);
    let depth_view = depth_tex.create_view(&Default::default());

    fxaa.draw(device, queue, &output_view, tile_w, tile_h, fxaa_enable, |intermediate, encoder| {
        model_render_pass(encoder, pipeline, &bind_group, vertex_buf, normal_buf, intermediate, &depth_view, background, vertex_count);
    });

    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let unpadded_row = tile_w * 4;
    let padded_row = ((unpadded_row + align - 1) / align) * align;

    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (padded_row * tile_h) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    encoder.copy_texture_to_buffer(
        output_tex.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &staging,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row),
                rows_per_image: None,
            },
        },
        extent,
    );
    queue.submit(Some(encoder.finish()));

    let (tx, rx) = std::sync::mpsc::channel();
    staging.slice(..).map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    rx.recv()??;

    let mapped = staging.slice(..).get_mapped_range().unwrap();
    let mut pixels: Vec<u8> = Vec::with_capacity((tile_w * tile_h * 4) as usize);
    for row in 0..tile_h as usize {
        let start = row * padded_row as usize;
        pixels.extend_from_slice(&mapped[start..start + unpadded_row as usize]);
    }
    drop(mapped);
    staging.unmap();

    Ok(pixels)
}

pub fn render_to_window(config: Config) -> Result<(), Box<dyn Error>> {
    let mesh = Mesh::load(&config.model_filename, config.recalc_normals)?;

    let event_loop = EVENT_LOOP.with(|cell| {
        cell.borrow_mut().take().unwrap_or_else(create_event_loop_once)
    });

    let window_dim = PhysicalSize::new(config.width, config.height);
    let window = Arc::new(event_loop.create_window(
        WindowAttributes::default()
            .with_title("stl-thumb")
            .with_inner_size(window_dim)
            .with_min_inner_size(window_dim)
            .with_max_inner_size(window_dim)
            .with_visible(config.visible),
    )?);

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());

    let surface = instance.create_surface(Arc::clone(&window))?;

    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        compatible_surface: Some(&surface),
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .expect("No suitable wgpu adapter found");
    info!("Adapter: {:?}", adapter.get_info());

    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: None,
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))?;

    let surface_caps = surface.get_capabilities(&adapter);
    let surface_format = surface_caps
        .formats
        .iter()
        .find(|f| f.is_srgb())
        .copied()
        .unwrap_or(surface_caps.formats[0]);

    surface.configure(
        &device,
        &wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            color_space: wgpu::SurfaceColorSpace::default(),
            width: config.width,
            height: config.height,
            present_mode: surface_caps.present_modes[0],
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        },
    );

    let uniforms = build_model_uniforms(&config, &mesh);
    let uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::bytes_of(&uniforms),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let vertex_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&mesh.vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let normal_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&mesh.normals),
        usage: wgpu::BufferUsages::VERTEX,
    });

    let (model_pipeline, model_bgl) =
        build_model_pipeline(&device, wgpu::TextureFormat::Rgba8Unorm);
    let model_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &model_bgl,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform_buf.as_entire_binding(),
        }],
    });

    let fxaa = fxaa::FxaaSystem::new(&device, surface_format);
    let fxaa_enable = matches!(config.aamethod, AAMethod::FXAA);
    let vertex_count = mesh.vertices.len() as u32;
    let depth_tex = create_depth_texture(&device, config.width, config.height);
    let depth_view = depth_tex.create_view(&Default::default());

    event_loop.run(move |event, elwt| match event {
        Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } => elwt.exit(),

        Event::WindowEvent {
            event: WindowEvent::RedrawRequested,
            ..
        } => {
            let surface_tex = match surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
                _ => return,
            };
            let surface_view = surface_tex.texture.create_view(&Default::default());

            fxaa.draw(
                &device,
                &queue,
                &surface_view,
                config.width,
                config.height,
                fxaa_enable,
                |intermediate_view, encoder| {
                    model_render_pass(
                        encoder,
                        &model_pipeline,
                        &model_bind_group,
                        &vertex_buf,
                        &normal_buf,
                        intermediate_view,
                        &depth_view,
                        config.background,
                        vertex_count,
                    );
                },
            );

            queue.present(surface_tex);
        }

        Event::AboutToWait => {
            window.request_redraw();
        }

        _ => {}
    })?;

    Ok(())
}

pub fn render_to_image(config: &Config) -> Result<image::DynamicImage, Box<dyn Error>> {
    let mesh = Mesh::load(&config.model_filename, config.recalc_normals)?;
    let (device, queue) = create_headless_device();

    let vertex_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&mesh.vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let normal_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&mesh.normals),
        usage: wgpu::BufferUsages::VERTEX,
    });

    let (model_pipeline, model_bgl) =
        build_model_pipeline(&device, wgpu::TextureFormat::Rgba8Unorm);
    let fxaa = fxaa::FxaaSystem::new(&device, wgpu::TextureFormat::Rgba8Unorm);
    let fxaa_enable = matches!(config.aamethod, AAMethod::FXAA);
    let vertex_count = mesh.vertices.len() as u32;
    let bg = config.background;

    if config.multi_view {
        // 2×2 grid: isometric (top-left) | front (top-right)
        //             top     (bot-left)  | side  (bot-right)
        let tile_w = config.width / 2;
        let tile_h = config.height / 2;

        type Cam = (cgmath::Point3<f32>, cgmath::Vector3<f32>);
        let views: [Cam; 4] = [
            // isometric — same as default single-view camera
            (cgmath::Point3::new(2.0, -4.0, 2.0), cgmath::Vector3::unit_z()),
            // front — looking along -Y toward origin, Z up
            (cgmath::Point3::new(0.0, -5.0, 0.0), cgmath::Vector3::unit_z()),
            // top — looking down from +Z, Y up in image
            (cgmath::Point3::new(0.0, 0.0, 5.0), cgmath::Vector3::new(0.0, 1.0, 0.0)),
            // side — looking from +X, Z up
            (cgmath::Point3::new(5.0, 0.0, 0.0), cgmath::Vector3::unit_z()),
        ];

        let mut tiles: Vec<image::RgbaImage> = Vec::with_capacity(4);
        for (cam_pos, cam_up) in &views {
            let uniforms = build_model_uniforms_for_camera(config, &mesh, *cam_pos, *cam_up, tile_w, tile_h);
            let pixels = render_tile(&device, &queue, &model_pipeline, &model_bgl, &vertex_buf, &normal_buf, &fxaa, &uniforms, tile_w, tile_h, bg, fxaa_enable, vertex_count)?;
            tiles.push(image::ImageBuffer::from_raw(tile_w, tile_h, pixels).unwrap());
        }

        let offsets: [(u32, u32); 4] = [(0, 0), (tile_w, 0), (0, tile_h), (tile_w, tile_h)];
        let labels = ["Isometric", "Front", "Top", "Side"];

        let mut grid: image::RgbaImage = image::ImageBuffer::new(config.width, config.height);
        for (tile, &(ox, oy)) in tiles.iter().zip(offsets.iter()) {
            image::imageops::replace(&mut grid, tile, ox as i64, oy as i64);
        }

        // Grid separator lines are always drawn in multi-view mode.
        draw_separator_lines(&mut grid, tile_w, tile_h);

        if config.label {
            for (&(ox, oy), label) in offsets.iter().zip(labels.iter()) {
                draw_tile_label(&mut grid, label, ox, oy, tile_w);
            }
        }

        Ok(image::DynamicImage::ImageRgba8(grid))
    } else {
        let uniforms = build_model_uniforms(config, &mesh);
        let pixels = render_tile(&device, &queue, &model_pipeline, &model_bgl, &vertex_buf, &normal_buf, &fxaa, &uniforms, config.width, config.height, bg, fxaa_enable, vertex_count)?;
        let img = image::ImageBuffer::from_raw(config.width, config.height, pixels).unwrap();
        Ok(image::DynamicImage::ImageRgba8(img))
    }
}

pub fn render_to_file(config: &Config) -> Result<(), Box<dyn Error>> {
    let img = render_to_image(config)?;

    let mut output: Box<dyn io::Write> = match config.img_filename.as_str() {
        "-" => Box::new(io::stdout()),
        _ => Box::new(std::fs::File::create(&config.img_filename).unwrap()),
    };

    let mut buff: Vec<u8> = Vec::new();
    let mut cursor = io::Cursor::new(&mut buff);

    match config.format {
        ImageFormat::Png => {
            let encoder = image::codecs::png::PngEncoder::new_with_quality(
                &mut cursor,
                image::codecs::png::CompressionType::Fast,
                image::codecs::png::FilterType::Adaptive,
            );
            encoder.write_image(
                img.as_bytes(),
                config.width,
                config.height,
                img.color().into(),
            )?;
        }
        _ => img.write_to(&mut cursor, config.format.to_owned())?,
    }

    output.write_all(&buff)?;
    output.flush()?;

    Ok(())
}

#[no_mangle]
pub unsafe extern "C" fn render_to_buffer(
    buf_ptr: *mut u8,
    width: u32,
    height: u32,
    model_filename_c: *const c_char,
) -> bool {
    if buf_ptr.is_null() {
        error!("Image buffer pointer is null");
        return false;
    };

    let buf_size = (width * height * 4) as usize;
    let buf = unsafe { slice::from_raw_parts_mut(buf_ptr, buf_size) };

    let model_filename_cstr = unsafe {
        if model_filename_c.is_null() {
            error!("model file path pointer is null");
            return false;
        }
        CStr::from_ptr(model_filename_c)
    };

    let model_filename_str = match model_filename_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            error!("Invalid model file path {:?}", model_filename_cstr);
            return false;
        }
    };

    let config = Config {
        model_filename: model_filename_str.to_string(),
        width,
        height,
        ..Default::default()
    };

    let render_thread = thread::spawn(move || render_to_image(&config).unwrap());
    let img = match render_thread.join() {
        Ok(s) => s,
        Err(e) => {
            error!("Application error: {:?}", e);
            return false;
        }
    };

    match img.as_rgba8() {
        Some(s) => buf.copy_from_slice(s),
        None => {
            error!("Unable to get image");
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::ErrorKind;

    #[test]
    fn cube_stl() {
        let img_filename = "cube-stl.png".to_string();
        let config = Config {
            model_filename: "test_data/cube.stl".to_string(),
            img_filename: img_filename.clone(),
            format: image::ImageFormat::Png,
            ..Default::default()
        };
        match fs::remove_file(&img_filename) {
            Ok(_) => (),
            Err(ref error) if error.kind() == ErrorKind::NotFound => (),
            Err(_) => panic!("Couldn't clean files before testing"),
        }
        render_to_file(&config).expect("Error in render function");
        let size = fs::metadata(img_filename).expect("No file created").len();
        assert_ne!(0, size);
    }

    #[test]
    fn cube_obj() {
        let img_filename = "cube-obj.png".to_string();
        let config = Config {
            model_filename: "test_data/cube.obj".to_string(),
            img_filename: img_filename.clone(),
            format: image::ImageFormat::Png,
            ..Default::default()
        };
        match fs::remove_file(&img_filename) {
            Ok(_) => (),
            Err(ref error) if error.kind() == ErrorKind::NotFound => (),
            Err(_) => panic!("Couldn't clean files before testing"),
        }
        render_to_file(&config).expect("Error in render function");
        let size = fs::metadata(img_filename).expect("No file created").len();
        assert_ne!(0, size);
    }

    #[test]
    fn cube_3mf() {
        let img_filename = "cube-3mf.png".to_string();
        let config = Config {
            model_filename: "test_data/cube.3mf".to_string(),
            img_filename: img_filename.clone(),
            format: image::ImageFormat::Png,
            ..Default::default()
        };
        match fs::remove_file(&img_filename) {
            Ok(_) => (),
            Err(ref error) if error.kind() == ErrorKind::NotFound => (),
            Err(_) => panic!("Couldn't clean files before testing"),
        }
        render_to_file(&config).expect("Error in render function");
        let size = fs::metadata(img_filename).expect("No file created").len();
        assert_ne!(0, size);
    }
}
