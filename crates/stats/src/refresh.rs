//! The display's refresh period, from the page's own animation frames.
//!
//! No browser API reports the refresh rate, so the page runs its own
//! (otherwise empty) `requestAnimationFrame` loop and sends the gaps between
//! its frames to the worker. The page does little per frame, so its frames
//! land on every refresh even when the renderer's do not.

use std::collections::VecDeque;

/// Gaps kept, and how many are needed before there is an estimate.
pub const REFRESH_SAMPLES: usize = 120;
pub const REFRESH_MIN_SAMPLES: usize = 30;
/// Gaps within this share of the median count as single refreshes.
pub const REFRESH_INLIER: f64 = 0.15;

/// The mean of the recent gaps that sit within [`REFRESH_INLIER`] of their
/// median, which ignores a frame the page missed and a tab switch.
#[derive(Clone, Debug, Default)]
pub struct RefreshEstimator {
    gaps: VecDeque<f64>,
}

impl RefreshEstimator {
    /// Adds gaps (ms) in the order they happened. Non-positive and
    /// non-finite ones are dropped.
    pub fn push(&mut self, gaps: impl IntoIterator<Item = f64>) {
        for gap in gaps {
            if gap.is_finite() && gap > 0.0 {
                self.gaps.push_back(gap);
                if self.gaps.len() > REFRESH_SAMPLES {
                    self.gaps.pop_front();
                }
            }
        }
    }

    /// The refresh period in ms, or `None` until enough gaps were seen.
    pub fn period_ms(&self) -> Option<f64> {
        if self.gaps.len() < REFRESH_MIN_SAMPLES {
            return None;
        }
        let mut sorted: Vec<f64> = self.gaps.iter().copied().collect();
        sorted.sort_by(f64::total_cmp);
        let median = sorted[sorted.len() >> 1];
        let (mut sum, mut count) = (0.0, 0u32);
        for &gap in &self.gaps {
            if (gap - median).abs() <= median * REFRESH_INLIER {
                sum += gap;
                count += 1;
            }
        }
        (count > 0 && sum > 0.0).then(|| sum / f64::from(count))
    }

    pub fn clear(&mut self) {
        self.gaps.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needs_thirty_gaps() {
        let mut r = RefreshEstimator::default();
        r.push(std::iter::repeat_n(16.67, 29));
        assert_eq!(r.period_ms(), None);
        r.push([16.67]);
        assert!((r.period_ms().unwrap() - 16.67).abs() < 1e-9);
    }

    #[test]
    fn ignores_outliers_and_keeps_the_last_120() {
        let mut r = RefreshEstimator::default();
        let mut gaps = vec![8.33; 100];
        gaps.extend([33.3, 250.0, 4.0, -1.0, f64::NAN]);
        r.push(gaps);
        assert!((r.period_ms().unwrap() - 8.33).abs() < 1e-9);
        r.push(std::iter::repeat_n(16.0, 120));
        assert!((r.period_ms().unwrap() - 16.0).abs() < 1e-9);
    }
}
