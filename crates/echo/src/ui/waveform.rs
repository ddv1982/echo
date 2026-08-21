use std::f32::consts::PI;

#[derive(Debug, Clone)]
pub struct RmsRing {
    samples: Vec<f32>,
    cap: usize,
    next: usize,
    filled: usize,
}

impl RmsRing {
    #[must_use]
    pub fn new(cap: usize) -> Self {
        let cap = cap.max(1);
        Self {
            samples: vec![0.0; cap],
            cap,
            next: 0,
            filled: 0,
        }
    }

    pub fn push(&mut self, rms: f32) {
        self.samples[self.next] = rms.max(0.0);
        self.next = (self.next + 1) % self.cap;
        self.filled = (self.filled + 1).min(self.cap);
    }

    #[must_use]
    pub fn bars(&self, count: usize) -> Vec<f32> {
        if self.filled == 0 || count == 0 {
            return vec![0.0; count];
        }
        let mut ordered = Vec::with_capacity(self.filled);
        let start = (self.next + self.cap - self.filled) % self.cap;
        for i in 0..self.filled {
            ordered.push(self.samples[(start + i) % self.cap]);
        }
        let peak = ordered.iter().copied().fold(0.0f32, f32::max).max(1e-6);
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let idx = i * self.filled / count;
            out.push((ordered[idx] / peak).clamp(0.0, 1.0));
        }
        out
    }
}

#[must_use]
pub fn sine_rms_fixture(len: usize, cycles: f32) -> Vec<f32> {
    (0..len)
        .map(|i| {
            let t = i as f32 / len.max(1) as f32;
            ((2.0 * PI * cycles * t).sin().abs() * 0.6) + 0.05
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_fixture_makes_nonzero_bars() {
        let mut ring = RmsRing::new(32);
        for sample in sine_rms_fixture(32, 2.0) {
            ring.push(sample);
        }
        let bars = ring.bars(8);
        assert_eq!(bars.len(), 8);
        assert!(bars.iter().any(|b| *b > 0.2));
        assert!(bars.iter().all(|b| (0.0..=1.0).contains(b)));
    }

    #[test]
    fn empty_ring_is_flat() {
        let ring = RmsRing::new(8);
        assert_eq!(ring.bars(4), vec![0.0, 0.0, 0.0, 0.0]);
    }
}
