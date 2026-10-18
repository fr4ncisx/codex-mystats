use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
pub struct HeatColor {
    pub deep: [u8; 3],
    pub mid: [u8; 3],
    pub light: [u8; 3],
}

#[derive(Deserialize)]
struct Colors {
    steps: [HeatColor; 10],
    warning: HeatColor,
    threshold: HeatColor,
    zero: HeatColor,
}

fn colors() -> &'static Colors {
    static COLORS: OnceLock<Colors> = OnceLock::new();
    COLORS.get_or_init(|| {
        serde_json::from_str(include_str!("../frontend/usage-colors.json"))
            .expect("valid bundled palette")
    })
}

pub fn palette(remaining: f64, threshold: f64) -> HeatColor {
    let value = remaining.clamp(0.0, 100.0);
    let limit = threshold.clamp(0.0, 100.0);
    let colors = colors();
    if value > limit + 5.0 {
        return colors.steps[(value / 10.0).floor().min(9.0) as usize];
    }
    let (from, to, amount) = if value > limit {
        (
            colors.warning,
            colors.threshold,
            (limit + 5.0 - value) / 5.0,
        )
    } else {
        (
            colors.threshold,
            colors.zero,
            if limit == 0.0 {
                1.0
            } else {
                (limit - value) / limit
            },
        )
    };
    HeatColor {
        deep: blend(from.deep, to.deep, amount),
        mid: blend(from.mid, to.mid, amount),
        light: blend(from.light, to.light, amount),
    }
}

pub fn blend(from: [u8; 3], to: [u8; 3], amount: f64) -> [u8; 3] {
    std::array::from_fn(|index| {
        (f64::from(from[index]) + (f64::from(to[index]) - f64::from(from[index])) * amount).round()
            as u8
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_browser_cases_match_native_colors() {
        let cases: serde_json::Value =
            serde_json::from_str(include_str!("../tests/usage-color-cases.json")).unwrap();
        for case in cases.as_array().unwrap() {
            let actual = palette(
                case["remaining"].as_f64().unwrap(),
                case["threshold"].as_f64().unwrap(),
            );
            assert_eq!(serde_json::json!(actual.mid), case["mid"], "{case}");
        }
    }

    #[test]
    fn relative_alert_boundaries_and_zero_threshold_are_exact() {
        for threshold in [0.0, 10.0, 20.0, 75.0, 100.0] {
            assert_eq!(palette(0.0, threshold).mid, [255, 48, 72]);
            if threshold > 0.0 {
                assert_eq!(palette(threshold, threshold).mid, [228, 83, 83]);
            }
            if threshold <= 95.0 {
                assert_eq!(palette(threshold + 5.0, threshold).mid, [220, 112, 64]);
            }
        }
        assert_eq!(palette(84.0, 20.0).mid, [99, 181, 131]);
        assert_ne!(palette(23.0, 20.0), palette(23.0, 50.0));
    }
}
