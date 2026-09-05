use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessIdentity {
    pub pid: u32,
    pub start_time_ticks: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessObservation {
    pub pid: u32,
    pub start_time_ticks: u64,
    pub state: char,
    /// Earliest plausible start time, accounting for `/proc/uptime` precision.
    pub start_unix_nanos: Option<u128>,
}

impl ProcessObservation {
    pub fn identity(self) -> ProcessIdentity {
        ProcessIdentity {
            pid: self.pid,
            start_time_ticks: self.start_time_ticks,
        }
    }
}

pub fn current() -> Option<ProcessObservation> {
    observe(std::process::id())
}

pub fn observe(pid: u32) -> Option<ProcessObservation> {
    let raw = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (state, start_time_ticks) = parse_stat(&raw)?;
    Some(ProcessObservation {
        pid,
        start_time_ticks,
        state,
        start_unix_nanos: process_start_unix_nanos(start_time_ticks),
    })
}

pub fn alive(identity: ProcessIdentity) -> bool {
    observe(identity.pid).is_some_and(|process| {
        process.state != 'Z' && process.start_time_ticks == identity.start_time_ticks
    })
}

/// Field 2 can contain spaces and parentheses, so its final `) ` is the only
/// safe anchor before state field 3 and start-time field 22.
pub fn parse_stat(raw: &str) -> Option<(char, u64)> {
    let close = raw.rfind(") ")?;
    let mut fields = raw.get(close + 2..)?.split_whitespace();
    let state_text = fields.next()?;
    let mut state_chars = state_text.chars();
    let state = state_chars.next()?;
    if state_chars.next().is_some() {
        return None;
    }
    Some((state, fields.nth(18)?.parse().ok()?))
}

fn process_start_unix_nanos(start_time_ticks: u64) -> Option<u128> {
    let ticks_per_second = clock_ticks_per_second()? as u128;
    if ticks_per_second == 0 {
        return None;
    }
    let now_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let uptime = std::fs::read_to_string("/proc/uptime").ok()?;
    let uptime_nanos = decimal_seconds_to_nanos(uptime.split_whitespace().next()?)?;
    let started_since_boot = (start_time_ticks as u128)
        .checked_mul(1_000_000_000)?
        .checked_div(ticks_per_second)?;
    earliest_start_time(now_nanos, uptime_nanos, started_since_boot)
}

fn earliest_start_time(
    now_before_read: u128,
    uptime_floor: u128,
    started_since_boot: u128,
) -> Option<u128> {
    // Linux truncates uptime to hundredths. Use its upper bound so a fresh
    // legacy lock is not mistaken for a lock from before the process existed.
    now_before_read
        .checked_sub(uptime_floor.checked_add(10_000_000)?)?
        .checked_add(started_since_boot)
}

fn decimal_seconds_to_nanos(raw: &str) -> Option<u128> {
    let (seconds, fraction) = raw.split_once('.').unwrap_or((raw, ""));
    let seconds = seconds.parse::<u128>().ok()?;
    let mut nanos = fraction.bytes().take(9).try_fold(0_u128, |value, digit| {
        digit
            .is_ascii_digit()
            .then_some(value * 10 + u128::from(digit - b'0'))
    })?;
    for _ in fraction.len().min(9)..9 {
        nanos *= 10;
    }
    seconds.checked_mul(1_000_000_000)?.checked_add(nanos)
}

fn clock_ticks_per_second() -> Option<u64> {
    const AT_CLKTCK: u64 = 17;
    let raw = std::fs::read("/proc/self/auxv").ok()?;
    let word_bytes = std::mem::size_of::<usize>();
    for entry in raw.chunks_exact(word_bytes.checked_mul(2)?) {
        let key = native_word(entry.get(..word_bytes)?)?;
        let value = native_word(entry.get(word_bytes..)?)?;
        if key == AT_CLKTCK {
            return (value != 0).then_some(value);
        }
        if key == 0 {
            break;
        }
    }
    None
}

fn native_word(raw: &[u8]) -> Option<u64> {
    match raw.len() {
        8 => Some(u64::from_ne_bytes(raw.try_into().ok()?)),
        4 => Some(u32::from_ne_bytes(raw.try_into().ok()?).into()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uptime_precision_cannot_place_a_live_process_after_its_new_lock() {
        let wall_before_read = 10_005_000_000;
        let lock_created = 10_001_000_000;
        let earliest = earliest_start_time(wall_before_read, 1_000_000_000, 1_000_000_000).unwrap();
        assert_eq!(earliest, 9_995_000_000);
        assert!(earliest <= lock_created);
    }

    #[test]
    fn stat_parser_handles_spaces_and_parentheses_in_comm() {
        let mut trailing = vec!["0"; 18];
        trailing.push("424242");
        let raw = format!("77 (echo worker (old)) S {}", trailing.join(" "));
        assert_eq!(parse_stat(&raw), Some(('S', 424242)));
    }
}
