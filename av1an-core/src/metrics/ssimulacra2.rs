use std::cmp::Ordering;

pub struct CommonStatistics {
    pub average:            f64,
    pub median:             f64,
    pub minimum:            f64,
    pub maximum:            f64,
    pub standard_deviation: f64,
    pub variance:           f64,
    pub percentile_1:       f64,
    pub percentile_10:      f64,
    pub percentile_25:      f64,
}

/// Return some common statistics from a vector of f64
pub fn get_common_statistics(mut scores: Vec<f64>) -> CommonStatistics {
    assert!(!scores.is_empty());

    let sum: f64 = scores.iter().sum();
    let average = sum / scores.len() as f64;

    let mid = scores.len() / 2;
    scores.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Less));
    let median = if scores.len() % 2 == 0 {
        (scores[mid - 1] + scores[mid]) / 2.0
    } else {
        scores[mid]
    };

    let minimum = scores[0];
    let maximum = scores[scores.len() - 1];

    let variance = scores
        .iter()
        .map(|x| {
            let diff = x - average;
            diff * diff
        })
        .sum::<f64>()
        / scores.len() as f64;

    let stddev = variance.sqrt();

    let percentile_1_index = ((scores.len() - 1) as f64 * 0.01) as usize;
    let percentile_1 = *scores.get(percentile_1_index).unwrap_or(&scores[0]);

    let percentile_10_index = ((scores.len() - 1) as f64 * 0.1) as usize;
    let percentile_10 = *scores.get(percentile_10_index).unwrap_or(&scores[0]);

    let percentile_25_index = ((scores.len() - 1) as f64 * 0.25) as usize;
    let percentile_25 = *scores.get(percentile_25_index).unwrap_or(&scores[0]);

    CommonStatistics {
        average,
        median,
        minimum,
        maximum,
        standard_deviation: stddev,
        variance,
        percentile_1,
        percentile_10,
        percentile_25,
    }
}
