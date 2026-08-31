pub(crate) async fn run_blocking<T, F>(operation: &'static str, work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| format!("{operation} task failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::run_blocking;
    use std::sync::mpsc;
    use std::thread;

    #[test]
    fn blocking_work_runs_on_a_worker_thread() {
        let caller = thread::current().id();
        let (sender, receiver) = mpsc::channel();

        let result = tauri::async_runtime::block_on(run_blocking("test operation", move || {
            sender.send(thread::current().id()).unwrap();
        }));

        assert!(result.is_ok());
        assert_ne!(receiver.recv().unwrap(), caller);
    }

    #[test]
    fn blocking_worker_panic_returns_named_error() {
        let result = tauri::async_runtime::block_on(run_blocking("test operation", || {
            panic!("worker panic");
        }));

        let error = result.unwrap_err();
        assert!(error.starts_with("test operation task failed: "));
    }
}
