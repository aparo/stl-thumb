extern crate cgmath;
extern crate image;
extern crate libc;
#[macro_use]
extern crate log;
extern crate mint;

pub mod config;
mod fxaa;
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

fn build_model_uniforms(config: &Config, mesh: &Mesh) -> ModelUniforms {
    let transform_matrix = mesh.scale_and_center();
    let view_matrix = cgmath::Matrix4::look_at_rh(
        CAM_POSITION,
        cgmath::Point3::origin(),
        cgmath::Vector3::unit_z(),
    );
    let perspective_matrix = OPENGL_TO_WGPU
        * cgmath::perspective(
            cgmath::Deg(CAM_FOV_DEG),
            config.width as f32 / config.height as f32,
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

    let uniforms = build_model_uniforms(config, &mesh);
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

    let extent = wgpu::Extent3d {
        width: config.width,
        height: config.height,
        depth_or_array_layers: 1,
    };

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

    let depth_tex = create_depth_texture(&device, config.width, config.height);
    let depth_view = depth_tex.create_view(&Default::default());

    let fxaa_enable = matches!(config.aamethod, AAMethod::FXAA);
    let vertex_count = mesh.vertices.len() as u32;
    let bg = config.background;

    let fxaa = fxaa::FxaaSystem::new(&device, wgpu::TextureFormat::Rgba8Unorm);
    fxaa.draw(
        &device,
        &queue,
        &output_view,
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
                bg,
                vertex_count,
            );
        },
    );

    // Read back pixels from GPU via a staging buffer.
    // wgpu requires row stride to be a multiple of COPY_BYTES_PER_ROW_ALIGNMENT (256).
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let unpadded_row = config.width * 4;
    let padded_row = ((unpadded_row + align - 1) / align) * align;

    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (padded_row * config.height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
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
    staging
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    rx.recv()??;

    let mapped = staging.slice(..).get_mapped_range().unwrap();
    let mut pixels: Vec<u8> = Vec::with_capacity((config.width * config.height * 4) as usize);
    for row in 0..config.height as usize {
        let start = row * padded_row as usize;
        pixels.extend_from_slice(&mapped[start..start + unpadded_row as usize]);
    }
    drop(mapped);
    staging.unmap();

    // wgpu textures are stored top-down, same as image conventions — no flipv needed.
    let img = image::ImageBuffer::from_raw(config.width, config.height, pixels).unwrap();
    Ok(image::DynamicImage::ImageRgba8(img))
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
