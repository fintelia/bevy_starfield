use bevy::{
    core_pipeline::core_3d::Opaque3d,
    ecs::{
        query::WorldQuery,
        system::{lifetimeless::Read, SystemParam, SystemState},
    },
    pbr::SetMeshViewBindGroup,
    prelude::*,
    reflect::TypeUuid,
    render::{
        extract_resource::ExtractResource,
        render_phase::{
            AddRenderCommand, DrawFunctions, PhaseItem, RenderCommand, RenderCommandResult,
            RenderPhase, SetItemPipeline, TrackedRenderPass,
        },
        render_resource::{
            BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout,
            BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingResource, BindingType, Buffer,
            BufferBinding, BufferBindingType, BufferInitDescriptor, BufferUsages, ColorTargetState,
            ColorWrites, CompareFunction, DepthStencilState, FragmentState, FrontFace,
            MultisampleState, PipelineCache, PolygonMode, PrimitiveState, PrimitiveTopology,
            RenderPipelineDescriptor, ShaderStages, ShaderType, SpecializedRenderPipeline,
            SpecializedRenderPipelines, TextureFormat, UniformBuffer, VertexState,
        },
        renderer::{RenderDevice, RenderQueue},
        texture::DefaultImageSampler,
        view::{ViewUniformOffset, ViewUniforms},
        Extract, RenderApp, RenderSet,
    },
};
use std::num::NonZeroU64;

mod astro;

/// Conversion between game units and astronomical ones.
#[derive(Clone, Resource)]
pub struct GameUnitsToCelestial {
    /// The matrix that transforms world space coordinates into ECEF coordinates.
    pub world_to_ecef: Mat3,
    /// The [Julian date](https://en.wikipedia.org/wiki/Julian_date) of the start of the game.
    ///
    /// Defaults to 2451545.0 which corresponds to midnight on January 1st, 2000.
    pub initial_julian_date: f64,
    /// Scale factor between the game's time and the real world's time.
    ///
    /// Defaults to 1.0. Set to 0.0 to have stars stop moving, or to large values to have stars
    /// move quickly across the sky.
    pub time_scale: f64,
}
impl GameUnitsToCelestial {
    /// Initialize `world_to_ecef` by specifying the location of the world space origin in
    /// [geodetic coordinates](https://en.wikipedia.org/wiki/Geodetic_coordinates)).
    pub fn with_origin_coordinates(self, latitude: f32, longitude: f32) -> Self {
        let latitude = latitude.to_radians();
        let longitude = longitude.to_radians();

        let sin_latitude = latitude.sin();
        let cos_latitude = latitude.cos();
        let sin_longitude = longitude.sin();
        let cos_longitude = longitude.cos();

        #[rustfmt::skip]
        let world_to_ecef = Mat3::from_cols_array(&[
            -sin_latitude, -sin_longitude * cos_latitude, cos_latitude * cos_longitude,
            cos_latitude,  -sin_longitude * sin_latitude, cos_latitude * sin_longitude,
            0.0,           cos_longitude,                 sin_longitude,
        ]).transpose();

        Self {
            world_to_ecef,
            ..self
        }
    }
}
impl Default for GameUnitsToCelestial {
    fn default() -> Self {
        Self {
            world_to_ecef: Mat3::IDENTITY,
            time_scale: 1.0,
            initial_julian_date: 2451545.0,
        }
    }
}

type DrawStarfield = (
    SetItemPipeline,
    SetMeshViewBindGroup<0>,
    StarfieldRenderCommand,
);

#[derive(Default, Clone, Resource, ExtractResource, Reflect, ShaderType)]
#[reflect(Resource)]
struct StarfieldUniform {
    pub world_to_ecef: Mat3,
    pub sidereal_time: f32,
}

#[derive(Resource, Default)]
struct StarfieldUniformBuffer {
    buffer: UniformBuffer<StarfieldUniform>,
}

#[derive(Component)]
struct StarfieldBindGroup(BindGroup);

/// Render a sky filled with stars.
pub struct StarfieldPlugin;
impl Plugin for StarfieldPlugin {
    fn build(&self, app: &mut App) {
        let mut shaders = app.world.resource_mut::<Assets<Shader>>();
        let starfield_shader = Shader::from_wgsl(include_str!("shader.wgsl"));
        shaders.set_untracked(STARFIELD_SHADER_HANDLE, starfield_shader);

        app.insert_resource(ClearColor(Color::BLACK))
            .init_resource::<GameUnitsToCelestial>()
            .init_resource::<StarfieldUniformBuffer>();

        if let Ok(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .init_resource::<StarfieldPipeline>()
                .init_resource::<StarfieldUniformBuffer>()
                .init_resource::<SpecializedRenderPipelines<StarfieldPipeline>>()
                .add_system(extract_starfield.in_schedule(ExtractSchedule))
                .add_system(prepare_starfield.in_set(RenderSet::Prepare))
                .add_system(queue_starfield.in_set(RenderSet::Queue))
                .add_render_command::<Opaque3d, DrawStarfield>();
        }
    }
}

fn extract_starfield(mut commands: Commands, r: Extract<Res<GameUnitsToCelestial>>) {
    commands.insert_resource(r.clone())
}

fn prepare_starfield(
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut starfield_buffer: ResMut<StarfieldUniformBuffer>,
    game_units_to_celestial: Res<GameUnitsToCelestial>,
    time: Res<Time>,
) {
    let buffer = starfield_buffer.buffer.get_mut();
    buffer.world_to_ecef = game_units_to_celestial.world_to_ecef;
    buffer.sidereal_time = astro::mn_sidr(
        game_units_to_celestial.initial_julian_date
            + game_units_to_celestial.time_scale * time.elapsed_seconds_f64() / 86400.0,
    ) as f32;

    starfield_buffer
        .buffer
        .write_buffer(&render_device, &render_queue);
}

fn queue_starfield(
    mut commands: Commands,
    starfield_pipeline: Res<StarfieldPipeline>,
    starfield_buffer: Res<StarfieldUniformBuffer>,
    mut pipelines: ResMut<SpecializedRenderPipelines<StarfieldPipeline>>,
    pipeline_cache: Res<PipelineCache>,
    draw_functions: Res<DrawFunctions<Opaque3d>>,
    render_device: Res<RenderDevice>,
    view_uniforms: Res<ViewUniforms>,
    mut views: Query<(Entity, &mut RenderPhase<Opaque3d>)>,
) {
    let pipeline = pipelines.specialize(&pipeline_cache, &starfield_pipeline, ());

    let draw_function = draw_functions.read().id::<DrawStarfield>();
    if let (Some(view_uniforms), Some(starfield_buffer)) = (
        view_uniforms.uniforms.binding(),
        starfield_buffer.buffer.binding(),
    ) {
        for (entity, mut opaque3d) in views.iter_mut() {
            opaque3d.add(Opaque3d {
                distance: f32::MAX,
                pipeline,
                entity: commands.spawn_empty().id(),
                draw_function,
            });

            commands
                .entity(entity)
                .insert(StarfieldBindGroup(render_device.create_bind_group(
                    &BindGroupDescriptor {
                        label: Some("starfield_bind_group"),
                        layout: &starfield_pipeline.stars_layout,
                        entries: &[
                            BindGroupEntry {
                                binding: 0,
                                resource: view_uniforms.clone(),
                            },
                            BindGroupEntry {
                                binding: 1,
                                resource: starfield_buffer.clone(),
                            },
                            BindGroupEntry {
                                binding: 2,
                                resource: BindingResource::Buffer(BufferBinding {
                                    buffer: &starfield_pipeline.stars_buffer,
                                    offset: 0,
                                    size: None,
                                }),
                            },
                        ],
                    },
                )));
        }
    }
}

const STARFIELD_SHADER_HANDLE: HandleUntyped =
    HandleUntyped::weak_from_u64(Shader::TYPE_UUID, 17029892201246543411);

#[derive(Resource)]
struct StarfieldPipeline {
    stars_buffer: Buffer,
    stars_layout: BindGroupLayout,
}
impl FromWorld for StarfieldPipeline {
    fn from_world(world: &mut World) -> Self {
        let mut system_state: SystemState<(
            Res<RenderDevice>,
            Res<DefaultImageSampler>,
            Res<RenderQueue>,
        )> = SystemState::new(world);
        let (render_device, _default_sampler, _render_queue) = system_state.get_mut(world);

        let mut stars = vec![0.0f32; 4 * 9096];
        bytemuck::cast_slice_mut(&mut stars).copy_from_slice(include_bytes!("../stars.bin"));
        for star in stars.chunks_mut(4) {
            let (gal_lat, gal_long) = (star[0] as f64, star[1] as f64);
            star[0] = crate::astro::dec_frm_gal(gal_long, gal_lat) as f32;
            star[1] = crate::astro::asc_frm_gal(gal_long, gal_lat) as f32;
        }

        let stars_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("starfield_buffer"),
            contents: bytemuck::cast_slice(&stars),
            usage: BufferUsages::STORAGE,
        });

        let stars_layout = render_device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(4 * 9096),
                    },
                    count: None,
                },
            ],
            label: Some("starfield_layout"),
        });

        Self {
            stars_buffer,
            stars_layout,
        }
    }
}
impl SpecializedRenderPipeline for StarfieldPipeline {
    type Key = ();
    fn specialize(&self, _key: Self::Key) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some("starfield_pipeline".into()),
            layout: vec![self.stars_layout.clone()],
            push_constant_ranges: vec![],
            vertex: VertexState {
                shader: STARFIELD_SHADER_HANDLE.typed::<Shader>(),
                shader_defs: Vec::new(),
                entry_point: "vertex".into(),
                buffers: Vec::new(),
            },
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: PolygonMode::Fill,
                conservative: false,
                unclipped_depth: false,
            },
            depth_stencil: Some(DepthStencilState {
                format: TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: CompareFunction::Always,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: MultisampleState {
                count: 4,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(FragmentState {
                shader: STARFIELD_SHADER_HANDLE.typed::<Shader>(),
                shader_defs: Vec::new(),
                entry_point: "fragment".into(),
                targets: vec![Some(ColorTargetState {
                    format: TextureFormat::Rgba8UnormSrgb,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
            }),
        }
    }
}

struct StarfieldRenderCommand;
impl<P: PhaseItem> RenderCommand<P> for StarfieldRenderCommand {
    type Param = ();
    type ViewWorldQuery = (Read<ViewUniformOffset>, Read<StarfieldBindGroup>);
    type ItemWorldQuery = ();

    fn render<'w>(
        _item: &P,
        (view_uniform, bind_group): <<Self::ViewWorldQuery as WorldQuery>::ReadOnly as WorldQuery>::Item<'w>,
        _entity: <<Self::ItemWorldQuery as WorldQuery>::ReadOnly as WorldQuery>::Item<'w>,
        _param: <Self::Param as SystemParam>::Item<'w, '_>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        pass.set_bind_group(0, &bind_group.0, &[view_uniform.offset]);
        pass.draw(0..6 * 9096, 0..1);
        RenderCommandResult::Success
    }
}
