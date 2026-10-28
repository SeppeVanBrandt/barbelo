//! The status bar clock.

use std::ffi::CString;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::app::AppEvent;

/// The local time, formatted with `strftime` codes such as `%H:%M`.
pub fn now(format: &str) -> String {
    let Ok(format) = CString::new(format) else {
        return String::new();
    };
    // SAFETY: `tm` is plain data filled in by `localtime_r`, and `strftime`
    // writes at most `buf.len()` bytes.
    unsafe {
        let time = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&time, &mut tm).is_null() {
            return String::new();
        }
        let mut buf = [0u8; 256];
        let n = libc::strftime(buf.as_mut_ptr().cast(), buf.len(), format.as_ptr(), &tm);
        String::from_utf8_lossy(&buf[..n]).into_owned()
    }
}

/// Sends `AppEvent::Clock` whenever the formatted time changes.
pub fn spawn(format: String, events: Sender<AppEvent>) {
    thread::spawn(move || {
        let mut shown = now(&format);
        loop {
            // Wake just after the next whole second.
            let since_epoch = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
            let wait = Duration::from_secs(1) - Duration::from_nanos(since_epoch.subsec_nanos().into());
            thread::sleep(wait + Duration::from_millis(5));
            let time = now(&format);
            if time != shown {
                shown = time;
                if events.send(AppEvent::Clock).is_err() {
                    return;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_local_time() {
        let time = now("%H:%M");
        assert_eq!(time.len(), 5, "{time}");
        assert_eq!(&time[2..3], ":");
        assert_eq!(now("plain"), "plain");
    }
}
