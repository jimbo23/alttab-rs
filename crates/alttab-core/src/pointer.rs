pub const HOVER_DEAD_ZONE: f64 = 25.0;

#[derive(Clone, Debug)]
pub struct PointerGate {
    dead_zone: f64,
    anchor: Option<(f64, f64)>,
    enabled: bool,
}
impl PointerGate {
    pub fn new(dead_zone: f64) -> Self {
        Self {
            dead_zone,
            anchor: None,
            enabled: false,
        }
    }
    pub fn reset(&mut self) {
        self.anchor = None;
        self.enabled = false;
    }
    pub fn moved(&mut self, x: f64, y: f64) -> bool {
        if self.enabled {
            return true;
        }
        let Some((anchor_x, anchor_y)) = self.anchor else {
            self.anchor = Some((x, y));
            return false;
        };
        self.enabled = (x - anchor_x).hypot(y - anchor_y) > self.dead_zone;
        self.enabled
    }
}
