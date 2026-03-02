use crate::api::Status;

use super::filter::{apply_filter_pipeline_rgba, FilterPipeline, FilterReport};

#[derive(Debug, Clone, Default)]
pub struct VulkanBackend {
    filter_pipeline: FilterPipeline,
}

impl VulkanBackend {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn backend_name(&self) -> &'static str {
        "vulkan-bootstrap-cpu-fallback"
    }

    pub fn gpu_path_active(&self) -> bool {
        false
    }

    pub fn set_filter_pipeline(&mut self, pipeline: FilterPipeline) {
        self.filter_pipeline = pipeline;
    }

    pub fn filter_pipeline(&self) -> &FilterPipeline {
        &self.filter_pipeline
    }

    pub fn process_frame_rgba(
        &self,
        width: u32,
        height: u32,
        pixels_rgba8: &mut [u8],
    ) -> Result<FilterReport, Status> {
        apply_filter_pipeline_rgba(&self.filter_pipeline, width, height, pixels_rgba8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::filter::{FilterPass, GaussianBlurPass};

    #[test]
    fn vulkan_backend_bootstrap_processes_filter_pipeline() {
        let mut backend = VulkanBackend::new();
        backend.set_filter_pipeline(FilterPipeline {
            passes: vec![FilterPass::GaussianBlur(GaussianBlurPass {
                radius: 1,
                sigma: 1.0,
            })],
        });

        let mut pixels = vec![0u8; 3 * 3 * 4];
        let center = 4usize * 4usize;
        pixels[center] = 255;
        pixels[center + 1] = 255;
        pixels[center + 2] = 255;
        pixels[center + 3] = 255;

        let report = backend
            .process_frame_rgba(3, 3, &mut pixels)
            .expect("process frame");
        assert_eq!(report.frost_passes, 1);
        assert!(pixels[center] < 255);
    }
}
