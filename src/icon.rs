//! Supersampled Big-Tech SDF stopwatch icon renderer for StreamTimer.

#[inline]
fn sd_squircle(px: f32, py: f32, radius: f32, power: f32) -> f32 {
    let norm = (px.abs().powf(power) + py.abs().powf(power)).powf(1.0 / power);
    norm - radius
}

#[inline]
fn sd_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let pax = px - ax;
    let pay = py - ay;
    let bax = bx - ax;
    let bay = by - ay;
    let denom = (bax * bax + bay * bay).max(1e-6);
    let h = ((pax * bax + pay * bay) / denom).clamp(0.0, 1.0);
    let dx = pax - bax * h;
    let dy = pay - bay * h;
    (dx * dx + dy * dy).sqrt()
}

#[inline]
fn sd_rounded_box(px: f32, py: f32, cx: f32, cy: f32, hx: f32, hy: f32, r: f32) -> f32 {
    let qx = (px - cx).abs() - hx + r;
    let qy = (py - cy).abs() - hy + r;
    let ox = qx.max(0.0);
    let oy = qy.max(0.0);
    (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0) - r
}

#[inline]
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[inline]
fn blend_over(dst: [f32; 4], src: [f32; 4]) -> [f32; 4] {
    let sa = src[3].clamp(0.0, 1.0);
    let inv = 1.0 - sa;
    [
        src[0] * sa + dst[0] * inv,
        src[1] * sa + dst[1] * inv,
        src[2] * sa + dst[2] * inv,
        (sa + dst[3] * inv).clamp(0.0, 1.0),
    ]
}

fn sample_pixel(u: f32, v: f32, aa: f32) -> [f32; 4] {
    let mut col = [0.0f32; 4];

    // 1. Dark obsidian squircle badge
    let d_badge = sd_squircle(u, v, 0.88, 4.4);
    let badge_a = 1.0 - smoothstep(-aa, aa, d_badge);
    if badge_a <= 0.001 {
        return col;
    }

    let grad_t = ((v + 0.88) / 1.76).clamp(0.0, 1.0);
    col = [
        (30.0 * (1.0 - grad_t) + 10.0 * grad_t) / 255.0,
        (30.0 * (1.0 - grad_t) + 10.0 * grad_t) / 255.0,
        (37.0 * (1.0 - grad_t) + 13.0 * grad_t) / 255.0,
        badge_a,
    ];

    // Specular inner rim
    let rim_band = (1.0 - smoothstep(0.0, 0.055 + aa, d_badge.abs())) * badge_a;
    if rim_band > 0.0 {
        let top_lit = (1.0 - grad_t * 0.65).clamp(0.25, 1.0);
        col = blend_over(
            col,
            [
                (78.0 * top_lit) / 255.0,
                (78.0 * top_lit) / 255.0,
                (94.0 * top_lit) / 255.0,
                rim_band * 0.85,
            ],
        );
    }

    // 2. Top stopwatch crown button
    let d_crown = sd_rounded_box(u, v, 0.0, -0.50, 0.11, 0.045, 0.02);
    let crown_a = 1.0 - smoothstep(-aa, aa, d_crown);
    if crown_a > 0.0 {
        col = blend_over(
            col,
            [180.0 / 255.0, 180.0 / 255.0, 192.0 / 255.0, crown_a * 0.95],
        );
    }

    // 3. Outer crisp bone-white stopwatch ring centered at (0.0, 0.05)
    let cy = 0.05;
    let dist_c = (u * u + (v - cy).powi(2)).sqrt();
    let ring_r = 0.41;
    let ring_w = 0.048;
    let d_ring = (dist_c - ring_r).abs() - ring_w;
    let ring_a = 1.0 - smoothstep(-aa, aa, d_ring);
    if ring_a > 0.0 {
        col = blend_over(
            col,
            [244.0 / 255.0, 244.0 / 255.0, 246.0 / 255.0, ring_a],
        );
    }

    // 4. Minimalist clock hands (12 o'clock vertical + 2 o'clock sweep)
    let d_hand1 = sd_segment(u, v, 0.0, cy, 0.0, cy - 0.22) - 0.036;
    let d_hand2 = sd_segment(u, v, 0.0, cy, 0.17, cy + 0.09) - 0.034;
    let hand_a = 1.0 - smoothstep(-aa, aa, d_hand1.min(d_hand2));
    if hand_a > 0.0 {
        col = blend_over(
            col,
            [244.0 / 255.0, 244.0 / 255.0, 246.0 / 255.0, hand_a],
        );
    }

    // 5. Emerald live-recording pivot dot (#34D399) at center
    let d_pivot = dist_c - 0.068;
    let pivot_a = 1.0 - smoothstep(-aa, aa, d_pivot);
    if pivot_a > 0.0 {
        col = blend_over(
            col,
            [52.0 / 255.0, 211.0 / 255.0, 153.0 / 255.0, pivot_a],
        );
    }

    col
}

/// Render straight non-premultiplied RGBA8 pixels for a square icon of `size x size`.
pub fn render_stream_timer_rgba(size: u32) -> Vec<u8> {
    let mut out = vec![0u8; (size * size * 4) as usize];
    let inv = 1.0 / size as f32;
    let aa = (1.6 / size as f32).max(0.008);
    let subs = [0.25f32, 0.75f32];

    for y in 0..size {
        for x in 0..size {
            let mut acc = [0.0f32; 4];
            for sy in subs {
                for sx in subs {
                    let u = ((x as f32 + sx) * inv) * 2.0 - 1.0;
                    let v = ((y as f32 + sy) * inv) * 2.0 - 1.0;
                    let s = sample_pixel(u, v, aa);
                    acc[0] += s[0];
                    acc[1] += s[1];
                    acc[2] += s[2];
                    acc[3] += s[3];
                }
            }
            let idx = ((y * size + x) * 4) as usize;
            out[idx] = ((acc[0] * 0.25).clamp(0.0, 1.0) * 255.0).round() as u8;
            out[idx + 1] = ((acc[1] * 0.25).clamp(0.0, 1.0) * 255.0).round() as u8;
            out[idx + 2] = ((acc[2] * 0.25).clamp(0.0, 1.0) * 255.0).round() as u8;
            out[idx + 3] = ((acc[3] * 0.25).clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }

    out
}
