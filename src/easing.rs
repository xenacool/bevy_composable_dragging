//! The curves that shape the progress of an animation.

/// Decelerating curve: fast at the start, settling gently at the end.
pub(crate) fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// The strength of a back curve whose highest point is `overshoot` beyond the end (`0.12` is
/// twelve percent of the distance travelled).
///
/// The curve `1 + (s + 1)(t - 1)^3 + s(t - 1)^2` peaks at `1 + 4s^3 / (27 (s + 1)^2)`, which grows
/// with `s`, so the strength is found by bisection.
fn back_strength(overshoot: f32) -> f32 {
    let peak = |strength: f32| 4.0 * strength.powi(3) / (27.0 * (strength + 1.0).powi(2));
    let (mut low, mut high) = (0.0_f32, 64.0_f32);
    for _ in 0..40 {
        let middle = f32::midpoint(low, high);
        if peak(middle) < overshoot {
            low = middle;
        } else {
            high = middle;
        }
    }
    f32::midpoint(low, high)
}

/// Decelerating curve that runs past the end by `overshoot` of the distance travelled and then
/// settles back onto it. Without an overshoot it is the plain [`ease_out`].
pub(crate) fn ease_out_back(t: f32, overshoot: f32) -> f32 {
    if overshoot <= 0.0 {
        return ease_out(t);
    }
    let strength = back_strength(overshoot);
    let away = t.clamp(0.0, 1.0) - 1.0;
    1.0 + (strength + 1.0) * away.powi(3) + strength * away.powi(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_curves_start_at_zero_and_end_at_one() {
        for overshoot in [0.0, 0.12, 0.3] {
            assert!(ease_out_back(0.0, overshoot).abs() < 1e-6);
            assert!((ease_out_back(1.0, overshoot) - 1.0).abs() < 1e-6);
        }
        assert!(ease_out(0.0).abs() < 1e-6 && (ease_out(1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn the_back_curve_peaks_exactly_at_the_overshoot_and_then_settles() {
        let samples: Vec<f32> = (0..=1000)
            .map(|step| ease_out_back(step as f32 / 1000.0, 0.12))
            .collect();
        let highest = (0..samples.len())
            .max_by(|a, b| samples[*a].total_cmp(&samples[*b]))
            .unwrap();
        let peak = samples[highest];
        assert!((peak - 1.12).abs() < 1e-3, "{peak}");
        assert!(samples[highest..].windows(2).all(|pair| pair[1] <= pair[0]));
        assert!(samples[..highest].windows(2).all(|pair| pair[1] >= pair[0]));
    }

    #[test]
    fn no_overshoot_is_the_plain_ease_out_and_progress_is_clamped() {
        assert_eq!(ease_out_back(0.4, 0.0), ease_out(0.4));
        assert!((ease_out_back(2.0, 0.12) - 1.0).abs() < 1e-6);
        assert!(ease_out_back(-1.0, 0.12).abs() < 1e-6);
    }
}
