//! Following the panel without fighting the app over the renderer's CPU
//! timing tier.
//!
//! The renderer group wants sub-frame CPU scopes while it is open. The app
//! may have its own tier (a `?trace=` flag, a debug toggle), and closing the
//! group must give that back rather than switch timing off. Tiers are ranks
//! here (`0` off, `1` frame, `2` sub-frame) so the rule is testable without
//! the renderer; `renderer.rs` maps them to `TimingTier`.

// Only the `renderer` feature drives it; the rule is unit-tested either way.
#![cfg_attr(not(feature = "renderer"), allow(dead_code))]

/// Remembers the app's own tier and the one this collector last set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TierFollower {
    app: u8,
    set: Option<u8>,
}

impl TierFollower {
    /// The tier for this frame. `current` is what the renderer has now:
    /// anything other than what this follower set last came from the app,
    /// and becomes the app's tier. With `detail` the app's tier is raised to
    /// at least `detail_rank`; without it the app's tier is restored as is.
    pub(crate) fn next(&mut self, current: u8, detail: bool, detail_rank: u8) -> u8 {
        if self.set != Some(current) {
            self.app = current;
        }
        let want = if detail {
            self.app.max(detail_rank)
        } else {
            self.app
        };
        self.set = Some(want);
        want
    }
}

#[cfg(test)]
mod tests {
    use super::TierFollower;

    const OFF: u8 = 0;
    const FRAME: u8 = 1;
    const SUB: u8 = 2;

    #[test]
    fn no_app_tier_follows_the_group() {
        let mut f = TierFollower::default();
        assert_eq!(f.next(OFF, false, SUB), OFF);
        assert_eq!(f.next(OFF, true, SUB), SUB);
        assert_eq!(f.next(SUB, true, SUB), SUB);
        assert_eq!(f.next(SUB, false, SUB), OFF);
    }

    #[test]
    fn app_trace_survives_closing_the_group() {
        // The app traces at the frame tier (`?trace=frame`).
        let mut f = TierFollower::default();
        assert_eq!(f.next(FRAME, false, SUB), FRAME);
        assert_eq!(f.next(FRAME, true, SUB), SUB);
        // Closing the group gives the app its tier back, not off.
        assert_eq!(f.next(SUB, false, SUB), FRAME);
        assert_eq!(f.next(FRAME, false, SUB), FRAME);
    }

    #[test]
    fn app_sub_frame_is_never_lowered() {
        let mut f = TierFollower::default();
        assert_eq!(f.next(SUB, false, SUB), SUB);
        assert_eq!(f.next(SUB, true, SUB), SUB);
        assert_eq!(f.next(SUB, false, SUB), SUB);
    }

    #[test]
    fn an_app_change_mid_session_is_adopted() {
        let mut f = TierFollower::default();
        assert_eq!(f.next(OFF, true, SUB), SUB);
        // The app switches its own trace on to the frame tier while the group
        // is open: the renderer now reads something this follower didn't set.
        assert_eq!(f.next(FRAME, true, SUB), SUB);
        assert_eq!(f.next(SUB, false, SUB), FRAME);
    }
}
