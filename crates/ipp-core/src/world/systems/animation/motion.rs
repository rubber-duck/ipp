//! Analytical motion of a controller's sample position, in Host seconds.

use crate::ErrorReason;

/// Motion law for a bounded animation cursor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AnimationMotionMode {
    /// Direct manipulation holds the sample and retains its release velocity.
    Hold,
    /// Damped spring; frequency is in Hz and damping is a damping ratio.
    Spring {
        /// Destination sample position.
        target: f64,
        /// Natural frequency in cycles per Host second.
        frequency: f64,
        /// Ratio to critical damping; one is critically damped.
        damping: f64,
    },
    /// Exponentially decaying velocity; friction is in inverse seconds.
    Decay {
        /// Rate of velocity decay in inverse Host seconds.
        friction: f64,
    },
}

/// Semantic playback state. Position is the controller's existing `time`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimationMotionState {
    /// Analytical motion law.
    pub mode: AnimationMotionMode,
    /// Velocity in sample units per Host second; omission in a control preserves momentum.
    pub velocity: f64,
    /// Inclusive lower cursor bound.
    pub min: f64,
    /// Inclusive upper cursor bound.
    pub max: f64,
}

/// Retarget motion without replacing its current position or velocity by default.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimationMotionControl {
    /// Analytical motion law.
    pub mode: AnimationMotionMode,
    /// Inclusive lower cursor bound.
    pub min: f64,
    /// Inclusive upper cursor bound.
    pub max: f64,
    /// Explicit sample position; omission preserves the current position.
    pub position: Option<f64>,
    /// Velocity in sample units per Host second; omission in a control preserves momentum.
    pub velocity: Option<f64>,
}

impl AnimationMotionControl {
    /// Reject nonfinite, unbounded or inconsistent control parameters.
    pub fn validate(self) -> Result<(), ErrorReason> {
        AnimationMotionState {
            mode: self.mode,
            velocity: self.velocity.unwrap_or(0.0),
            min: self.min,
            max: self.max,
        }
        .validate(self.position.unwrap_or(self.min))
    }
}

impl AnimationMotionState {
    /// Validate saved state against its controller sample position.
    pub fn validate(self, position: f64) -> Result<(), ErrorReason> {
        let valid = self.velocity.is_finite()
            && self.velocity.abs() <= 1e15
            && [position, self.min, self.max]
                .iter()
                .all(|value| value.is_finite() && value.abs() <= 1e9)
            && self.min >= 0.0
            && self.min < self.max
            && (self.min..=self.max).contains(&position)
            && match self.mode {
                AnimationMotionMode::Hold => true,
                AnimationMotionMode::Spring {
                    target,
                    frequency,
                    damping,
                } => {
                    target.is_finite()
                        && (self.min..=self.max).contains(&target)
                        && frequency.is_finite()
                        && (0.001..=1000.0).contains(&frequency)
                        && damping.is_finite()
                        && (0.001..=100.0).contains(&damping)
                }
                AnimationMotionMode::Decay {
                    friction,
                } => friction.is_finite() && (0.001..=10000.0).contains(&friction),
            };
        if valid {
            Ok(())
        } else {
            Err(ErrorReason::InvalidValue)
        }
    }

    /// Exact solution over `dt`; no integration substeps or catch-up loop.
    /// Bounds absorb outward velocity, allowing a spring to return inward.
    pub(super) fn advance(&mut self, position: &mut f64, dt: f64) -> bool {
        // Every supported law has settled long before this horizon. Bound
        // intermediates even if a Host reports an exceptionally long frame.
        let dt = dt.min(1e9);
        let tolerance = (self.max - self.min).max(1.0) * 1e-5;
        let (next, velocity, destination) = match self.mode {
            AnimationMotionMode::Hold => return false,
            AnimationMotionMode::Decay {
                friction,
            } => {
                let decay = (-friction * dt).exp();
                (
                    *position - self.velocity * (-friction * dt).exp_m1() / friction,
                    self.velocity * decay,
                    None,
                )
            }
            AnimationMotionMode::Spring {
                target,
                frequency,
                damping,
            } => {
                let omega = std::f64::consts::TAU * frequency;
                let x = *position - target;
                let v = self.velocity;
                let (offset, velocity) = if (damping - 1.0).abs() < 1e-8 {
                    let decay = (-omega * dt).exp();
                    let b = v + omega * x;
                    ((x + b * dt) * decay, (v - omega * b * dt) * decay)
                } else if damping < 1.0 {
                    let alpha = omega * damping;
                    let beta = omega * (1.0 - damping * damping).sqrt();
                    let (sin, cos) = (beta * dt).sin_cos();
                    let decay = (-alpha * dt).exp();
                    (
                        decay * (x * cos + (v + alpha * x) * sin / beta),
                        decay * (v * cos - (alpha * v + omega * omega * x) * sin / beta),
                    )
                } else {
                    let sum = damping + (damping * damping - 1.0).sqrt();
                    let slow = -omega / sum;
                    let fast = -omega * sum;
                    let a = (v - fast * x) / (slow - fast);
                    let b = x - a;
                    let p = a * (slow * dt).exp();
                    let q = b * (fast * dt).exp();
                    (p + q, slow * p + fast * q)
                };
                (target + offset, velocity, Some(target))
            }
        };
        *position = next.clamp(self.min, self.max);
        self.velocity =
            if (next <= self.min && velocity < 0.0) || (next >= self.max && velocity > 0.0) {
                0.0
            } else {
                velocity
            };
        let settled = self.velocity.abs() <= tolerance
            && destination.is_none_or(|target| (*position - target).abs() <= tolerance);
        if settled {
            if let Some(target) = destination {
                *position = target;
            }
            self.velocity = 0.0;
        }
        settled
    }
}

#[cfg(test)]
#[path = "motion_tests.rs"]
mod tests;
