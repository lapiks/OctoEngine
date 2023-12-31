use std::{time::Duration, path::Path};

use glam::{Vec3, Vec2, UVec3};
use thiserror::Error;
use winit::event::VirtualKeyCode;

use crate::{
    time_step::TimeStep, 
    system::System, 
    globals::Globals,
    camera::Camera, 
    inputs::Inputs,
    renderer_context::{
        RendererContext, 
        ComputePassDesc, 
        Binding, 
        BindingResource, 
        RenderPassDesc, 
        PipelineDesc, 
        ComputePipelineHandle, 
        RenderPipelineHandle, 
        TextureHandle, 
        Resolution, 
        ShaderHandle, 
        RendererContextError, 
        Frame, 
        BindGroupHandle
    }, 
    file_watcher::FileWatcher, 
    utils::make_relative_path, 
    voxel_world::VoxelWorld, 
};


#[derive(Error, Debug)]
pub enum GameError {
    #[error("Renderer Context error")]
    RendererContextError(#[from] RendererContextError),
}


pub struct Game {
    world: VoxelWorld,
    inputs: Inputs,
    camera: Camera,
    globals: Globals,
    output_texture: TextureHandle,
    time_step: TimeStep,
    compute_shader: Option<ShaderHandle>,
    compute_pipeline: Option<ComputePipelineHandle>,
    compute_bind_group: Option<BindGroupHandle>,
    render_shader: Option<ShaderHandle>,
    render_pipeline: Option<RenderPipelineHandle>,
    render_bind_group: Option<BindGroupHandle>,
    file_watcher: FileWatcher,
}

impl Game {
    pub fn new(renderer: &mut RendererContext) -> Self {
        let file_watcher = Some(FileWatcher::new("./src/shaders", Duration::from_secs(5))).unwrap().unwrap();
        
        let world = VoxelWorld::new(renderer);
        
        let inputs = Inputs::new();

        let camera = Camera::new(
            renderer,
            [0.0, 0.0, -1.0],
            1.0,
        );

        let globals = Globals::new(
            renderer, 
            Vec2::new(800.0, 600.0)
        );

        let output_texture = renderer.new_texture(
            &wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: 800,
                    height: 600,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Uint,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            }
        );

        Game {
            world,
            inputs,
            camera,
            globals,
            output_texture,
            time_step: TimeStep::new(),
            compute_shader: None,
            compute_pipeline : None,
            compute_bind_group: None,
            render_shader: None,
            render_pipeline : None,
            render_bind_group: None,
            file_watcher,
        }
    }

    fn create_shader<P: AsRef<Path>>(renderer: &mut RendererContext, path: P) -> Option<ShaderHandle> {
        let render_shader_src = std::fs::read_to_string(path).unwrap();
        match renderer.new_shader(render_shader_src.as_str()) {
            Ok(shader) => {
                return Some(shader);
            }
            Err(e) => {
                println!("error: {}", e);
            }
        }

        None
    }

    fn create_render_pipeline(renderer: &mut RendererContext, shader: ShaderHandle, globals: &Globals) -> RenderPipelineHandle {
        renderer.new_render_pipeline(
            &PipelineDesc {
                shader: shader,
                bindings_layout: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Uint,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: globals.binding_type(),
                        count: None,
                    },
                ]
            }
        )
    }

    fn create_compute_pipeline(
        renderer: &mut RendererContext, 
        shader: ShaderHandle, 
        world: &VoxelWorld,
        globals: &Globals, 
        camera: &Camera
    ) -> ComputePipelineHandle{
        renderer.new_compute_pipeline(
            &PipelineDesc {
                shader: shader,
                bindings_layout: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: VoxelWorld::binding_type(),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::StorageTexture {
                            access: wgpu::StorageTextureAccess::WriteOnly,
                            format: wgpu::TextureFormat::Rgba8Uint,
                            view_dimension: wgpu::TextureViewDimension::D2,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: globals.binding_type(),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: camera.binding_type(),
                        count: None,
                    },
                ]
            }
        )
    }

    pub fn hot_reload(&mut self, renderer: &mut RendererContext) {
        if let Some(watcher_event) = self.file_watcher.get_event() {
            if let notify::EventKind::Modify(_) = watcher_event.kind {
                for path in watcher_event.paths.iter() {
                    if let Ok(relative_path) = make_relative_path(path) {
                        if let Some(file_stem) = relative_path.file_stem() {
                            if file_stem == "render" {
                                if let Some(render_shader) = self.render_shader {
                                    renderer.destroy_shader(render_shader);
                                }
                                if let Some(render_pipeline) = self.render_pipeline {
                                    renderer.destroy_render_pipeline(render_pipeline);
                                }
                                self.render_shader = Game::create_shader(renderer, "src/shaders/render.wgsl");
                                if let Some(render_shader) = self.render_shader {
                                    self.render_pipeline = Some(Game::create_render_pipeline(renderer, render_shader, &self.globals))
                                }
                            }
                            else if file_stem == "compute" {
                                if let Some(compute_shader) = self.compute_shader {
                                    renderer.destroy_shader(compute_shader);
                                }
                                if let Some(compute_pipeline) = self.compute_pipeline {
                                    renderer.destroy_compute_pipeline(compute_pipeline);
                                }
                                self.compute_shader = Game::create_shader(renderer, "src/shaders/compute.wgsl");
                                if let Some(compute_shader) = self.compute_shader {
                                    self.compute_pipeline = Some(Game::create_compute_pipeline(renderer, compute_shader, &self.world, &self.globals, &self.camera))
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

impl System for Game {
    fn init(&mut self, renderer: &mut RendererContext) {
        self.render_shader = Game::create_shader(renderer, "src/shaders/render.wgsl");
        self.compute_shader = Game::create_shader(renderer,"src/shaders/compute.wgsl");
        if let Some(render_shader) = self.render_shader {
            self.render_pipeline = Some(Game::create_render_pipeline(renderer, render_shader, &self.globals));
        }
        if let Some(compute_shader) = self.compute_shader {
            self.compute_pipeline = Some(Game::create_compute_pipeline(renderer, compute_shader, &self.world, &self.globals, &self.camera));
        }

        for z in 1..self.world.get_size().z-1 {
            for y in 1..self.world.get_size().y-1 {
                for x in 1..self.world.get_size().x-1 {
                    self.world.set_voxel_at(0, &UVec3::new(x, y, z));
                }
            }
        }

        self.world.set_voxel_at(255, &UVec3::new(8, 8, 8));
        self.camera.set_position(Vec3::new(8.0, 8.0, 4.0));
    }

    fn update(&mut self) {
        let delta_time = self.time_step.tick();
        let speed = 5.0;

        if self.inputs.get_key_down(VirtualKeyCode::Z) {
            self.camera.translate(Vec3::Z * delta_time * speed);
        }
        if self.inputs.get_key_down(VirtualKeyCode::S) {
            self.camera.translate(Vec3::NEG_Z * delta_time * speed);
        }
        if self.inputs.get_key_down(VirtualKeyCode::D) {
            self.camera.translate(Vec3::X * delta_time * speed);
        }
        if self.inputs.get_key_down(VirtualKeyCode::Q) {
            self.camera.translate(Vec3::NEG_X * delta_time * speed);
        }
        if self.inputs.get_key_down(VirtualKeyCode::Space) {
            self.camera.translate(Vec3::Y * delta_time * speed);
        }
        if self.inputs.get_key_down(VirtualKeyCode::LControl) {
            self.camera.translate(Vec3::NEG_Y * delta_time * speed);
        }

        self.inputs.reset();
    }

    fn prepare_rendering(&mut self, renderer: &mut RendererContext) {
        self.camera.update_buffer(renderer);
        self.world.update_texture(renderer);
        
        if let Some(compute_bind_group) = self.compute_bind_group {
            renderer.destroy_bind_group(compute_bind_group);
        }
        
        self.compute_bind_group = Some(renderer.new_compute_bind_group(
            self.compute_pipeline.unwrap(), 
            &[
            Binding {
                binding: 0,
                resource: BindingResource::Texture(self.world.get_texture()),
            },
            Binding {
                binding: 1,
                resource: BindingResource::Texture(self.output_texture),
            },
            Binding {
                binding: 2,
                resource: BindingResource::Buffer(self.globals.get_buffer()),
            },
            Binding {
                binding: 3,
                resource: BindingResource::Buffer(self.camera.get_buffer()),
            }]
        ));

        if let Some(render_bind_group) = self.render_bind_group {
            renderer.destroy_bind_group(render_bind_group);
        }

        self.render_bind_group = Some(renderer.new_render_bind_group(
            self.render_pipeline.unwrap(), 
            &[
            Binding {
                binding: 0,
                resource: BindingResource::Texture(self.output_texture),
            },
            Binding {
                binding: 1,
                resource: BindingResource::Buffer(self.globals.get_buffer()),
            }]
        ));
    }

    fn render(&mut self, frame: &mut Frame) {
        // Compute pass
        {
            let mut cpass = frame.begin_compute_pass(
                &ComputePassDesc {
                pipeline: self.compute_pipeline.unwrap(),
                bind_group: self.compute_bind_group.unwrap(),
            });
            let output_size = self.globals.get_size();
            cpass.dispatch(output_size.x as u32, output_size.y as u32, 1);
        }

        // Render pass
        {
            let mut rpass = frame.begin_render_pass(
                &RenderPassDesc {
                pipeline: self.render_pipeline.unwrap(),
                bind_group: self.render_bind_group.unwrap(),
            });
            rpass.draw(0..3, 0..1);
        }
    }

    fn on_key_down(&mut self, key: winit::event::VirtualKeyCode) {
        self.inputs.on_key_down(key);
    }

    fn on_key_up(&mut self, key: winit::event::VirtualKeyCode) {
        self.inputs.on_key_up(key);
    }

    fn on_mouse_button_down(&mut self, button: winit::event::MouseButton) {
        self.inputs.on_mouse_button_down(button);
    }

    fn on_mouse_button_up(&mut self, button: winit::event::MouseButton) {
        self.inputs.on_mouse_button_up(button);
    }

    fn on_mouse_move(&mut self, x_delta: f32, y_delta: f32) {
        self.inputs.on_mouse_move(x_delta, y_delta);
    }

    fn on_mouse_wheel(&mut self, delta: f32) {
        self.inputs.on_mouse_wheel(delta);
    }

    fn resize(&mut self, renderer: &mut RendererContext, width: u32, height: u32) {
        renderer.resize(Resolution { width: width, height: height });
        renderer.update_texture(
            self.output_texture,
            &wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: width,
                    height: height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Uint,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            }
        );
        self.globals.set_size(Vec2::new(width as f32, height as f32));
        self.globals.update_buffer(renderer);
    }
}