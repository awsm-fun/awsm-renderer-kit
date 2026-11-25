//! Smoothing: every figure on the panel is an exponential moving average.

use std::collections::HashMap;

/// Smoothing weight of each new window: a real change shows within 2–3
/// windows, while the readout holds steady against noise.
pub const EMA_ALPHA: f64 = 0.35;

/// The next average after `value` (the first value is taken as it is).
pub fn smooth(ema: Option<f64>, value: f64) -> f64 {
    match ema {
        None => value,
        Some(ema) => ema + (value - ema) * EMA_ALPHA,
    }
}

/// Averages by key, for an app's own figures.
#[derive(Clone, Debug, Default)]
pub struct Smoother {
    values: HashMap<String, f64>,
}

impl Smoother {
    /// The next average of `key` after `value`.
    pub fn smooth(&mut self, key: &str, value: f64) -> f64 {
        let next = smooth(self.values.get(key).copied(), value);
        self.values.insert(key.to_string(), next);
        next
    }

    /// The current average of `key`.
    pub fn get(&self, key: &str) -> Option<f64> {
        self.values.get(key).copied()
    }

    pub fn remove(&mut self, key: &str) {
        self.values.remove(key);
    }

    pub fn clear(&mut self) {
        self.values.clear();
    }
}

/// A named breakdown (passes, scopes, phases): the averages of the entries in
/// the latest window, in that window's order. An entry missing from a
/// window is no longer being measured (its group closed, or the pass did not
/// run), so it drops out rather than show a stale figure.
#[derive(Clone, Debug, Default)]
pub struct NamedMeans {
    entries: Vec<(String, f64)>,
}

impl NamedMeans {
    /// Replaces the breakdown with `latest`, each entry smoothed from its
    /// previous average.
    pub fn update(&mut self, latest: impl IntoIterator<Item = (String, f64)>) {
        let previous = std::mem::take(&mut self.entries);
        self.entries = latest
            .into_iter()
            .map(|(name, value)| {
                let before = previous.iter().find(|(n, _)| *n == name).map(|e| e.1);
                let mean = smooth(before, value);
                (name, mean)
            })
            .collect();
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// In the latest window's order.
    pub fn entries(&self) -> &[(String, f64)] {
        &self.entries
    }

    /// Slowest first.
    pub fn slowest_first(&self) -> Vec<(String, f64)> {
        let mut sorted = self.entries.clone();
        sorted.sort_by(|a, b| b.1.total_cmp(&a.1));
        sorted
    }

    pub fn total(&self) -> f64 {
        self.entries.iter().map(|e| e.1).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ema() {
        assert_eq!(smooth(None, 10.0), 10.0);
        assert!((smooth(Some(10.0), 20.0) - 13.5).abs() < 1e-12);
        let mut s = Smoother::default();
        assert_eq!(s.smooth("a", 4.0), 4.0);
        assert!((s.smooth("a", 0.0) - 2.6).abs() < 1e-12);
        assert_eq!(s.get("b"), None);
    }

    #[test]
    fn named_means_drop_missing_entries() {
        let mut m = NamedMeans::default();
        m.update([("a".to_string(), 1.0), ("b".to_string(), 3.0)]);
        m.update([("b".to_string(), 1.0), ("c".to_string(), 5.0)]);
        let e = m.entries();
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].0, "b");
        assert!((e[0].1 - 2.3).abs() < 1e-12);
        assert_eq!(e[1], ("c".to_string(), 5.0));
        assert_eq!(m.slowest_first()[0].0, "c");
    }
}
