mod ui;

use std::{mem, slice};
use winit::window::WindowBuilder;
use winit::event_loop::{EventLoop, ControlFlow};
use winit::event::{Event, WindowEvent, MouseButton, ElementState};
use winit::dpi::PhysicalSize;
use wgpu::util::DeviceExt;
use log::LevelFilter;
use glam::Vec2;
use shared::{Vertex, Consts};
use ui::Context;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;

fn main() -> Result {
  env_logger::builder()
    .filter_level(LevelFilter::Info)
    .filter(Some("wgpu_core"), LevelFilter::Warn)
    .filter(Some("wgpu_hal"), LevelFilter::Warn)
    .init();
  std::panic::set_hook(Box::new(|i| log::error!("{}", i)));
  let event_loop = EventLoop::new()?;
  let window = WindowBuilder::new().build(&event_loop)?;

  let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
  let surface = unsafe { instance.create_surface(&window)? };
  let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
    power_preference: wgpu::PowerPreference::HighPerformance,
    compatible_surface: Some(&surface),
    force_fallback_adapter: false,
  }))
  .unwrap();
  let (device, queue) = pollster::block_on(adapter.request_device(
    &wgpu::DeviceDescriptor {
      features: wgpu::Features::PUSH_CONSTANTS,
      limits: wgpu::Limits {
        max_push_constant_size: 128,
        ..Default::default()
      },
      label: None,
    },
    None,
  ))?;
  resize(&surface, &device, window.inner_size());

  let rt_shader = device.create_shader_module(wgpu::include_spirv!(env!("rt.spv")));
  let rt_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
    label: None,
    bind_group_layouts: &[],
    push_constant_ranges: &[wgpu::PushConstantRange {
      stages: wgpu::ShaderStages::FRAGMENT,
      range: 0..mem::size_of::<Consts>() as _,
    }],
  });
  let rt_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
    layout: Some(&rt_pipeline_layout),
    vertex: wgpu::VertexState {
      module: &rt_shader,
      entry_point: "main_v",
      buffers: &[],
    },
    fragment: Some(wgpu::FragmentState {
      module: &rt_shader,
      entry_point: "main_f",
      targets: &[Some(wgpu::ColorTargetState {
        format: FORMAT,
        blend: None,
        write_mask: wgpu::ColorWrites::ALL,
      })],
    }),
    primitive: wgpu::PrimitiveState::default(),
    depth_stencil: None,
    multisample: wgpu::MultisampleState::default(),
    multiview: None,
    label: None,
  });

  let ui_shader = device.create_shader_module(wgpu::include_spirv!(env!("shaders.spv")));
  let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
    entries: &[
      wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
          multisampled: false,
          view_dimension: wgpu::TextureViewDimension::D2,
          sample_type: wgpu::TextureSampleType::Float { filterable: true },
        },
        count: None,
      },
      wgpu::BindGroupLayoutEntry {
        binding: 1,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
      },
    ],
    label: None,
  });
  let ui_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
    label: None,
    bind_group_layouts: &[&tex_layout],
    push_constant_ranges: &[wgpu::PushConstantRange {
      stages: wgpu::ShaderStages::VERTEX,
      range: 0..mem::size_of::<Consts>() as _,
    }],
  });
  let ui_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
    layout: Some(&ui_pipeline_layout),
    vertex: wgpu::VertexState {
      module: &ui_shader,
      entry_point: "main_v",
      buffers: &[wgpu::VertexBufferLayout {
        array_stride: mem::size_of::<Vertex>() as _,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x3],
      }],
    },
    fragment: Some(wgpu::FragmentState {
      module: &ui_shader,
      entry_point: "main_f",
      targets: &[Some(wgpu::ColorTargetState {
        format: FORMAT,
        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
        write_mask: wgpu::ColorWrites::ALL,
      })],
    }),
    primitive: wgpu::PrimitiveState::default(),
    depth_stencil: None,
    multisample: wgpu::MultisampleState::default(),
    multiview: None,
    label: None,
  });
  let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
    address_mode_u: wgpu::AddressMode::ClampToEdge,
    address_mode_v: wgpu::AddressMode::ClampToEdge,
    address_mode_w: wgpu::AddressMode::ClampToEdge,
    mag_filter: wgpu::FilterMode::Linear,
    min_filter: wgpu::FilterMode::Nearest,
    mipmap_filter: wgpu::FilterMode::Nearest,
    ..Default::default()
  });

  let mut ctx = Context::new();
  ctx.fonts().add_font(include_bytes!("roboto.ttf"), 18.0)?;
  let size = wgpu::Extent3d {
    width: ctx.fonts().size().0,
    height: ctx.fonts().size().1,
    depth_or_array_layers: 1,
  };
  let tex = device.create_texture(&wgpu::TextureDescriptor {
    size,
    mip_level_count: 1,
    sample_count: 1,
    dimension: wgpu::TextureDimension::D2,
    format: wgpu::TextureFormat::Rgba8Unorm,
    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
    view_formats: &[],
    label: None,
  });
  let tex_view = tex.create_view(&wgpu::TextureViewDescriptor::default());
  queue.write_texture(
    wgpu::ImageCopyTexture {
      texture: &tex,
      mip_level: 0,
      origin: wgpu::Origin3d::ZERO,
      aspect: wgpu::TextureAspect::All,
    },
    cast_slice(&ctx.fonts().build_tex()),
    wgpu::ImageDataLayout {
      offset: 0,
      bytes_per_row: Some(4 * size.width),
      rows_per_image: Some(size.height),
    },
    size,
  );
  let tex_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
    layout: &tex_layout,
    entries: &[
      wgpu::BindGroupEntry {
        binding: 0,
        resource: wgpu::BindingResource::TextureView(&tex_view),
      },
      wgpu::BindGroupEntry {
        binding: 1,
        resource: wgpu::BindingResource::Sampler(&sampler),
      },
    ],
    label: None,
  });

  event_loop.run(move |event, elwt| {
    handle_ui_event(&mut ctx, &event);
    match event {
      Event::WindowEvent { event, .. } => match event {
        WindowEvent::Resized(size) => resize(&surface, &device, size),
        WindowEvent::CloseRequested => elwt.exit(),
        WindowEvent::RedrawRequested => {
          let surface = surface.get_current_texture().unwrap();
          let surface_view = surface
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
          let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());

          let mut ui = ctx.begin_frame();
          ui.text("hello floppa");
          if ui.button("button") {
            log::info!("pressed");
          }
          let out = ctx.end_frame();

          let vtx_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            contents: cast_slice(&out.vtx_buf),
            usage: wgpu::BufferUsages::VERTEX,
            label: None,
          });
          let idx_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            contents: cast_slice(&out.idx_buf),
            usage: wgpu::BufferUsages::INDEX,
            label: None,
          });

          let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
              view: &surface_view,
              resolve_target: None,
              ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: true,
              },
            })],
            depth_stencil_attachment: None,
            label: None,
          });
          let consts = Consts {
            screen_size: Vec2::new(surface.texture.width() as _, surface.texture.height() as _),
          };
          render_pass.set_pipeline(&rt_pipeline);
          render_pass.set_push_constants(wgpu::ShaderStages::FRAGMENT, 0, cast(&consts));
          render_pass.draw(0..3, 0..1);

          render_pass.set_pipeline(&ui_pipeline);
          render_pass.set_push_constants(wgpu::ShaderStages::VERTEX, 0, cast(&consts));
          render_pass.set_bind_group(0, &tex_bind_group, &[]);
          render_pass.set_vertex_buffer(0, vtx_buf.slice(..));
          render_pass.set_index_buffer(idx_buf.slice(..), wgpu::IndexFormat::Uint32);
          render_pass.draw_indexed(0..out.idx_buf.len() as _, 0, 0..1);
          drop(render_pass);

          queue.submit([encoder.finish()]);
          surface.present();
        }
        _ => {}
      },
      Event::AboutToWait => window.request_redraw(),
      _ => {}
    }
  })?;
  Ok(())
}

fn resize(surface: &wgpu::Surface, device: &wgpu::Device, size: PhysicalSize<u32>) {
  surface.configure(
    device,
    &wgpu::SurfaceConfiguration {
      usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
      format: FORMAT,
      width: size.width,
      height: size.height,
      present_mode: wgpu::PresentMode::Fifo,
      alpha_mode: wgpu::CompositeAlphaMode::Auto,
      view_formats: vec![],
    },
  );
}

fn handle_ui_event<T>(ctx: &mut Context, event: &Event<T>) {
  let input = ctx.input();
  match event {
    Event::WindowEvent { event, .. } => match event {
      WindowEvent::CursorMoved { position, .. } => {
        input.cursor_pos = Vec2::new(position.x as _, position.y as _);
      }
      WindowEvent::MouseInput { button, state, .. } => {
        input.mouse_buttons[match button {
          MouseButton::Left => 0,
          MouseButton::Middle => 2,
          MouseButton::Right => 3,
          _ => return,
        }] = *state == ElementState::Pressed;
      }
      _ => {}
    },
    _ => {}
  }
}

fn cast_slice<T>(t: &[T]) -> &[u8] {
  unsafe { slice::from_raw_parts(t.as_ptr() as _, mem::size_of_val(t)) }
}

fn cast<T>(t: &T) -> &[u8] {
  cast_slice(slice::from_ref(t))
}
