use crate::{FolderPicker, HostChoice};
use futures_util::future::BoxFuture;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

pub struct NativePicker {
    app: AppHandle,
    open: Arc<AtomicBool>,
}

impl NativePicker {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            open: Arc::default(),
        }
    }
}

impl FolderPicker for NativePicker {
    fn pick(&self) -> BoxFuture<'_, HostChoice> {
        Box::pin(async move {
            if self.open.swap(true, Ordering::AcqRel) {
                tracing::debug!(
                    event = "desktop_folder_dialog_busy",
                    "Folder dialog already open"
                );
                return HostChoice::Busy;
            }
            let (sender, receiver) = tokio::sync::oneshot::channel();
            let open = self.open.clone();
            let mut dialog = self.app.dialog().file().set_title("Choose a music folder");
            if let Some(window) = self.app.get_webview_window("main") {
                dialog = dialog.set_parent(&window);
            }
            dialog.pick_folder(move |path| {
                open.store(false, Ordering::Release);
                let _ = sender.send(path);
            });
            let picked = receiver
                .await
                .ok()
                .flatten()
                .and_then(|path| path.into_path().ok());
            tracing::debug!(
                event = "desktop_folder_dialog_closed",
                chosen = picked.is_some(),
                "Folder dialog closed"
            );
            picked.map_or(HostChoice::Cancelled, HostChoice::Chosen)
        })
    }
}
