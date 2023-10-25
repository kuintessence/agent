use std::borrow::Cow;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use alice_architecture::hosting::IBackgroundService;
use alice_infrastructure::config::MessageQueueConfig;
use anyhow::Context;
use domain::{
    model::entity::{
        file::FileType,
        task::{
            CollectFrom, CollectRule, CollectTo, FacilityKind, FileInfo, Requirements,
            SoftwareDeploymentStatus, StdInKind, TaskStatus, TaskType,
        },
        SubTask, Task,
    },
    service::TaskSchedulerService,
};
use futures::StreamExt;
use rdkafka::admin::{AdminClient, AdminOptions, NewTopic, TopicReplication};
use rdkafka::client::DefaultClientContext;
use rdkafka::{
    config::RDKafkaLogLevel,
    consumer::{Consumer, StreamConsumer},
    error::KafkaError,
    ClientConfig, Message,
};
use tracing::Instrument;
use uuid::Uuid;

use crate::dto;

pub struct KafkaMessageQueue {
    stream_consumer: StreamConsumer,
    service: Arc<dyn TaskSchedulerService>,
}

#[async_trait::async_trait]
impl IBackgroundService for KafkaMessageQueue {
    async fn run(&self) -> () {
        let mut stream = self.stream_consumer.stream();
        loop {
            match stream.next().await {
                Some(Ok(borrowed_message)) => {
                    let message = borrowed_message
                        .payload_view::<str>()
                        .and_then(|msg| msg.ok())
                        .unwrap_or("{}");
                    tracing::debug!("Message: {message}");
                    let task: dto::Task = match serde_json::from_str(message) {
                        Ok(x) => x,
                        Err(e) => {
                            tracing::error!("{e}");
                            continue;
                        }
                    };
                    let service = self.service.clone();
                    tokio::spawn(
                        async {
                            if let Err(e) = routine(task, service).await {
                                tracing::error!("{e}");
                            }
                        }
                        .instrument(tracing::trace_span!("kafka_message_queue")),
                    );
                }
                Some(Err(kafka_error)) => match kafka_error {
                    KafkaError::PartitionEOF(partition) => {
                        tracing::info!("at end of partition {partition:?}");
                    }
                    _ => tracing::error!("errors from kafka, {kafka_error}"),
                },
                None => (),
            }
        }
    }
}

async fn routine(task: dto::Task, service: Arc<dyn TaskSchedulerService>) -> anyhow::Result<()> {
    tracing::debug!("Message: {task:#?}");
    match task.command {
        dto::TaskCommand::Start => {
            let task = Task {
                id: task.id,
                body: task
                    .body
                    .iter()
                    .map(|x| {
                        let mut sub_task = SubTask {
                            id: Uuid::new_v4(),
                            parent_id: task.id,
                            status: TaskStatus::Queuing,
                            ..Default::default()
                        };
                        match x {
                            dto::TaskBody::SoftwareDeployment {
                                facility_kind,
                                command,
                            } => {
                                sub_task.facility_kind = match facility_kind.clone() {
                                    dto::FacilityKind::Spack {
                                        name,
                                        argument_list,
                                    } => FacilityKind::Spack {
                                        name,
                                        argument_list,
                                    },
                                    dto::FacilityKind::Singularity { image, tag } => {
                                        FacilityKind::Singularity { image, tag }
                                    }
                                };
                                sub_task.task_type = TaskType::SoftwareDeployment {
                                    status: match command {
                                        dto::SoftwareDeploymentCommand::Install => {
                                            SoftwareDeploymentStatus::Install
                                        }
                                        dto::SoftwareDeploymentCommand::Uninstall => {
                                            SoftwareDeploymentStatus::Uninstall
                                        }
                                    },
                                };
                            }
                            dto::TaskBody::UsecaseExecution {
                                name,
                                facility_kind,
                                arguments,
                                environments,
                                std_in,
                                files,
                                requirements,
                            } => {
                                sub_task.facility_kind = match facility_kind.clone() {
                                    dto::FacilityKind::Spack {
                                        name,
                                        argument_list,
                                    } => FacilityKind::Spack {
                                        name,
                                        argument_list,
                                    },
                                    dto::FacilityKind::Singularity { image, tag } => {
                                        FacilityKind::Singularity { image, tag }
                                    }
                                };
                                sub_task.requirements =
                                    requirements.clone().map(|x| Requirements {
                                        cpu_cores: x.cpu_cores,
                                        node_count: x.node_count,
                                        max_wall_time: x.max_wall_time,
                                        max_cpu_time: x.max_cpu_time,
                                        stop_time: x.stop_time,
                                    });
                                sub_task.task_type = TaskType::UsecaseExecution {
                                    name: name.clone(),
                                    arguments: arguments.clone(),
                                    environments: environments.clone(),
                                    std_in: match std_in {
                                        dto::StdInKind::Text { text } => {
                                            StdInKind::Text { text: text.clone() }
                                        }
                                        dto::StdInKind::File { path } => {
                                            StdInKind::File { path: path.clone() }
                                        }
                                        dto::StdInKind::None => StdInKind::Unknown,
                                    },
                                    files: files
                                        .iter()
                                        .map(|x| match x.clone() {
                                            dto::FileInfo::Input {
                                                path,
                                                is_package,
                                                form,
                                            } => match form {
                                                dto::InFileForm::Id(id) => FileInfo {
                                                    id: Uuid::new_v4(),
                                                    metadata_id: id,
                                                    path,
                                                    is_package,
                                                    optional: false,
                                                    file_type: FileType::IN,
                                                    is_generated: false,
                                                    ..Default::default()
                                                },
                                                dto::InFileForm::Content(text) => FileInfo {
                                                    id: Uuid::new_v4(),
                                                    metadata_id: Uuid::new_v4(),
                                                    path,
                                                    is_package,
                                                    optional: false,
                                                    file_type: FileType::IN,
                                                    is_generated: true,
                                                    text,
                                                },
                                            },
                                            dto::FileInfo::Output {
                                                id,
                                                path,
                                                is_package,
                                                optional,
                                            } => FileInfo {
                                                id: Uuid::new_v4(),
                                                metadata_id: id,
                                                path,
                                                is_package,
                                                optional,
                                                file_type: FileType::OUT,
                                                ..Default::default()
                                            },
                                        })
                                        .collect::<Vec<FileInfo>>(),
                                }
                            }
                            dto::TaskBody::CollectedOut {
                                from,
                                rule,
                                to,
                                optional,
                            } => {
                                sub_task.task_type = TaskType::CollectedOut {
                                    from: match from {
                                        dto::CollectFrom::FileOut { path } => {
                                            CollectFrom::FileOut { path: path.clone() }
                                        }
                                        dto::CollectFrom::Stdout => CollectFrom::Stdout,
                                        dto::CollectFrom::Stderr => CollectFrom::Stderr,
                                    },
                                    rule: match rule.clone() {
                                        dto::CollectRule::Regex(exp) => CollectRule::Regex { exp },
                                        dto::CollectRule::BottomLines(n) => {
                                            CollectRule::BottomLines { n }
                                        }
                                        dto::CollectRule::TopLines(n) => {
                                            CollectRule::TopLines { n }
                                        }
                                    },
                                    to: match to.clone() {
                                        dto::CollectTo::File { id, path } => {
                                            CollectTo::File { id, path }
                                        }
                                        dto::CollectTo::Text { id } => CollectTo::Text { id },
                                    },
                                    optional: *optional,
                                }
                            }
                        }
                        sub_task
                    })
                    .collect(),
                update_time: chrono::Utc::now(),
                ..Default::default()
            };

            service.enqueue_task(&task).await
        }
        dto::TaskCommand::Pause => service.pause_task(&task.id.to_string()).await,
        dto::TaskCommand::Continue => service.continue_task(&task.id.to_string()).await,
        dto::TaskCommand::Delete => service.delete_task(&task.id.to_string(), false).await,
    }
}

impl KafkaMessageQueue {
    pub async fn new(
        mq: &MessageQueueConfig,
        extra_topics: impl IntoIterator<Item = String>,
        service: Arc<dyn TaskSchedulerService>,
    ) -> anyhow::Result<Self> {
        let mut topics: HashSet<_> = mq.topics().clone().into_iter().collect();
        topics.extend(extra_topics);

        let kafka_producer_config = client_config(mq.producer());
        let kafka_consumer_config = client_config(mq.consumer());
        let stream_consumer: StreamConsumer = kafka_consumer_config.create()?;
        let admin_client: AdminClient<DefaultClientContext> = kafka_producer_config.create()?;

        let new_topics: Vec<_> = topics
            .iter()
            .map(|topic| NewTopic::new(topic, 1, TopicReplication::Fixed(1)))
            .collect();
        let results = admin_client
            .create_topics(&new_topics, &AdminOptions::default())
            .await
            .context("Failed to create topics")?;
        for result in results {
            match result {
                Ok(topic) => tracing::debug!("Created topic: {topic}"),
                Err((topic, e)) => match e {
                    rdkafka::types::RDKafkaErrorCode::TopicAlreadyExists => {
                        tracing::debug!("Topic: {topic} already exists.")
                    }
                    _ => anyhow::bail!("Failed to create topic {topic:?}, caused by {e}"),
                },
            }
        }

        stream_consumer.subscribe(&topics.iter().map(String::as_str).collect::<Vec<&str>>())?;

        Ok(Self {
            stream_consumer,
            service,
        })
    }
}

fn client_config(options: &HashMap<String, String>) -> ClientConfig {
    let mut config = ClientConfig::new();
    for (key, value) in options {
        let key = if key.contains('_') {
            Cow::from(key.replace('_', "."))
        } else {
            Cow::from(key)
        };
        config.set(key, value);
    }
    config.set_log_level(RDKafkaLogLevel::Debug);
    config
}
