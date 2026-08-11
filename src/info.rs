use crate::mesh::{Mesh, Vertex};
use cgmath::InnerSpace;
use std::collections::HashMap;
use std::error::Error;
use std::fmt;

/// Significant KPIs extracted from a 3D model file without GPU rendering.
pub struct ModelInfo {
    pub file_path: String,
    pub file_size_bytes: Option<u64>,
    pub format: String,
    pub triangle_count: usize,
    pub vertex_count: usize,
    pub size_x: f32,
    pub size_y: f32,
    pub size_z: f32,
    pub surface_area: f32,
    pub volume: f32,
    pub is_watertight: bool,
}

impl ModelInfo {
    pub fn from_file(path: &str, recalc_normals: bool) -> Result<Self, Box<dyn Error>> {
        let file_size_bytes = if path == "-" {
            None
        } else {
            Some(std::fs::metadata(path)?.len())
        };

        let format = if path == "-" {
            "STL".to_string()
        } else {
            std::path::Path::new(path)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("stl")
                .to_uppercase()
        };

        let mesh = Mesh::load(path, recalc_normals)?;

        let triangle_count = mesh.vertices.len() / 3;
        let vertex_count = mesh.vertices.len();
        let bb = &mesh.bounds;
        let size_x = bb.max.x - bb.min.x;
        let size_y = bb.max.y - bb.min.y;
        let size_z = bb.max.z - bb.min.z;

        let (surface_area, volume) = compute_surface_and_volume(&mesh.vertices);
        let is_watertight = check_watertight(&mesh.vertices);

        Ok(ModelInfo {
            file_path: path.to_string(),
            file_size_bytes,
            format,
            triangle_count,
            vertex_count,
            size_x,
            size_y,
            size_z,
            surface_area,
            volume,
            is_watertight,
        })
    }
}

/// Computes (surface_area, volume) over flat unindexed triangle vertices.
/// Volume is derived via the divergence theorem and is only geometrically
/// meaningful for closed (watertight) meshes.
fn compute_surface_and_volume(vertices: &[Vertex]) -> (f32, f32) {
    let mut surface_area = 0.0f32;
    let mut signed_volume = 0.0f64;

    for tri in vertices.chunks_exact(3) {
        let a = cgmath::Vector3::from(tri[0].position);
        let b = cgmath::Vector3::from(tri[1].position);
        let c = cgmath::Vector3::from(tri[2].position);

        surface_area += (b - a).cross(c - a).magnitude() * 0.5;

        // (1/6) · a · (b × c)  — accumulate in f64 to reduce cancellation error
        let a64 = cgmath::Vector3::new(a.x as f64, a.y as f64, a.z as f64);
        let b64 = cgmath::Vector3::new(b.x as f64, b.y as f64, b.z as f64);
        let c64 = cgmath::Vector3::new(c.x as f64, c.y as f64, c.z as f64);
        signed_volume += a64.dot(b64.cross(c64));
    }

    (surface_area, (signed_volume / 6.0).abs() as f32)
}

/// Returns true when every mesh edge is shared by exactly two triangles
/// (necessary condition for a closed, manifold mesh).
fn check_watertight(vertices: &[Vertex]) -> bool {
    let quantize = |p: &[f32; 3]| -> [u32; 3] {
        [p[0].to_bits(), p[1].to_bits(), p[2].to_bits()]
    };

    let mut edge_count: HashMap<([u32; 3], [u32; 3]), u32> = HashMap::new();

    for tri in vertices.chunks_exact(3) {
        let v = [
            quantize(&tri[0].position),
            quantize(&tri[1].position),
            quantize(&tri[2].position),
        ];
        for (i, j) in [(0usize, 1usize), (1, 2), (2, 0)] {
            let edge = if v[i] <= v[j] { (v[i], v[j]) } else { (v[j], v[i]) };
            *edge_count.entry(edge).or_insert(0) += 1;
        }
    }

    edge_count.values().all(|&c| c == 2)
}

fn fmt_bytes(bytes: u64) -> String {
    const KB: u64 = 1_024;
    const MB: u64 = 1_024 * KB;
    const GB: u64 = 1_024 * MB;
    if bytes >= GB {
        format!("{:.2} GB ({} bytes)", bytes as f64 / GB as f64, fmt_int(bytes as usize))
    } else if bytes >= MB {
        format!("{:.2} MB ({} bytes)", bytes as f64 / MB as f64, fmt_int(bytes as usize))
    } else if bytes >= KB {
        format!("{:.2} KB ({} bytes)", bytes as f64 / KB as f64, fmt_int(bytes as usize))
    } else {
        format!("{} bytes", bytes)
    }
}

fn fmt_int(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

impl fmt::Display for ModelInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let w = 14; // label column width
        writeln!(f, "{:<w$} {}", "File:", self.file_path)?;
        writeln!(f, "{:<w$} {}", "Format:", self.format)?;
        if let Some(sz) = self.file_size_bytes {
            writeln!(f, "{:<w$} {}", "File size:", fmt_bytes(sz))?;
        }
        writeln!(f, "{:<w$} {}", "Triangles:", fmt_int(self.triangle_count))?;
        writeln!(f, "{:<w$} {}", "Vertices:", fmt_int(self.vertex_count))?;
        writeln!(
            f, "{:<w$} {:.4} x {:.4} x {:.4}  (X × Y × Z)",
            "Dimensions:", self.size_x, self.size_y, self.size_z
        )?;
        writeln!(f, "{:<w$} {:.4}", "Surface area:", self.surface_area)?;
        if self.is_watertight {
            writeln!(f, "{:<w$} {:.4}", "Volume:", self.volume)?;
        } else {
            writeln!(f, "{:<w$} {:.4}  (estimate — mesh is not watertight)", "Volume:", self.volume)?;
        }
        writeln!(f, "{:<w$} {}", "Watertight:", if self.is_watertight { "yes" } else { "no" })?;
        Ok(())
    }
}
