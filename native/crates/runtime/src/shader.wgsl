struct Camera {
    view_projection: mat4x4<f32>,
    eye: vec4<f32>,
    environment: vec4<f32>,
    light: mat4x4<f32>,
    // xyz: direction towards the sun or moon, w: daylight 0..1.
    sun: vec4<f32>,
    sun_color: vec4<f32>,
    // Multiplier for the day-baked prelit colours (night darkening).
    ambient: vec4<f32>,
    sky: vec4<f32>,
    // x: how many object lights light surfaces per pixel (the Lamp lighting
    // setting, at night), then two vec4s each: position and reach, colour
    // and strength.
    point: vec4<f32>,
    lights: array<vec4<f32>, 64>,
};
@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var image: texture_2d<f32>;
@group(1) @binding(1) var image_sampler: sampler;
@group(2) @binding(0) var shadow_map: texture_depth_2d;
@group(2) @binding(1) var shadow_sampler: sampler_comparison;

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
// environment.x: outdoor haze, environment.y: sun lighting strength (0 keeps the
// original prelit look), environment.z: sun shadow strength, environment.w: weather
// haze density relative to clear weather. The face normal comes from screen-space derivatives,
// so moving cars and animated peds need no extra vertex data.
var<private> DISC: array<vec2<f32>, 12> = array<vec2<f32>, 12>(
    vec2<f32>(-0.326, -0.406), vec2<f32>(-0.840, -0.074), vec2<f32>(-0.696, 0.457),
    vec2<f32>(-0.203, 0.621), vec2<f32>(0.962, -0.195), vec2<f32>(0.473, -0.480),
    vec2<f32>(0.519, 0.767), vec2<f32>(0.185, -0.893), vec2<f32>(0.507, 0.064),
    vec2<f32>(0.896, 0.412), vec2<f32>(-0.322, -0.933), vec2<f32>(-0.792, -0.598),
);
// Must match the puddle noise in postprocess.wgsl.
fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}
fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash(i), hash(i + vec2<f32>(1.0, 0.0)), u.x), mix(hash(i + vec2<f32>(0.0, 1.0)), hash(i + vec2<f32>(1.0, 1.0)), u.x), u.y);
}
fn puddles(p: vec2<f32>, wetness: f32) -> f32 {
    let n = noise(p * 0.18) * 0.65 + noise(p * 0.7) * 0.35;
    let pools = smoothstep(0.55 - wetness * 0.2, 0.7 - wetness * 0.2, n);
    return wetness * mix(0.35, 1.0, pools);
}
fn face_normal(world_position: vec3<f32>) -> vec3<f32> {
    var normal = normalize(cross(dpdx(world_position), dpdy(world_position)));
    if (dot(normal, camera.eye.xyz - world_position) < 0.0) { normal = -normal; }
    return normal;
}
// Enhanced lighting: wet ground is darker and catches a sharp sun glint;
// the reflection itself is added by the finishing pass. sun_color.w carries
// the wetness and is zero when enhanced lighting is off.
fn wet_ground(color: vec3<f32>, world_position: vec3<f32>, normal: vec3<f32>) -> vec3<f32> {
    let wetness = camera.sun_color.w;
    if (wetness <= 0.0) { return color; }
    let wet = puddles(world_position.xz, wetness) * smoothstep(0.85, 0.97, normal.y);
    let view = normalize(camera.eye.xyz - world_position);
    let half_way = normalize(view + camera.sun.xyz);
    let glint = pow(max(dot(vec3<f32>(0.0, 1.0, 0.0), half_way), 0.0), 220.0) * camera.sun.w * sun_shadow(world_position);
    return color * (1.0 - 0.4 * wet) + camera.sun_color.rgb * glint * wet * 2.0;
}
fn sun_shadow(world_position: vec3<f32>) -> f32 {
    let clip = camera.light * vec4<f32>(world_position, 1.0);
    let uv = clip.xy * vec2<f32>(0.5, -0.5) + 0.5;
    let texel = 1.0 / 2048.0;
    var lit = 0.0;
    if (camera.eye.w > 0.5) {
        // Enhanced lighting: a wider, per-pixel rotated disc gives soft
        // penumbras instead of the stepped 3x3 edge.
        let angle = hash(world_position.xz * 7.0 + world_position.y) * 6.2831853;
        let spin = mat2x2<f32>(cos(angle), sin(angle), -sin(angle), cos(angle));
        for (var i = 0; i < 12; i += 1) {
            let offset = spin * DISC[i] * 2.5 * texel;
            lit += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + offset, clip.z);
        }
        lit *= 9.0 / 12.0;
    } else {
        for (var y = -1; y <= 1; y += 1) {
            for (var x = -1; x <= 1; x += 1) {
                lit += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2<f32>(f32(x), f32(y)) * texel, clip.z);
            }
        }
    }
    // Fade out towards the edge of the shadowed square instead of cutting off.
    let edge = max(abs(clip.x), abs(clip.y));
    let inside = 1.0 - smoothstep(0.85, 1.0, edge);
    let covered = select(0.0, inside, clip.z > 0.0 && clip.z < 1.0);
    return mix(1.0, lit / 9.0, covered);
}
fn sun_light(world_position: vec3<f32>, normal: vec3<f32>) -> vec3<f32> {
    let facing = max(dot(normal, camera.sun.xyz), 0.0);
    let shadow = sun_shadow(world_position);
    let sky = mix(vec3<f32>(0.62, 0.60, 0.58), vec3<f32>(0.72, 0.76, 0.84), normal.y * 0.5 + 0.5);
    let lit = sky + camera.sun_color.rgb * 0.45 * facing * shadow * camera.sun.w;
    // Shadows also darken the original prelit colours, which already contain
    // the sun, so they read clearly even with low sun lighting.
    let shade = mix(1.0, 0.55, (1.0 - shadow) * camera.environment.z * camera.sun.w * smoothstep(0.0, 0.2, facing));
    return mix(vec3<f32>(1.0), lit, camera.environment.y) * shade * camera.ambient.rgb;
}
// Object lights (lamps placed in Build Mode) lighting what is near them.
fn point_lights(world_position: vec3<f32>, normal: vec3<f32>) -> vec3<f32> {
    var sum = vec3<f32>(0.0);
    let count = min(u32(camera.point.x), 32u);
    for (var i = 0u; i < count; i += 1u) {
        let place = camera.lights[i * 2u];
        let to_light = place.xyz - world_position;
        let d = length(to_light);
        if (d >= place.w) { continue; }
        let colour = camera.lights[i * 2u + 1u];
        // Bright under the lamp, fading softly to nothing at its reach.
        let near = smoothstep(0.0, 1.0, 1.0 - d / place.w);
        // Mostly the side facing the lamp, a little all round.
        let facing = max(dot(normal, to_light / max(d, 0.001)), 0.0) * 0.8 + 0.2;
        sum += colour.rgb * (near * near * facing * colour.w);
    }
    return sum;
}
fn shade(color: vec4<f32>, input: VertexOut, light: vec3<f32>, normal: vec3<f32>) -> vec4<f32> {
    let density = 0.00045 * max(camera.environment.w, 1.0);
    let haze = (1.0 - exp(-distance(input.world_position, camera.eye.xyz) * density)) * camera.environment.x;
    let lit = wet_ground(color.rgb * light, input.world_position, normal);
    return vec4<f32>(mix(lit, camera.sky.rgb, haze), color.a);
}
@fragment fn fs_main(input: VertexOut) -> @location(0) vec4<f32> {
    // Derivatives are taken before any discard.
    let normal = face_normal(input.world_position);
    let light = sun_light(input.world_position, normal) + point_lights(input.world_position, normal);
    let color = textureSample(image, image_sampler, input.uv) * input.color;
    if (color.a < 0.5) { discard; }
    return shade(color, input, light, normal);
}
@fragment fn fs_blended(input: VertexOut) -> @location(0) vec4<f32> {
    // Derivatives are taken before any discard.
    let normal = face_normal(input.world_position);
    let light = sun_light(input.world_position, normal) + point_lights(input.world_position, normal);
    let color = textureSample(image, image_sampler, input.uv) * input.color;
    if (color.a < 0.04) { discard; }
    return shade(color, input, light, normal);
}
struct ShadowOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) alpha: f32,
};
@vertex fn vs_shadow(input: VertexIn) -> ShadowOut {
    var out: ShadowOut;
    out.position = camera.light * vec4<f32>(input.position, 1.0);
    out.uv = input.uv;
    out.alpha = input.color.a;
    return out;
}
@fragment fn fs_shadow(input: ShadowOut) {
    if (textureSample(image, image_sampler, input.uv).a * input.alpha < 0.5) { discard; }
}
// Night glows from original 2DFX lights, drawn additively after the scene.
// Colour alpha 1 marks night-only lights, 0.5 always-lit ones.
fn glow_visibility(alpha: f32) -> f32 {
    let night = 1.0 - camera.sun.w;
    return select(night, max(night, 0.35), alpha < 0.75);
}
@vertex fn vs_corona(input: VertexIn) -> VertexOut {
    let to_eye = camera.eye.xyz - input.position;
    let distance = max(length(to_eye), 0.001);
    let forward = to_eye / distance;
    let right = normalize(cross(vec3<f32>(0.0, 1.0, 0.0), forward) + vec3<f32>(1e-5, 0.0, 0.0));
    let up = cross(forward, right);
    // Pull the sprite towards the camera so the lamp does not hide its glow,
    // and grow it gently with distance so far lamps stay visible.
    let centre = input.position + forward * min(0.6, distance * 0.5);
    let world = centre + (right * input.uv.x + up * input.uv.y) * (1.0 + distance * 0.004);
    var out: VertexOut;
    out.position = camera.view_projection * vec4<f32>(world, 1.0);
    out.uv = sign(input.uv);
    out.color = input.color;
    out.world_position = world;
    return out;
}
@fragment fn fs_corona(input: VertexOut) -> @location(0) vec4<f32> {
    let r2 = dot(input.uv, input.uv);
    let falloff = max(exp(-r2 * 4.0) - exp(-4.0), 0.0);
    return vec4<f32>(input.color.rgb * falloff * glow_visibility(input.color.a) * 1.6, 1.0);
}
@fragment fn fs_pool(input: VertexOut) -> @location(0) vec4<f32> {
    let edge = max(1.0 - dot(input.uv, input.uv), 0.0);
    return vec4<f32>(input.color.rgb * edge * edge * glow_visibility(input.color.a) * 0.45, 1.0);
}
