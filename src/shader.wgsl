#import bevy_render::view

struct Uniforms {
    world_to_ecef: mat3x3<f32>,
    sidereal_time: f32,
}

@group(0) @binding(0)
var<uniform> view: View;

@group(0) @binding(1)
var<uniform> uniforms: Uniforms;

@group(0) @binding(2)
var<storage,read> stars: array<vec4<f32>>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) texcoord: vec2<f32>,
    @location(1) magnitude: f32,
    @location(2) world_position: vec4<f32>,
};

@vertex
fn vertex( @builtin(vertex_index) in_vertex_index: u32,) -> VertexOutput {
    var out: VertexOutput;

    let star = stars[in_vertex_index / 6u];
    let declination = star.x;
    let ascension = star.y;
    out.magnitude = star.z;

    let sidereal_time = uniforms.sidereal_time;

	if(in_vertex_index % 6u == 0u) { out.texcoord = vec2(0., 0.); }
	if(in_vertex_index % 6u == 1u) { out.texcoord = vec2(1., 0.); }
	if(in_vertex_index % 6u == 2u) { out.texcoord = vec2(0., 1.); }
	if(in_vertex_index % 6u == 3u) { out.texcoord = vec2(1., 1.); }
	if(in_vertex_index % 6u == 4u) { out.texcoord = vec2(0., 1.); }
	if(in_vertex_index % 6u == 5u) { out.texcoord = vec2(1., 0.); }

    let direction = vec3(
		-sin(ascension - sidereal_time) * cos(declination),
		cos(ascension - sidereal_time) * cos(declination),
		sin(declination));

    let screen_dimensions = vec2(view.viewport.z, view.viewport.w);

	out.position = view.view_proj * vec4(uniforms.world_to_ecef * direction, 1.e-15);
    let position_delta = (out.texcoord-vec2(0.5)) * out.position.w * 4.0 * 2.0 * clamp(exp(1. - 0.35 * out.magnitude), .25, 1.) / vec2(screen_dimensions);
	out.position.x += position_delta.x;
    out.position.y += position_delta.y;

	out.world_position = out.position;

    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // // The north star has a magnitude of 2.02. Setting its color to red can be useful for
    // // debugging purposes.
    // if (in.magnitude == 2.02) {
    //     return vec4(1., 0., 0., 1.);
    // }

	let v = in.texcoord * 2.0 - 1.0;
	let x = dot(v, v);
	let alpha = smoothstep(1., 0., x) * clamp(0., 1., exp(1. - 0.7 * in.magnitude));
    return vec4(1., 1., 1., alpha);
}
