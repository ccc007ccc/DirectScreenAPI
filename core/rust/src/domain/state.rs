use crate::api::{Decision, DisplayState, RectRegion, RouteResult, Status};

#[derive(Debug)]
pub struct RuntimeState {
    pub display: DisplayState,
    pub default_decision: Decision,
    pub regions: Vec<RectRegion>,
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            display: DisplayState::default(),
            default_decision: Decision::Pass,
            regions: Vec::new(),
        }
    }
}

impl RuntimeState {
    pub fn set_display(&mut self, display: DisplayState) -> Result<(), Status> {
        display.validate()?;
        self.display = display;
        Ok(())
    }

    pub fn clear_regions(&mut self) {
        self.regions.clear();
    }

    pub fn add_rect_region(&mut self, region: RectRegion) -> Result<(), Status> {
        region.validate()?;
        self.regions.push(region);
        Ok(())
    }

    pub fn route_point(&self, x: f32, y: f32) -> RouteResult {
        for region in self.regions.iter().rev() {
            if region.contains(x, y) {
                return RouteResult {
                    decision: region.decision,
                    region_id: region.region_id,
                };
            }
        }

        RouteResult {
            decision: self.default_decision,
            region_id: -1,
        }
    }
}
