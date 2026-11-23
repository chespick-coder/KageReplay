pub fn render_icon_rgba(size: u32) -> Vec<u8> {
    let s = size as f32;
    let mut out = vec![0u8; (size * size * 4) as usize];
    let samples = if size <= 32 { 8 } else { 4 };
    for y in 0..size {
        for x in 0..size {
            let mut acc = [0f32; 4];
            for sy in 0..samples {
                for sx in 0..samples {
                    let px = (x as f32 + (sx as f32 + 0.5) / samples as f32) / s;
                    let py = (y as f32 + (sy as f32 + 0.5) / samples as f32) / s;
                    let c = shade(px, py);
                    acc[0] += c[0] * c[3];
                    acc[1] += c[1] * c[3];
                    acc[2] += c[2] * c[3];
                    acc[3] += c[3];
                }
            }
            let n = (samples * samples) as f32;
            let a = acc[3] / n;
            let idx = ((y * size + x) * 4) as usize;
            if a > 0.0 {
                out[idx] = (acc[0] / acc[3] * 255.0).round().clamp(0.0, 255.0) as u8;
                out[idx + 1] = (acc[1] / acc[3] * 255.0).round().clamp(0.0, 255.0) as u8;
                out[idx + 2] = (acc[2] / acc[3] * 255.0).round().clamp(0.0, 255.0) as u8;
                out[idx + 3] = (a * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

pub const MARK_POLYGONS: [&[(f32, f32)]; 3] = [
    &[(619.0, 297.5), (682.0, 243.0), (682.0, 457.0), (619.0, 507.5)],
    &[(812.0, 281.0), (913.0, 281.0), (702.0, 441.0), (702.0, 368.0)],
    &[(812.0, 383.0), (935.5, 485.0), (829.0, 485.0), (757.5, 424.4)],
];

const MARK_LEFT: f32 = 619.0;
const MARK_TOP: f32 = 243.0;
const MARK_WIDTH: f32 = 316.5;
const MARK_HEIGHT: f32 = 264.5;

pub fn mark_point(x: f32, y: f32, origin: (f32, f32), height: f32) -> (f32, f32) {
    let scale = height / MARK_HEIGHT;
    (origin.0 + (x - MARK_LEFT) * scale, origin.1 + (y - MARK_TOP) * scale)
}

pub fn mark_aspect() -> f32 {
    MARK_WIDTH / MARK_HEIGHT
}

fn rounded_rect_inside(px: f32, py: f32, inset: f32, radius: f32) -> bool {
    let lo = inset;
    let hi = 1.0 - inset;
    if px < lo || px > hi || py < lo || py > hi {
        return false;
    }
    let cx = px.clamp(lo + radius, hi - radius);
    let cy = py.clamp(lo + radius, hi - radius);
    let dx = px - cx;
    let dy = py - cy;
    dx * dx + dy * dy <= radius * radius
}

fn inside_convex(px: f32, py: f32, polygon: &[(f32, f32)]) -> bool {
    let mut sign = 0.0f32;
    for i in 0..polygon.len() {
        let (ax, ay) = polygon[i];
        let (bx, by) = polygon[(i + 1) % polygon.len()];
        let cross = (bx - ax) * (py - ay) - (by - ay) * (px - ax);
        if cross != 0.0 {
            if sign == 0.0 {
                sign = cross.signum();
            } else if cross.signum() != sign {
                return false;
            }
        }
    }
    true
}

fn inside_mark(px: f32, py: f32) -> bool {
    let height = 0.56;
    let width = height * mark_aspect();
    let origin = ((1.0 - width) / 2.0 + 0.01, (1.0 - height) / 2.0);
    let scale = MARK_HEIGHT / height;
    let x = MARK_LEFT + (px - origin.0) * scale;
    let y = MARK_TOP + (py - origin.1) * scale;
    MARK_POLYGONS.iter().any(|polygon| inside_convex(x, y, polygon))
}

fn shade(px: f32, py: f32) -> [f32; 4] {
    if !rounded_rect_inside(px, py, 0.02, 0.24) {
        return [0.0; 4];
    }
    if inside_mark(px, py) {
        return [0.953, 0.953, 0.953, 1.0];
    }
    let edge = !rounded_rect_inside(px, py, 0.045, 0.215);
    let base = 0.075 - py * 0.05;
    if edge {
        let v = base + 0.07;
        return [v, v, v, 1.0];
    }
    [base, base, base * 1.04, 1.0]
}

pub fn build_ico(sizes: &[u32]) -> Vec<u8> {
    let images: Vec<(u32, Vec<u8>)> = sizes.iter().map(|&size| (size, encode_dib(size))).collect();
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (size, data) in &images {
        let dim = if *size >= 256 { 0u8 } else { *size as u8 };
        out.push(dim);
        out.push(dim);
        out.push(0);
        out.push(0);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in images {
        out.extend_from_slice(&data);
    }
    out
}

fn encode_dib(size: u32) -> Vec<u8> {
    let rgba = render_icon_rgba(size);
    let mut out = Vec::new();
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(size as i32).to_le_bytes());
    out.extend_from_slice(&((size * 2) as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(size * size * 4).to_le_bytes());
    out.extend_from_slice(&0i32.to_le_bytes());
    out.extend_from_slice(&0i32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for y in (0..size).rev() {
        for x in 0..size {
            let i = ((y * size + x) * 4) as usize;
            out.push(rgba[i + 2]);
            out.push(rgba[i + 1]);
            out.push(rgba[i]);
            out.push(rgba[i + 3]);
        }
    }
    let mask_stride = size.div_ceil(32) * 4;
    out.extend(std::iter::repeat_n(0u8, (mask_stride * size) as usize));
    out
}
