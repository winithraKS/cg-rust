use bytemuck::{Pod, Zeroable};
use rayon::prelude::*;
use std::iter;
use wgpu::util::DeviceExt;
use winit::window::Window;

const LOW_RES_WIDTH: u32 = 320;
const LOW_RES_HEIGHT: u32 = 180;
const MAX_ITERATIONS: u32 = 1000;
const PREVIEW_ITERATIONS: u32 = 300;

#[repr(C)]
#[derive(Debug, Copy, Clone, Pod, Zeroable)]
struct ViewParams {
    center: [f32; 2],
    range: [f32; 2],
    screen_dims: [u32; 2],
}

pub struct State {
    surface: wgpu::Surface,
    pub device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pub size: winit::dpi::PhysicalSize<u32>,
    pub window: Window,

    render_pipeline: wgpu::RenderPipeline,
    compute_pipeline: wgpu::ComputePipeline,

    view_params: ViewParams,
    view_params_buffer: wgpu::Buffer,
    high_res_texture: wgpu::Texture,
    low_res_texture: wgpu::Texture,
    texture_sampler: wgpu::Sampler,

    high_res_render_bind_group: wgpu::BindGroup,
    low_res_render_bind_group: wgpu::BindGroup,
    compute_bind_group: wgpu::BindGroup,

    show_low_res: bool,
}

impl State {
    pub async fn new(window: Window) -> Self {
        let size = window.inner_size();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
        let surface = unsafe { instance.create_surface(&window) }.unwrap();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .unwrap();

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("Main Device"),
                    features: wgpu::Features::empty(),
                    limits: wgpu::Limits::default(),
                },
                None,
            )
            .await
            .unwrap();

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps.formats[0];
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let render_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Render Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("./render.wgsl").into()),
        });
        let compute_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Compute Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("./compute.wgsl").into()),
        });

        let texture_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Texture Sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let high_res_texture = create_texture(&device, size.width, size.height, "High-Res Texture", wgpu::TextureUsages::STORAGE_BINDING);
        let low_res_texture = create_texture(&device, LOW_RES_WIDTH, LOW_RES_HEIGHT, "Low-Res Texture", wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);

        let view_params = ViewParams {
            center: [-0.5, 0.0],
            range: [3.5, 2.0],
            screen_dims: [size.width, size.height],
        };

        let view_params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("View Params Buffer"),
            contents: bytemuck::bytes_of(&view_params),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let compute_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Compute Bind Group Layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::StorageTexture {
                            access: wgpu::StorageTextureAccess::WriteOnly,
                            format: wgpu::TextureFormat::Rgba8Unorm,
                            view_dimension: wgpu::TextureViewDimension::D2,
                        },
                        count: None,
                    },
                ],
            });

        let high_res_texture_view = high_res_texture.create_view(&wgpu::TextureViewDescriptor::default());

        // TODO: Create the compute bind group
        // This connects the shader's @group(0) bindings to actual GPU resources
        // You need to bind:
        //   - binding 0: view_params_buffer (uniform buffer with view parameters)
        //   - binding 1: high_res_texture_view (storage texture for output)
        let compute_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Compute Bind Group"),
            layout: &compute_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: view_params_buffer.as_entire_binding(),
                    
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&high_res_texture_view),
                },
            ],
        });

        // TODO: Create the compute pipeline layout
        // This defines the overall structure of bind groups used by the pipeline
        let compute_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Compute Pipeline Layout"),
                bind_group_layouts: &[&compute_bind_group_layout],
                push_constant_ranges: &[],
            });

        let compute_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Compute Pipeline"),
            layout: Some(&compute_pipeline_layout),
            module: &compute_shader,
            entry_point: "main",
        });

        let render_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Render Bind Group Layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
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
                ],
            });

        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Render Pipeline Layout"),
                bind_group_layouts: &[&render_bind_group_layout],
                push_constant_ranges: &[],
            });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &render_shader,
                entry_point: "vs_main",
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &render_shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let low_res_texture_view = low_res_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let low_res_render_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Low-Res Render Bind Group"),
            layout: &render_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(&texture_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&low_res_texture_view),
                },
            ],
        });

        let high_res_render_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("High-Res Render Bind Group"),
            layout: &render_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(&texture_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&high_res_texture_view),
                },
            ],
        });


        let mut s = Self {
            window,
            surface,
            device,
            queue,
            config,
            size,
            render_pipeline,
            compute_pipeline,
            view_params,
            view_params_buffer,
            high_res_texture,
            low_res_texture,
            texture_sampler,
            high_res_render_bind_group,
            low_res_render_bind_group,
            compute_bind_group,
            show_low_res: false,
        };

        s.trigger_render(false);

        let preview_params = ViewParams {
            screen_dims: [LOW_RES_WIDTH, LOW_RES_HEIGHT],
            ..s.view_params
        };
        let low_res_pixels = compute_cpu_preview(&preview_params);
        s.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &s.low_res_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &low_res_pixels,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4 * LOW_RES_WIDTH),
                rows_per_image: Some(LOW_RES_HEIGHT),
            },
            wgpu::Extent3d {
                width: LOW_RES_WIDTH,
                height: LOW_RES_HEIGHT,
                depth_or_array_layers: 1,
            },
        );
        s.show_low_res = true;

        s
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);

            self.high_res_texture = create_texture(&self.device, self.size.width, self.size.height, "High-Res Texture", wgpu::TextureUsages::STORAGE_BINDING);
            let high_res_texture_view = self.high_res_texture.create_view(&wgpu::TextureViewDescriptor::default());

            let render_bind_group_layout = self.render_pipeline.get_bind_group_layout(0);
            self.high_res_render_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("High-Res Render Bind Group"),
                layout: &render_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Sampler(&self.texture_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&high_res_texture_view),
                    },
                ],
            });

            let compute_bind_group_layout = self.compute_pipeline.get_bind_group_layout(0);
            self.compute_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Compute Bind Group"),
                layout: &compute_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.view_params_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&high_res_texture_view),
                    },
                ],
            });


            self.view_params.screen_dims = [new_size.width, new_size.height];
            self.trigger_render(false);
        }
    }

    fn trigger_render(&mut self, with_preview: bool) {
        if with_preview {
            let preview_params = ViewParams {
                screen_dims: [LOW_RES_WIDTH, LOW_RES_HEIGHT],
                ..self.view_params
            };
            let low_res_pixels = compute_cpu_preview(&preview_params);

            self.queue.write_texture(
                wgpu::ImageCopyTexture {
                    texture: &self.low_res_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &low_res_pixels,
                wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * LOW_RES_WIDTH),
                    rows_per_image: Some(LOW_RES_HEIGHT),
                },
                wgpu::Extent3d {
                    width: LOW_RES_WIDTH,
                    height: LOW_RES_HEIGHT,
                    depth_or_array_layers: 1,
                },
            );
            self.show_low_res = true;
        }

        self.view_params.screen_dims = [self.size.width, self.size.height];
        self.queue.write_buffer(
            &self.view_params_buffer,
            0,
            bytemuck::bytes_of(&self.view_params),
        );

        // TODO: Execute the compute shader on the GPU
        // Step 1: Create a command encoder to record GPU commands
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("Compute Encoder") });

        // Step 2: Begin a compute pass (this is where compute shaders run)
        let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("Compute Pass"), ..Default::default() });

        // TODO: Set the compute pipeline and bind group
        // Hint: Use compute_pass.set_pipeline() and compute_pass.set_bind_group()
        compute_pass.set_pipeline(&self.compute_pipeline);
        compute_pass.set_bind_group(0, &self.compute_bind_group, &[]);

        // TODO: Calculate the number of workgroups needed
        // The compute shader uses @workgroup_size(8, 8, 1)
        // We need enough workgroups to cover the entire screen
        let workgroup_x = (self.size.width as f32 / 8.0).ceil() as u32;
        let workgroup_y = (self.size.height as f32 / 8.0).ceil() as u32;

        // TODO: Dispatch the compute shader with the calculated workgroup counts
        compute_pass.dispatch_workgroups(workgroup_x, workgroup_y, 1);

        // End the compute pass and submit commands to GPU
        drop(compute_pass);
        self.queue.submit(iter::once(encoder.finish()));
    }


    pub fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        let output_frame = self.surface.get_current_texture()?;
        let view = output_frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("Render Encoder") });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: true,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });

            render_pass.set_pipeline(&self.render_pipeline);

            if self.show_low_res {
                render_pass.set_bind_group(0, &self.low_res_render_bind_group, &[]);
                self.show_low_res = false;
            } else {
                render_pass.set_bind_group(0, &self.high_res_render_bind_group, &[]);
            }

            render_pass.draw(0..6, 0..1);
        }

        self.queue.submit(iter::once(encoder.finish()));
        output_frame.present();

        Ok(())
    }
}

fn create_texture(device: &wgpu::Device, width: u32, height: u32, label: &str, usage: wgpu::TextureUsages) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: usage | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

fn hsv_to_rgb_u8(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    if s == 0.0 { let val = (v * 255.0) as u8; return (val, val, val); }
    let h_sector = h / 60.0;
    let i = h_sector.floor() as i32;
    let f = h_sector - i as f32;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    let (r, g, b) = match i {
        0 => (v, t, p), 1 => (q, v, p), 2 => (p, v, t),
        3 => (p, q, v), 4 => (t, p, v), _ => (v, p, q),
    };
    ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

fn compute_cpu_preview(params: &ViewParams) -> Vec<u8> {
    let width = params.screen_dims[0];
    let height = params.screen_dims[1];
    let mut pixels = vec![0u8; (width * height * 4) as usize];

    pixels.par_chunks_mut((width * 4) as usize).enumerate().for_each(|(y, row)| {
        for x in 0..width {
            let norm_x = x as f32 / width as f32 - 0.5;
            let norm_y = y as f32 / height as f32 - 0.5;

            let (c_real, c_imag) = (
                params.center[0] + (norm_x * params.range[0]),
                params.center[1] + (norm_y * params.range[1]),
            );

            // TODO: Implement the Mandelbrot iteration on CPU (same logic as GPU shader)
            // This provides a quick preview using Rayon for parallel CPU processing
            let (mut z_real, mut z_imag) = (0.0, 0.0);

            let mut iterations = 0;
            // TODO: Implement the while loop to iterate the Mandelbrot formula
            // Same logic as in compute.wgsl: z_{n+1} = z_n^2 + c
            // Hint: Loop while |z|^2 <= 4.0 and iterations < PREVIEW_ITERATIONS
            while z_real * z_real + z_imag * z_imag <= 4.0 && iterations < PREVIEW_ITERATIONS {
                let z_real_new = z_real * z_real - z_imag * z_imag + c_real;
                z_imag = 2.0 * z_real * z_imag + c_imag;
                z_real = z_real_new;
                iterations += 1;
            }

            // TODO: Calculate the color based on iteration count (same as GPU shader)
            let (r, g, b) = if iterations == PREVIEW_ITERATIONS {
                // In the set - use angle-based coloring
                let angle = z_imag.atan2(z_real);
                let hue_norm = (angle + std::f32::consts::PI) / (2.0 * std::f32::consts::PI);
                let hue = hue_norm * 360.0;
                hsv_to_rgb_u8(hue, 1.0, 1.0)
            } else {
                // Escaped -> use iteration count
                let hue = (iterations as f32 / PREVIEW_ITERATIONS as f32) * 360.0;
                hsv_to_rgb_u8(hue, 1.0, 1.0)
            };

            let idx = (x * 4) as usize;
            row[idx] = r; row[idx + 1] = g; row[idx + 2] = b; row[idx + 3] = 255;
        }
    });
    pixels
}