#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn success_schedules_six_hours_with_bounded_jitter() {
        assert_eq!(success_due(100, 901), 22_600);
    }
    #[test]
    fn failures_back_off_then_reset() {
        assert_eq!(
            (1..=5).map(|n| failure_due(10, n)).collect::<Vec<_>>(),
            vec![310, 910, 3610, 21610, 310]
        );
    }
    #[test]
    fn cdn_rejects_confusable_and_nonrelease_urls() {
        assert!(allowed_download(
            "https://download.harmonywatch.cn/releases/a.exe"
        ));
        for url in [
            "http://download.harmonywatch.cn/releases/a.exe",
            "https://download.harmonywatch.cn.evil.com/releases/a.exe",
            "https://evil.com/releases/a.exe",
            "https://download.harmonywatch.cn/updates/a.exe",
            "https://download.harmonywatch.cn:444/releases/a.exe",
        ] {
            assert!(!allowed_download(url), "{url}");
        }
    }
    #[test]
    fn quiescence_waits_for_existing_work_and_releases_on_cancel() {
        let permit = admit_work().unwrap();
        let waiting = Quiesce::begin();
        assert!(!waiting.ready());
        assert!(admit_work().is_err());
        drop(permit);
        assert!(waiting.ready());
        drop(waiting);
        assert!(admit_work().is_ok());
    }
    #[test]
    fn committed_handoff_never_reopens_admission() {
        let state = std::sync::Mutex::new(Admission::default());
        {
            let waiting = Quiesce::for_state(&state);
            assert!(state.lock().unwrap().blocked);
            drop(waiting);
        }
        assert!(!state.lock().unwrap().blocked);
        {
            let waiting = Quiesce::for_state(&state);
            waiting.commit();
        }
        assert!(state.lock().unwrap().blocked);
    }
    #[test]
    fn preview_is_explicit() {
        assert!(valid_channel("stable"));
        assert!(valid_channel("preview"));
        assert!(!valid_channel("beta"));
    }
    #[test]
    fn mac_cdn_accepts_only_unambiguous_signed_bundle_archive_locations() {
        assert!(allowed_macos_download(
            "https://download.harmonywatch.cn/releases/macos/Harmony_Work_1.1.2_aarch64.app.tar.gz"
        ));
        assert!(allowed_macos_download(
            "https://download.harmonywatch.cn/releases/macos/Harmony%20Work.app.tar.gz"
        ));
        for url in [
            "http://download.harmonywatch.cn/releases/update.app.tar.gz",
            "https://download.harmonywatch.cn.evil.com/releases/update.app.tar.gz",
            "https://download.harmonywatch.cn:444/releases/update.app.tar.gz",
            "https://user@download.harmonywatch.cn/releases/update.app.tar.gz",
            "https://download.harmonywatch.cn/updates/update.app.tar.gz",
            "https://download.harmonywatch.cn/releases/../update.app.tar.gz",
            "https://download.harmonywatch.cn/releases/%2e%2e/update.app.tar.gz",
            "https://download.harmonywatch.cn/releases/update%2f.app.tar.gz",
            "https://download.harmonywatch.cn/releases/update.app.tar.gz?unsigned=1",
            "https://download.harmonywatch.cn/releases/update.exe",
        ] {
            assert!(!allowed_macos_download(url), "{url}");
        }
    }
}

pub fn success_due(now: u64, jitter: u64) -> u64 {
    now.saturating_add(21_600 + jitter.min(900))
}
pub fn failure_due(now: u64, failures: u32) -> u64 {
    now.saturating_add([300, 900, 3600, 21600][failures.saturating_sub(1) as usize % 4])
}
pub fn valid_channel(channel: &str) -> bool {
    matches!(channel, "stable" | "preview")
}
#[cfg(any(not(target_os = "macos"), test))]
pub fn allowed_download(url: &str) -> bool {
    let Some(path) = url.strip_prefix("https://download.harmonywatch.cn/releases/") else {
        return false;
    };
    !path.is_empty()
        && !path.contains(['\\', '#', '?'])
        && !path
            .split('/')
            .any(|p| p == ".." || p == "." || p.is_empty())
        && path.ends_with(".exe")
}

pub fn allowed_platform_download(url: &str) -> bool {
    #[cfg(target_os = "macos")]
    return allowed_macos_download(url);
    #[cfg(not(target_os = "macos"))]
    allowed_download(url)
}
#[cfg(any(target_os = "macos", test))]
fn allowed_macos_download(url: &str) -> bool {
    let Some(path) = url.strip_prefix("https://download.harmonywatch.cn/releases/") else {
        return false;
    };
    // Encoded spaces in product names are valid, but separators, dot segments,
    // double-encoding and control characters must never change the directory.
    !path.is_empty()
        && path.ends_with(".app.tar.gz")
        && !path.contains(['\\', '#', '?'])
        && path.split('/').all(|part| {
            urlencoding::decode(part).is_ok_and(|decoded| {
                !decoded.is_empty()
                    && decoded != "."
                    && decoded != ".."
                    && !decoded.contains(['/', '\\', ':', '%'])
                    && !decoded.chars().any(char::is_control)
            })
        })
}

#[derive(Default, Debug)]
struct Admission {
    blocked: bool,
    active: usize,
}
static ADMISSION: std::sync::Mutex<Admission> = std::sync::Mutex::new(Admission {
    blocked: false,
    active: 0,
});
#[derive(Debug, Clone)]
pub struct WorkPermit {
    _inner: std::sync::Arc<PermitInner>,
}
#[derive(Debug)]
struct PermitInner;
impl Drop for PermitInner {
    fn drop(&mut self) {
        ADMISSION.lock().unwrap().active -= 1;
    }
}
pub fn admit_work() -> Result<WorkPermit, &'static str> {
    let mut state = ADMISSION.lock().unwrap();
    if state.blocked {
        return Err("正在等待安装更新，请稍后重试");
    }
    state.active += 1;
    Ok(WorkPermit {
        _inner: std::sync::Arc::new(PermitInner),
    })
}
pub struct Quiesce<'a> {
    state: &'a std::sync::Mutex<Admission>,
    committed: bool,
}
impl Quiesce<'static> {
    pub fn begin() -> Self {
        Self::for_state(&ADMISSION)
    }
}
impl<'a> Quiesce<'a> {
    fn for_state(state: &'a std::sync::Mutex<Admission>) -> Self {
        state.lock().unwrap().blocked = true;
        Self {
            state,
            committed: false,
        }
    }
    pub fn ready(&self) -> bool {
        self.state.lock().unwrap().active == 0
    }
    pub fn commit(mut self) {
        self.committed = true;
    }
}
impl Drop for Quiesce<'_> {
    fn drop(&mut self) {
        if !self.committed {
            self.state.lock().unwrap().blocked = false;
        }
    }
}
