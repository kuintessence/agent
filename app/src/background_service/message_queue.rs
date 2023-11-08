use std::borrow::Cow;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use alice_infrastructure::config::MessageQueueConfig;
use anyhow::Context;
use futures::StreamExt;
use rdkafka::{
    admin::{AdminClient, AdminOptions, NewTopic, TopicReplication},
    client::DefaultClientContext,
    config::RDKafkaLogLevel,
    consumer::{Consumer, StreamConsumer},
    error::KafkaError,
    message::BorrowedMessage,
    ClientConfig, Message,
};

use crate::dto;
use crate::dto::TaskCommand;
use crate::infrastructure::ioc::Container;
use crate::infrastructure::service::SelectTaskService;

pub struct KafkaMessageQueue {
    stream_consumer: StreamConsumer,
    service: Arc<Container>,
}

impl KafkaMessageQueue {
    pub async fn run(&self) {
        let mut stream = self.stream_consumer.stream();
        loop {
            match stream.next().await {
                Some(Ok(borrowed_message)) => {
                    if let Err(e) = self.routine(borrowed_message).await {
                        tracing::error!("{e}");
                        continue;
                    }
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

impl KafkaMessageQueue {
    async fn routine(&self, message: BorrowedMessage<'_>) -> serde_json::Result<()> {
        let message = message.payload_view::<str>().and_then(Result::ok).unwrap_or("{}");
        tracing::debug!(incoming_message = %message);
        let task = serde_json::from_str::<dto::Task>(message)?;

        if let dto::TaskCommand::Start = task.command {
            let dto::TaskStart { node_id, body } = serde_json::from_str::<dto::TaskStart>(message)?;
            let service = self.service.clone();
            tokio::spawn(async move { service.start(task.id, node_id, body).await });
        } else {
            let r#type = serde_json::from_str::<dto::TaskType>(message)?;
            let service = self.service.clone();
            match task.command {
                TaskCommand::Resume => {
                    tokio::spawn(async move { service.resume(r#type, task.id).await });
                }
                TaskCommand::Pause => {
                    tokio::spawn(async move { service.pause(r#type, task.id).await });
                }
                TaskCommand::Cancel => {
                    tokio::spawn(async move { service.cancel(r#type, task.id).await });
                }
                _ => unreachable!(),
            }
        }

        Ok(())
    }
}

impl KafkaMessageQueue {
    pub async fn new(
        service: Arc<Container>,
        mq: &MessageQueueConfig,
        extra_topics: impl IntoIterator<Item = String>,
    ) -> anyhow::Result<Self> {
        let mut topics: HashSet<_> = mq.topics.clone().into_iter().collect();
        topics.extend(extra_topics);

        let kafka_producer_config = client_config(&mq.producer);
        let kafka_consumer_config = client_config(&mq.consumer);
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
