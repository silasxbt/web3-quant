use crate::config::AppConfig;
use tokio::process::Command;

#[derive(Clone)]
pub struct MacosNotifier {
    config: AppConfig,
}

impl MacosNotifier {
    pub fn new(config: AppConfig) -> Self {
        Self { config }
    }

    pub async fn notify(&self, title: &str, body: &str) {
        if !self.config.alert.macos.enabled || !cfg!(target_os = "macos") {
            return;
        }
        let script = format!(
            "display notification {} with title {}",
            apple_script_quote(body),
            apple_script_quote(title)
        );
        if let Err(err) = Command::new("osascript")
            .arg("-e")
            .arg(script)
            .output()
            .await
        {
            tracing::warn!(error = ?err, "macOS notification failed");
        }
    }
}

fn apple_script_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}
