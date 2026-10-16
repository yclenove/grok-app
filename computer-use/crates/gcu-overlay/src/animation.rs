//! Analytic critically damped spring, driven exclusively by Host monotonic time.
use gcu_host_api::ScreenPoint;
use std::time::Duration;

/// Frame pacing hint. Scheduling never participates in state transitions.
pub const FRAME_INTERVAL: Duration = Duration::from_micros(16_667);

/// A spring with continuous position and velocity when retargeted.
#[derive(Clone, Debug)]
pub struct Spring {
    start: ScreenPoint,
    target: ScreenPoint,
    velocity: ScreenPoint,
    epoch: Duration,
    omega: f64,
}
impl Spring {
    /// Construct at rest. Response is clamped to 0.05–2 seconds; NaN uses 0.35.
    pub fn new(at: ScreenPoint, now: Duration, response: f64) -> Self {
        let response = if response.is_finite() {
            response.clamp(0.05, 2.)
        } else {
            0.35
        };
        Self {
            start: at,
            target: at,
            velocity: ScreenPoint { x: 0., y: 0. },
            epoch: now,
            omega: 8. / response,
        }
    }
    fn axis(&self, start: f64, target: f64, velocity: f64, now: Duration) -> (f64, f64) {
        let t = now.saturating_sub(self.epoch).as_secs_f64();
        let delta = start - target;
        let c = velocity + self.omega * delta;
        let decay = (-self.omega * t).exp();
        (
            target + (delta + c * t) * decay,
            (velocity - self.omega * c * t) * decay,
        )
    }
    /// Pure sample; missed frames do not change the trajectory.
    pub fn sample(&self, now: Duration) -> ScreenPoint {
        if now <= self.epoch {
            return self.start;
        }
        ScreenPoint {
            x: self
                .axis(self.start.x, self.target.x, self.velocity.x, now)
                .0,
            y: self
                .axis(self.start.y, self.target.y, self.velocity.y, now)
                .0,
        }
    }
    /// Retarget from the exact current position and velocity, without a jump.
    pub fn retarget(&mut self, target: ScreenPoint, now: Duration) {
        let (x, vx) = self.axis(self.start.x, self.target.x, self.velocity.x, now);
        let (y, vy) = self.axis(self.start.y, self.target.y, self.velocity.y, now);
        self.start = ScreenPoint { x, y };
        self.velocity = ScreenPoint { x: vx, y: vy };
        self.target = target;
        self.epoch = now;
    }
}
