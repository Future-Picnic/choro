pub(super) fn branch_relative_time(unix_secs: i64) -> String {
    if unix_secs <= 0 {
        return String::new();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let delta = (now - unix_secs).max(0);
    match delta {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{}m ago", delta / 60),
        3600..=86_399 => format!("{}h ago", delta / 3600),
        86_400..=2_591_999 => format!("{}d ago", delta / 86_400),
        _ => format!("{}mo ago", delta / 2_592_000),
    }
}

pub(super) fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub(super) fn compact_duration(seconds: u64) -> String {
    match seconds {
        0..=59 => format!("{seconds}s"),
        60..=3599 => format!("{}m {}s", seconds / 60, seconds % 60),
        _ => format!("{}h {}m", seconds / 3600, (seconds % 3600) / 60),
    }
}

pub(super) fn chat_message_time_label(unix_secs: u64) -> String {
    if unix_secs == 0 {
        return String::new();
    }
    #[cfg(unix)]
    {
        let timestamp = unix_secs as libc::time_t;
        let mut local_time = std::mem::MaybeUninit::<libc::tm>::uninit();
        let local_time = unsafe {
            if libc::localtime_r(&timestamp, local_time.as_mut_ptr()).is_null() {
                return branch_relative_time(unix_secs as i64);
            }
            local_time.assume_init()
        };
        let hour = local_time.tm_hour;
        let minute = local_time.tm_min;
        let suffix = if hour >= 12 { "PM" } else { "AM" };
        let hour_12 = match hour % 12 {
            0 => 12,
            value => value,
        };
        format!("{hour_12}:{minute:02} {suffix}")
    }
    #[cfg(not(unix))]
    {
        branch_relative_time(unix_secs as i64)
    }
}
