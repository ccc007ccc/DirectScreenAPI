use crate::api::Status;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GaussianBlurPass {
    pub radius: u32,
    pub sigma: f32,
}

impl GaussianBlurPass {
    pub fn normalized(self) -> Self {
        let radius = self.radius.min(64);
        if radius == 0 {
            return Self {
                radius: 0,
                sigma: 0.0,
            };
        }
        let sigma = if self.sigma.is_finite() && self.sigma > 0.01 {
            self.sigma
        } else {
            (radius as f32) * 0.5
        };
        Self { radius, sigma }
    }
}

pub const FILTER_PASS_KIND_GAUSSIAN: u32 = 1;

#[derive(Debug, Clone, PartialEq)]
pub enum FilterPass {
    GaussianBlur(GaussianBlurPass),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FilterPipeline {
    pub passes: Vec<FilterPass>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FilterReport {
    pub executed_passes: u32,
    pub frost_passes: u32,
}

pub fn apply_filter_pipeline_rgba(
    pipeline: &FilterPipeline,
    width: u32,
    height: u32,
    pixels_rgba8: &mut [u8],
) -> Result<FilterReport, Status> {
    validate_rgba_len(width, height, pixels_rgba8.len())?;
    if pipeline.passes.is_empty() {
        return Ok(FilterReport::default());
    }

    let mut report = FilterReport::default();
    for pass in &pipeline.passes {
        match pass {
            FilterPass::GaussianBlur(cfg) => {
                apply_gaussian_blur_rgba(width, height, pixels_rgba8, cfg.normalized())?;
                report.executed_passes = report.executed_passes.saturating_add(1);
                report.frost_passes = report.frost_passes.saturating_add(1);
            }
        }
    }
    Ok(report)
}

fn apply_gaussian_blur_rgba(
    width: u32,
    height: u32,
    pixels_rgba8: &mut [u8],
    cfg: GaussianBlurPass,
) -> Result<(), Status> {
    let radius = cfg.radius as i32;
    if radius == 0 {
        return Ok(());
    }

    let kernel = build_gaussian_kernel(cfg.radius as usize, cfg.sigma);
    let w = width as i32;
    let h = height as i32;
    let mut scratch = vec![0u8; pixels_rgba8.len()];

    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0f32; 4];
            for k in -radius..=radius {
                let sx = (x + k).clamp(0, w - 1);
                let weight = kernel[(k + radius) as usize];
                let idx = ((y * w + sx) as usize) * 4;
                for c in 0..4usize {
                    acc[c] += pixels_rgba8[idx + c] as f32 * weight;
                }
            }
            let out_idx = ((y * w + x) as usize) * 4;
            for c in 0..4usize {
                scratch[out_idx + c] = f32_to_u8(acc[c]);
            }
        }
    }

    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0f32; 4];
            for k in -radius..=radius {
                let sy = (y + k).clamp(0, h - 1);
                let weight = kernel[(k + radius) as usize];
                let idx = ((sy * w + x) as usize) * 4;
                for c in 0..4usize {
                    acc[c] += scratch[idx + c] as f32 * weight;
                }
            }
            let out_idx = ((y * w + x) as usize) * 4;
            for c in 0..4usize {
                pixels_rgba8[out_idx + c] = f32_to_u8(acc[c]);
            }
        }
    }

    Ok(())
}

fn build_gaussian_kernel(radius: usize, sigma: f32) -> Vec<f32> {
    let mut kernel = Vec::with_capacity(radius * 2 + 1);
    let denom = 2.0 * sigma * sigma;
    let mut sum = 0.0f32;
    for i in 0..=(radius * 2) {
        let x = i as i32 - radius as i32;
        let v = (-(x * x) as f32 / denom).exp();
        kernel.push(v);
        sum += v;
    }
    if sum > 0.0 {
        for v in &mut kernel {
            *v /= sum;
        }
    }
    kernel
}

fn validate_rgba_len(width: u32, height: u32, len: usize) -> Result<(), Status> {
    if width == 0 || height == 0 {
        return Err(Status::InvalidArgument);
    }
    let expected_len = (width as usize)
        .checked_mul(height as usize)
        .and_then(|v| v.checked_mul(4usize))
        .ok_or(Status::OutOfRange)?;
    if expected_len != len {
        return Err(Status::InvalidArgument);
    }
    Ok(())
}

fn f32_to_u8(v: f32) -> u8 {
    v.round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_pass_keeps_pixels_unchanged() {
        let mut pixels = vec![
            0u8, 10, 20, 255, 30, 40, 50, 255, 60, 70, 80, 255, 90, 100, 110, 255,
        ];
        let before = pixels.clone();
        let pipeline = FilterPipeline::default();
        let report =
            apply_filter_pipeline_rgba(&pipeline, 2, 2, &mut pixels).expect("apply pipeline");
        assert_eq!(report, FilterReport::default());
        assert_eq!(pixels, before);
    }

    #[test]
    fn gaussian_blur_spreads_energy_to_neighbors() {
        let mut pixels = vec![0u8; 5 * 5 * 4];
        let center = ((2usize * 5usize) + 2usize) * 4usize;
        pixels[center] = 255;
        pixels[center + 1] = 255;
        pixels[center + 2] = 255;
        pixels[center + 3] = 255;

        let pipeline = FilterPipeline {
            passes: vec![FilterPass::GaussianBlur(GaussianBlurPass {
                radius: 1,
                sigma: 1.0,
            })],
        };

        let report = apply_filter_pipeline_rgba(&pipeline, 5, 5, &mut pixels).expect("apply blur");
        assert_eq!(report.executed_passes, 1);
        assert_eq!(report.frost_passes, 1);

        let center_after = pixels[center];
        let left = ((2usize * 5usize) + 1usize) * 4usize;
        assert!(center_after < 255);
        assert!(pixels[left] > 0);
    }
}
