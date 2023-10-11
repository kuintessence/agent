use alice_architecture::hosting::IBackgroundService;
use domain::service::RunJobService;
use notify::{Config, Event, PollWatcher, RecursiveMode, Watcher};
use std::time::Duration;
use tracing::instrument::Instrument;

pub struct FileSystemWatchRunner {
    service: std::sync::Arc<dyn RunJobService>,
    base_path: String,
    ssh_proxy: Option<crate::config::SshProxyConfig>,
}

#[async_trait::async_trait]
impl IBackgroundService for FileSystemWatchRunner {
    async fn run(&self) {
        if self.ssh_proxy.is_some() {
            tracing::warn!("SSH proxy is not supported for file system watcher");
        } else {
            let service = self.service.clone();
            let (sender, receiver): (
                flume::Sender<notify::Result<Event>>,
                flume::Receiver<notify::Result<Event>>,
            ) = flume::unbounded();
            let event_handler = FlumeEventHandler(sender);
            let mut watcher = match PollWatcher::new(
                event_handler,
                Config::default().with_poll_interval(Duration::from_secs(2)),
            ) {
                Ok(x) => x,
                Err(e) => {
                    tracing::error!("{}", e);
                    tracing::error!("Unable to start File Watcher.");
                    return;
                }
            };
            if let Err(e) = watcher.watch(
                std::path::Path::new(self.base_path.as_str()),
                RecursiveMode::Recursive,
            ) {
                tracing::error!("{}", e);
                tracing::error!("Unable to start File Watcher.");
                return;
            };

            loop {
                match receiver.recv_async().await {
                    Ok(result) => {
                        let service = service.clone();
                        match result {
                            Ok(event) => {
                                tracing::trace!("{:?}", event);
                                if event.kind.is_create() {
                                    let path = event.paths.get(0).unwrap().clone();
                                    if path.is_file() && path.ends_with(".co.sig") {
                                        tokio::spawn(
                                            async move {
                                                tracing::trace!(
                                                    "Signal File detected from path {}.",
                                                    path.to_string_lossy()
                                                );
                                                let id = match tokio::fs::read_to_string(path).await
                                                {
                                                    Ok(x) => x,
                                                    Err(e) => {
                                                        tracing::error!("{}", e);
                                                        return;
                                                    }
                                                };
                                                match service.refresh_status(id.as_str()).await {
                                                    Ok(_) => (),
                                                    Err(e) => {
                                                        tracing::error!("{}", e);
                                                    }
                                                }
                                            }
                                            .instrument(tracing::trace_span!(
                                                "file_watcher_watched"
                                            )),
                                        );
                                    }
                                }
                            }
                            Err(e) => tracing::error!("Watcher error: {}", e),
                        }
                    }
                    Err(e) => tracing::error!("Watcher receive event error: {}", e),
                }
            }
        }
    }
}

impl FileSystemWatchRunner {
    pub fn new(
        base_path: String,
        service: std::sync::Arc<dyn RunJobService>,
        ssh_proxy: Option<crate::config::SshProxyConfig>,
    ) -> Self {
        Self {
            base_path,
            service,
            ssh_proxy,
        }
    }
}

struct FlumeEventHandler(flume::Sender<notify::Result<Event>>);

impl notify::EventHandler for FlumeEventHandler {
    fn handle_event(&mut self, event: notify::Result<Event>) {
        if let Err(e) = self.0.send(event) {
            tracing::error!("File watcher send event error. {}", e)
        }
    }
}
