use crate::api::{DisplayState, RouteResult, Status};

pub mod filter;
pub mod vulkan;

use self::filter::{apply_filter_pipeline_rgba, FilterPipeline, FilterReport};

pub trait RenderBackend: Send {
    fn on_display_changed(&mut self, _display: DisplayState) -> Result<(), Status> {
        Ok(())
    }

    fn process_frame_rgba(
        &mut self,
        width: u32,
        height: u32,
        pixels_rgba8: &mut [u8],
    ) -> Result<FilterReport, Status> {
        let _ = (width, height, pixels_rgba8);
        Ok(FilterReport::default())
    }
}

pub trait InputBackend: Send {
    fn on_route_result(&mut self, _result: RouteResult) -> Result<(), Status> {
        Ok(())
    }
}

#[derive(Default)]
pub struct NullBackend {
    filter_pipeline: FilterPipeline,
}

impl NullBackend {
    pub fn set_filter_pipeline(&mut self, pipeline: FilterPipeline) {
        self.filter_pipeline = pipeline;
    }
}

impl RenderBackend for NullBackend {
    fn process_frame_rgba(
        &mut self,
        width: u32,
        height: u32,
        pixels_rgba8: &mut [u8],
    ) -> Result<FilterReport, Status> {
        apply_filter_pipeline_rgba(&self.filter_pipeline, width, height, pixels_rgba8)
    }
}

impl InputBackend for NullBackend {}
