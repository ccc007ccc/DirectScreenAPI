use crate::api::{Decision, DisplayState, RectRegion, RouteResult, Status};
use crate::domain::RuntimeState;

#[derive(Debug, Default)]
pub struct RuntimeEngine {
    state: RuntimeState,
}

impl RuntimeEngine {
    pub fn set_display_state(&mut self, display: DisplayState) -> Result<(), Status> {
        self.state.set_display(display)
    }

    pub fn display_state(&self) -> DisplayState {
        self.state.display
    }

    pub fn set_default_decision(&mut self, decision: Decision) {
        self.state.default_decision = decision;
    }

    pub fn clear_regions(&mut self) {
        self.state.clear_regions();
    }

    pub fn add_region_rect(&mut self, region: RectRegion) -> Result<(), Status> {
        self.state.add_rect_region(region)
    }

    pub fn route_point(&self, x: f32, y: f32) -> RouteResult {
        self.state.route_point(x, y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_prefers_last_added_region() {
        let mut engine = RuntimeEngine::default();
        engine
            .add_region_rect(RectRegion {
                region_id: 1,
                decision: Decision::Pass,
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 100.0,
            })
            .expect("region 1");
        engine
            .add_region_rect(RectRegion {
                region_id: 2,
                decision: Decision::Block,
                x: 10.0,
                y: 10.0,
                w: 20.0,
                h: 20.0,
            })
            .expect("region 2");

        let res = engine.route_point(15.0, 15.0);
        assert_eq!(res.decision, Decision::Block);
        assert_eq!(res.region_id, 2);
    }
}
