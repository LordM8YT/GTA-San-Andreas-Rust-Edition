struct Camera { view_projection: mat4x4<f32>, eye: vec4<f32>, environment: vec4<f32> };
@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var image: texture_2d<f32>;
@group(1) @binding(1) var image_sampler: sampler;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
};
struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) world_position: vec3<f32>,
};
@vertex fn vs_main(input: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.position = camera.view_projection * vec4<f32>(input.position, 1.0);
    out.uv = input.uv;
    out.color = input.color;
    out.world_position = input.position;
    return out;
}
@fragment fn fs_main(input: VertexOut) -> @location(0) vec4<f32> {
    let color = textureSample(image, image_sampler, input.uv) * input.color;
    if (color.a < 0.5) { discard; }
    let haze = (1.0 - exp(-distance(input.world_position, camera.eye.xyz) * 0.00045)) * camera.environment.x;
    return vec4<f32>(mix(color.rgb, vec3<f32>(0.48, 0.60, 0.70), haze), color.a);
}
@fragment fn fs_blended(input: VertexOut) -> @location(0) vec4<f32> {
    let color = textureSample(image, image_sampler, input.uv) * input.color;
    if (color.a < 0.04) { discard; }
    let haze = (1.0 - exp(-distance(input.world_position, camera.eye.xyz) * 0.00045)) * camera.environment.x;
    return vec4<f32>(mix(color.rgb, vec3<f32>(0.48, 0.60, 0.70), haze), color.a);
}
