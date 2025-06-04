use std::{cmp::Ordering, collections::HashMap};

pub struct MetricStatistics {
    scores: Vec<f64>,
    cache:  HashMap<String, f64>,
}

impl MetricStatistics {
    pub fn new(scores: Vec<f64>) -> Self {
        MetricStatistics {
            scores,
            cache: HashMap::new(),
        }
    }

    fn get_or_compute(&mut self, key: &str, compute: impl FnOnce(&[f64]) -> f64) -> f64 {
        *self.cache.entry(key.to_string()).or_insert_with(|| compute(&self.scores))
    }

    pub fn average(&mut self) -> f64 {
        self.get_or_compute("average", |scores| {
            scores.iter().sum::<f64>() / scores.len() as f64
        })
    }

    #[allow(dead_code)]
    pub fn median(&mut self) -> f64 {
        self.percentile(50)
    }

    #[allow(dead_code)]
    pub fn minimum(&mut self) -> f64 {
        self.get_or_compute("minimum", |scores| {
            *scores.iter().min_by(|a, b| a.partial_cmp(b).unwrap()).unwrap_or(&0.0)
        })
    }

    #[allow(dead_code)]
    pub fn maximum(&mut self) -> f64 {
        self.get_or_compute("maximum", |scores| {
            *scores.iter().max_by(|a, b| a.partial_cmp(b).unwrap()).unwrap_or(&0.0)
        })
    }

    #[allow(dead_code)]
    pub fn variance(&mut self) -> f64 {
        let average = self.average();
        self.get_or_compute("variance", |scores| {
            scores
                .iter()
                .map(|x| {
                    let diff = x - average;
                    diff * diff
                })
                .sum::<f64>()
                / scores.len() as f64
        })
    }

    #[allow(dead_code)]
    pub fn standard_deviation(&mut self) -> f64 {
        let variance = self.variance();
        self.get_or_compute("standard_deviation", |_| variance.sqrt())
    }

    pub fn percentile(&mut self, index: usize) -> f64 {
        let mut sorted_scores = self.scores.clone();
        sorted_scores.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Less));
        self.get_or_compute(&format!("percentile_{index}"), |scores| {
            let index = (index as f64 / 100.0 * scores.len() as f64) as usize;
            *sorted_scores.get(index).unwrap_or(&sorted_scores[0])
        })
    }
}
