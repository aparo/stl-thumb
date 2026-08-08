struct Uniforms {
    modelview: mat4x4<f32>,
    perspective: mat4x4<f32>,
    u_light: vec3<f32>,
    _pad0: f32,
    ambient_color: vec3<f32>,
    _pad1: f32,
    diffuse_color: vec3<f32>,
    _pad2: f32,
    specular_color: vec3<f32>,
    _pad3: f32,
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) v_position: vec3<f32>,
    @location(1) v_normal: vec3<f32>,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let world_pos = uniforms.modelview * vec4<f32>(in.position, 1.0);
    out.clip_position = uniforms.perspective * world_pos;
    out.v_position = world_pos.xyz / world_pos.w;
    out.v_normal = mat3x3<f32>(
        uniforms.modelview[0].xyz,
        uniforms.modelview[1].xyz,
        uniforms.modelview[2].xyz,
    ) * in.normal;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let normal = normalize(in.v_normal);
    let light = normalize(uniforms.u_light);
    let diffuse = max(dot(normal, light), 0.0);
    let camera_dir = normalize(-in.v_position);
    let half_dir = normalize(light + camera_dir);
    let specular = pow(max(dot(half_dir, normal), 0.0), 16.0);
    let color = uniforms.ambient_color
        + diffuse * uniforms.diffuse_color
        + specular * uniforms.specular_color;
    return vec4<f32>(color, 1.0);
}
