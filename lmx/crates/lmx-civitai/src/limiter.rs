//! Minimum-interval rate limiter that reserves its slot *before* sleeping.
//!
//! Reading the next slot, sleeping, then writing it lets concurrent callers all read the same
//! slot and fire together; reserving first keeps them spaced even under `join_all`.

use std::time::Duration;

use tokio::{sync::Mutex, time::Instant};

pub struct Limiter {
    next: Mutex<Instant>,
    interval: Duration,
}

impl Limiter {
    pub fn new(interval: Duration) -> Limiter {
        Limiter { next: Mutex::new(Instant::now()), interval }
    }

    /// Wait until this caller's reserved slot.
    pub async fn wait(&self) {
        let slot = {
            let mut next = self.next.lock().await;
            let now = Instant::now();
            let slot = if *next > now { *next } else { now };
            *next = slot + self.interval;
            slot
        };
        tokio::time::sleep_until(slot).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test(start_paused = true)]
    async fn concurrent_callers_are_spaced() {
        let lim = Arc::new(Limiter::new(Duration::from_millis(250)));
        let start = Instant::now();
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let l = lim.clone();
                tokio::spawn(async move {
                    l.wait().await;
                    Instant::now().duration_since(start)
                })
            })
            .collect();
        let mut times = Vec::new();
        for h in handles {
            times.push(h.await.unwrap());
        }
        times.sort();
        for (i, t) in times.iter().enumerate() {
            assert_eq!(t.as_millis(), (i as u128) * 250, "{times:?}");
        }
    }
}
