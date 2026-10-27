use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub(crate) struct RuntimeTurnCapture {
    pub root: String,
    pub session_id: String,
    pub key: String,
    pub before_error: Option<String>,
    completed: Arc<Mutex<bool>>,
    unavailable: Arc<AtomicBool>,
}

impl RuntimeTurnCapture {
    pub async fn begin(root: &str, session_id: &str) -> Self {
        let mut capture = Self {
            root: root.into(),
            session_id: session_id.into(),
            key: format!("runtime_{}", uuid::Uuid::new_v4()),
            before_error: None,
            completed: Arc::new(Mutex::new(false)),
            unavailable: Arc::new(AtomicBool::new(false)),
        };
        let tuple = capture.clone();
        match tokio::task::spawn_blocking(move || {
            let root = std::fs::canonicalize(&tuple.root)
                .ok()
                .and_then(|root| root.into_os_string().into_string().ok())
                .unwrap_or(tuple.root);
            let result =
                crate::commands::journal::capture_turn_before(&root, &tuple.session_id, &tuple.key);
            (root, result.err())
        })
        .await
        {
            Ok((root, error)) => {
                capture.root = root;
                capture.before_error = error;
            }
            Err(error) => capture.before_error = Some(error.to_string()),
        }
        capture
            .unavailable
            .store(capture.before_error.is_some(), Ordering::Relaxed);
        if let Some(error) = &capture.before_error {
            tracing::warn!(%error, session_id, "File review baseline unavailable; continuing the task");
        }
        capture
    }

    pub async fn finish(&self) -> Result<(), String> {
        let mut completed = self.completed.lock().await;
        if *completed {
            return Ok(());
        }
        let tuple = self.clone();
        let result = tokio::task::spawn_blocking(move || {
            crate::commands::journal::capture_turn_after(&tuple.root, &tuple.session_id, &tuple.key)
        })
        .await
        .map_err(|error| error.to_string())
        .and_then(|result| result);
        if result.is_ok() || self.unavailable.load(Ordering::Relaxed) {
            *completed = true;
        }
        result
    }

    pub async fn settled(&self) -> bool {
        *self.completed.lock().await
    }

    #[cfg(all(test, unix))]
    pub(crate) async fn hold_after_snapshot(&self) -> tokio::sync::MutexGuard<'_, bool> {
        self.completed.lock().await
    }

    pub async fn close_after_stop(&self) -> Result<(), String> {
        let result = self.finish().await;
        if let Err(error) = &result {
            if !self.settled().await {
                self.mark_unavailable(&format!(
                    "after snapshot failed after writer termination: {error}"
                ))
                .await;
                // An unavailable tuple releases its guard without inventing an after image.
                let _ = self.finish().await;
            }
        }
        result
    }

    pub async fn mark_unavailable(&self, reason: &str) {
        self.unavailable.store(true, Ordering::Relaxed);
        let tuple = self.clone();
        let reason = reason.to_string();
        if let Err(error) = tokio::task::spawn_blocking(move || {
            crate::commands::journal::mark_turn_capture_unavailable(
                &tuple.root,
                &tuple.session_id,
                &tuple.key,
                &reason,
            )
        })
        .await
        .map_err(|error| error.to_string())
        .and_then(|result| result)
        {
            tracing::warn!(%error, "Could not mark a pending file capture unavailable");
        }
    }

    pub async fn abandon(&self) {
        let tuple = self.clone();
        if let Err(error) = tokio::task::spawn_blocking(move || {
            crate::commands::journal::abandon_turn_capture(
                &tuple.root,
                &tuple.session_id,
                &tuple.key,
            )
        })
        .await
        .map_err(|error| error.to_string())
        .and_then(|result| result)
        {
            tracing::warn!(%error, "Could not abandon file capture after confirmed process shutdown");
        }
        *self.completed.lock().await = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_journal::FileJournal;
    use std::fs;
    use std::path::PathBuf;

    fn fixture() -> (PathBuf, FileJournal) {
        let root = std::env::temp_dir().join(format!("runtime-capture-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let journal = FileJournal::open(&root, root.join(".xiaoheng/journal.db")).unwrap();
        (root, journal)
    }

    #[tokio::test]
    async fn runtime_capture_records_before_after_with_one_stable_key() {
        let (root, journal) = fixture();
        fs::write(root.join("report.txt"), "before").unwrap();
        let capture = RuntimeTurnCapture::begin(root.to_str().unwrap(), "s").await;
        assert!(capture.before_error.is_none());
        let review = journal.review("s", Some(&capture.key)).unwrap();
        assert_eq!(review.status, "capturing");
        fs::write(root.join("report.txt"), "after").unwrap();
        capture.finish().await.unwrap();
        assert!(capture.settled().await);
        let review = journal.review("s", Some(&capture.key)).unwrap();
        assert_eq!(review.status, "complete");
        assert_eq!(review.changes.len(), 1);
        assert_eq!(review.changes[0].path, "report.txt");
        fs::write(root.join("report.txt"), "later external edit").unwrap();
        capture.finish().await.unwrap();
        assert_eq!(
            journal
                .diff("s", &capture.key, "report.txt")
                .unwrap()
                .after_text
                .as_deref(),
            Some("after")
        );
        drop(journal);
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn runtime_capture_warns_without_blocking_overlapping_workspace_writers() {
        let (root, journal) = fixture();
        let capture = RuntimeTurnCapture::begin(root.to_str().unwrap(), "a").await;
        let second = RuntimeTurnCapture::begin(root.to_str().unwrap(), "b").await;
        let warning = second.before_error.clone();
        let _ = capture.finish().await;
        let _ = second.finish().await;
        drop(journal);
        fs::remove_dir_all(root).unwrap();
        assert!(
            warning.is_some(),
            "unreviewable overlapping writers must surface a warning, not block chat"
        );
    }

    #[tokio::test]
    async fn runtime_capture_untrusted_stop_keeps_guard_until_actual_termination() {
        let (root, journal) = fixture();
        let capture = RuntimeTurnCapture::begin(root.to_str().unwrap(), "s").await;
        capture.mark_unavailable("untrusted local stop").await;
        assert_eq!(
            journal.review("s", Some(&capture.key)).unwrap().status,
            "unavailable"
        );
        assert!(!capture.settled().await);
        assert!(capture.finish().await.is_err());
        assert!(capture.settled().await);
        let next = RuntimeTurnCapture::begin(root.to_str().unwrap(), "next").await;
        assert!(
            next.before_error.is_none(),
            "real termination must clear the guard"
        );
        next.finish().await.unwrap();
        drop(journal);
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn runtime_capture_after_failure_downgrades_and_releases_the_guard() {
        let (root, journal) = fixture();
        let capture = RuntimeTurnCapture::begin(root.to_str().unwrap(), "s").await;
        for index in 0..=crate::file_journal::MAX_FILES {
            fs::write(root.join(format!("file-{index}.txt")), "").unwrap();
        }
        assert!(capture.close_after_stop().await.is_err());
        assert!(capture.settled().await);
        assert_eq!(
            journal.review("s", Some(&capture.key)).unwrap().status,
            "unavailable"
        );
        for index in 0..=crate::file_journal::MAX_FILES {
            fs::remove_file(root.join(format!("file-{index}.txt"))).unwrap();
        }
        let next = RuntimeTurnCapture::begin(root.to_str().unwrap(), "next").await;
        assert!(next.before_error.is_none());
        next.finish().await.unwrap();
        drop(journal);
        fs::remove_dir_all(root).unwrap();
    }
}
