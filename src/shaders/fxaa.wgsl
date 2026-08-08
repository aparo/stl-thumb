struct FxaaUniforms {
    resolution: vec2<f32>,
    enabled: i32,
    _pad: u32,
}

@group(0) @binding(0)
var<uniform> uniforms: FxaaUniforms;

@group(0) @binding(1)
var tex: texture_2d<f32>;

@group(0) @binding(2)
var tex_sampler: sampler;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) i_tex_coords: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) v_tex_coords: vec2<f32>,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(in.position, 0.0, 1.0);
    out.v_tex_coords = in.i_tex_coords;
    return out;
}

const FXAA_REDUCE_MIN: f32 = 1.0 / 128.0;
const FXAA_REDUCE_MUL: f32 = 1.0 / 8.0;
const FXAA_SPAN_MAX: f32 = 8.0;

fn fxaa_filter(frag_coord: vec2<f32>) -> vec4<f32> {
    let inv = vec2<f32>(1.0 / uniforms.resolution.x, 1.0 / uniforms.resolution.y);

    let nw = (frag_coord + vec2<f32>(-1.0, -1.0)) * inv;
    let ne = (frag_coord + vec2<f32>( 1.0, -1.0)) * inv;
    let sw = (frag_coord + vec2<f32>(-1.0,  1.0)) * inv;
    let se = (frag_coord + vec2<f32>( 1.0,  1.0)) * inv;
    let m  = frag_coord * inv;

    let rgb_nw = textureSample(tex, tex_sampler, nw).xyz;
    let rgb_ne = textureSample(tex, tex_sampler, ne).xyz;
    let rgb_sw = textureSample(tex, tex_sampler, sw).xyz;
    let rgb_se = textureSample(tex, tex_sampler, se).xyz;
    let tex_m  = textureSample(tex, tex_sampler, m);
    let rgb_m  = tex_m.xyz;

    let luma  = vec3<f32>(0.299, 0.587, 0.114);
    let luma4 = vec4<f32>(0.299, 0.587, 0.114, 0.0);

    let l_nw = dot(rgb_nw, luma);
    let l_ne = dot(rgb_ne, luma);
    let l_sw = dot(rgb_sw, luma);
    let l_se = dot(rgb_se, luma);
    let l_m  = dot(rgb_m,  luma);
    let l_min = min(l_m, min(min(l_nw, l_ne), min(l_sw, l_se)));
    let l_max = max(l_m, max(max(l_nw, l_ne), max(l_sw, l_se)));

    var dir: vec2<f32>;
    dir.x = -((l_nw + l_ne) - (l_sw + l_se));
    dir.y =  ((l_nw + l_sw) - (l_ne + l_se));

    let dir_reduce = max((l_nw + l_ne + l_sw + l_se) * (0.25 * FXAA_REDUCE_MUL), FXAA_REDUCE_MIN);
    let rcp = 1.0 / (min(abs(dir.x), abs(dir.y)) + dir_reduce);
    dir = clamp(dir * rcp, vec2<f32>(-FXAA_SPAN_MAX), vec2<f32>(FXAA_SPAN_MAX)) * inv;

    let caa = frag_coord * inv + dir * (1.0 / 3.0 - 0.5);
    let cab = frag_coord * inv + dir * (2.0 / 3.0 - 0.5);
    let s_aa = textureSample(tex, tex_sampler, caa);
    let s_ab = textureSample(tex, tex_sampler, cab);
    let a_aa = s_aa.a;
    let a_ab = s_ab.a;
    let at_a = a_aa + a_ab;
    let w_aa = select(0.5, a_aa / at_a, at_a > 0.0);
    let w_ab = select(0.5, a_ab / at_a, at_a > 0.0);
    let rgb_a = vec4<f32>(
        s_aa.rgb * w_aa + s_ab.rgb * w_ab,
        0.5 * at_a,
    );

    let cbc = frag_coord * inv + dir * -0.5;
    let cbd = frag_coord * inv + dir *  0.5;
    let s_bc = textureSample(tex, tex_sampler, cbc);
    let s_bd = textureSample(tex, tex_sampler, cbd);
    let a_bc = s_bc.a;
    let a_bd = s_bd.a;
    let at_b = a_aa + a_ab + a_bc + a_bd;
    let w_ba = select(0.25, a_aa / at_b, at_b > 0.0);
    let w_bb = select(0.25, a_ab / at_b, at_b > 0.0);
    let w_bc = select(0.25, a_bc / at_b, at_b > 0.0);
    let w_bd = select(0.25, a_bd / at_b, at_b > 0.0);
    let rgb_b = vec4<f32>(
        s_aa.rgb * w_ba + s_ab.rgb * w_bb + s_bc.rgb * w_bc + s_bd.rgb * w_bd,
        0.25 * at_b,
    );

    let l_b = dot(rgb_b, luma4);
    if l_b < l_min || l_b > l_max {
        return rgb_a;
    }
    return rgb_b;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    if uniforms.enabled != 0 {
        let frag_coord = in.v_tex_coords * uniforms.resolution;
        return fxaa_filter(frag_coord);
    }
    return textureSample(tex, tex_sampler, in.v_tex_coords);
}
