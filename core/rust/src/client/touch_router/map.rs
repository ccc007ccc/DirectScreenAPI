pub(super) fn normalize_axis(raw: i32, min_v: i32, max_v: i32) -> f32 {
    if max_v <= min_v {
        return 0.0;
    }
    let num = (raw - min_v) as f32;
    let den = (max_v - min_v) as f32;
    clamp01(num / den)
}

fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}
