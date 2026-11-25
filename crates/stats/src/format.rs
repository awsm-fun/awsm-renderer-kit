//! How the panel writes numbers. These are the shared rules every project
//! used; the JS twin only displays the strings.

/// `value` with `decimals` decimals, as JavaScript's `toFixed` writes it
/// for the magnitudes the panel shows.
pub fn fixed(value: f64, decimals: usize) -> String {
    let text = format!("{value:.decimals$}");
    // "-0", "-0.0": JS writes those without the sign.
    if text.starts_with('-') && text[1..].chars().all(|c| c == '0' || c == '.') {
        text[1..].to_string()
    } else {
        text
    }
}

/// A duration with its unit: microseconds below a tenth of a millisecond,
/// then two decimals of a millisecond, then one from 10 ms up.
pub fn duration(ms: f64) -> String {
    if ms < 0.09995 {
        let us = ms * 1000.0;
        return format!("{} µs", fixed(us, if us < 9.95 { 1 } else { 0 }));
    }
    format!("{} ms", fixed(ms, if ms < 9.995 { 2 } else { 1 }))
}

/// `"<duration> / frame"`.
pub fn per_frame(ms: f64) -> String {
    format!("{} / frame", duration(ms))
}

/// `"<duration> / step"`.
pub fn per_step(ms: f64) -> String {
    format!("{} / step", duration(ms))
}

/// For the frame budget's rows: milliseconds to one decimal, never reading
/// as nothing when it is something.
pub fn tenths(ms: f64) -> String {
    if ms > 0.0 && ms < 0.05 {
        "<0.1".to_string()
    } else {
        fixed(ms, 1)
    }
}

/// A share of the budget as a percentage, never reading as nothing when it
/// is something.
pub fn share(ratio: f64) -> String {
    if ratio > 0.0 && ratio < 0.005 {
        "<1%".to_string()
    } else {
        format!("{}%", (ratio * 100.0).round())
    }
}

/// 2 → "2nd", 3 → "3rd", 4 → "4th" (the pacing row: every Nth refresh).
pub fn ordinal(n: u32) -> String {
    let suffix = match n {
        2 => "nd",
        3 => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// A value split for display: numbers (`true`) bright, the units and words
/// around them dim. A number is an optional `<`, a digit, more digits and
/// dots, and an optional `%` (the rule `/<?\d[\d.]*%?/` every project used).
pub fn value_pieces(text: &str) -> Vec<(bool, &str)> {
    let bytes = text.as_bytes();
    let mut pieces = Vec::new();
    let mut plain_start = 0;
    let mut i = 0;
    while i < bytes.len() {
        let starts = if bytes[i] == b'<' {
            bytes.get(i + 1).is_some_and(u8::is_ascii_digit)
        } else {
            bytes[i].is_ascii_digit()
        };
        if !starts {
            i += 1;
            continue;
        }
        let start = i;
        if bytes[i] == b'<' {
            i += 1;
        }
        i += 1;
        while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b'%' {
            i += 1;
        }
        if plain_start < start {
            pieces.push((false, &text[plain_start..start]));
        }
        pieces.push((true, &text[start..i]));
        plain_start = i;
    }
    if plain_start < text.len() {
        pieces.push((false, &text[plain_start..]));
    }
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(duration(0.0), "0.0 µs");
        assert_eq!(duration(0.0042), "4.2 µs");
        assert_eq!(duration(0.012), "12 µs");
        assert_eq!(duration(0.0999), "100 µs");
        assert_eq!(duration(0.1), "0.10 ms");
        assert_eq!(duration(1.234), "1.23 ms");
        assert_eq!(duration(9.99), "9.99 ms");
        assert_eq!(duration(9.996), "10.0 ms");
        assert_eq!(duration(16.66), "16.7 ms");
        assert_eq!(per_frame(2.0), "2.00 ms / frame");
        assert_eq!(per_step(0.5), "0.50 ms / step");
    }

    #[test]
    fn budget_figures() {
        assert_eq!(tenths(0.0), "0.0");
        assert_eq!(tenths(0.01), "<0.1");
        assert_eq!(tenths(4.26), "4.3");
        assert_eq!(share(0.0), "0%");
        assert_eq!(share(0.001), "<1%");
        assert_eq!(share(0.256), "26%");
        assert_eq!(share(1.5), "150%");
        assert_eq!(fixed(-0.01, 1), "0.0");
        assert_eq!(fixed(-1.25, 1), "-1.2");
    }

    #[test]
    fn ordinals() {
        assert_eq!(ordinal(2), "2nd");
        assert_eq!(ordinal(3), "3rd");
        assert_eq!(ordinal(4), "4th");
    }

    #[test]
    fn pieces() {
        assert_eq!(
            value_pieces("16.7 ms (60 Hz) · headroom 9.5 ms"),
            vec![
                (true, "16.7"),
                (false, " ms ("),
                (true, "60"),
                (false, " Hz) · headroom "),
                (true, "9.5"),
                (false, " ms"),
            ]
        );
        assert_eq!(
            value_pieces("<0.1 / 16.7 ms (<1%)"),
            vec![
                (true, "<0.1"),
                (false, " / "),
                (true, "16.7"),
                (false, " ms ("),
                (true, "<1%"),
                (false, ")"),
            ]
        );
        assert_eq!(value_pieces("—"), vec![(false, "—")]);
        assert_eq!(value_pieces("n/a"), vec![(false, "n/a")]);
        assert_eq!(value_pieces("a < b"), vec![(false, "a < b")]);
        assert_eq!(value_pieces("12%"), vec![(true, "12%")]);
    }
}
