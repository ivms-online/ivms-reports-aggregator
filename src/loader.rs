/*
 * This file is part of the IVMS Online.
 *
 * @copyright 2024 © by Rafał Wrzeszcz - Wrzasq.pl.
 */

use crate::model::Report;
use crate::runtime_error::RuntimeError;
use async_zip::base::read::stream::ZipFileReader;
use async_zip::ZipEntry;
use aws_sdk_dynamodb::types::{PutRequest, WriteRequest};
use aws_sdk_dynamodb::Client as DynamoDbClient;
use aws_sdk_s3::Client as S3Client;
use chrono::{DateTime, Datelike};
use lazy_regex::regex_captures;
use log::{error, info, trace, warn};
use serde::Deserialize;
use serde_dynamo::to_item;
use serde_json::{from_str, Value};
use std::collections::HashMap;
use uuid::Uuid;

static CHUNK_SIZE: usize = 25;

// model structures for IVMSv1

#[derive(Deserialize)]
struct ReportValueEntry {
    sensor_text: String,
    value: Vec<Value>,
}

#[derive(Deserialize)]
struct ReportsSeries {
    columns: Vec<String>,
    values: Vec<Vec<Value>>,
}

#[derive(Deserialize)]
struct ReportsResults {
    series: Vec<ReportsSeries>,
}

#[derive(Deserialize)]
struct ReportsData {
    results: Vec<ReportsResults>,
}

// end of IVMSv1

struct DynamoDbBuffer<'a> {
    client: &'a DynamoDbClient,
    table_name: String,
    customer_id: Uuid,
    vessel_id: Uuid,
    buffer: Vec<WriteRequest>,
}

impl<'a> DynamoDbBuffer<'a> {
    fn new(
        client: &'a DynamoDbClient,
        table_name: String,
        customer_id: &'a str,
        vessel_id: &'a str,
    ) -> Result<Self, RuntimeError> {
        Ok(Self {
            client,
            table_name,
            customer_id: Uuid::parse_str(customer_id)?,
            vessel_id: Uuid::parse_str(vessel_id)?,
            buffer: vec![],
        })
    }

    async fn save_record(&mut self, entity: Report) -> Result<(), RuntimeError> {
        let item = Some(to_item(&entity)?);

        self.write(PutRequest::builder().set_item(item).build()?).await?;

        Ok(())
    }

    async fn save_report(&mut self, report_name: String, data: HashMap<String, &Value>) -> Result<(), RuntimeError> {
        for (key, payload) in data
            .into_iter()
            .filter(|item| item.0.parse::<f64>().is_ok())
            .filter_map(|item| {
                item.1
                    .as_str()
                    .and_then(|value| from_str::<ReportValueEntry>(value).ok())
                    .map(|value| (item.0, value))
            })
        {
            if let Value::String(value) = &payload.value[0] {
                self.save_record(Report {
                    customer_id: self.customer_id,
                    vessel_id: self.vessel_id,
                    report_name: report_name.clone(),
                    field_name: key,
                    value: value.clone(),
                    label: payload.sensor_text.clone(),
                })
                .await?;
            }
        }

        Ok(())
    }

    async fn save_data_series(&mut self, series: &ReportsSeries) -> Result<(), RuntimeError> {
        for row in &series.values {
            let record: HashMap<String, &Value> = series
                .columns
                .iter()
                .zip(row)
                .map(|(key, value)| (key.clone(), value))
                .collect();

            if let (Some(Value::Number(time)), Some(Value::String(event_text))) =
                (record.get("time"), record.get("event_text"))
            {
                match time.as_i64().and_then(|secs| DateTime::from_timestamp(secs, 0)) {
                    None => {
                        warn!("Could not handle record with invalid date: {}", time);
                    }
                    Some(date) => {
                        self.save_report(
                            format!("{}-{}-{}.{}", date.year(), date.month(), date.day(), event_text),
                            record,
                        )
                        .await?;
                    }
                }
            }
        }

        Ok(())
    }

    async fn save_sensors_data(&mut self, data: String) -> Result<(), RuntimeError> {
        let sensors = from_str::<ReportsData>(data.as_str())?;

        for series in sensors.results.iter().flat_map(|result| &result.series) {
            self.save_data_series(series).await?;
        }

        Ok(())
    }

    async fn process_entry(&mut self, entry: ZipEntry, data: String) -> Result<(), RuntimeError> {
        if let Ok(filename) = entry.filename().as_str() {
            info!("Processing ZIP entry {}.", filename);

            match filename {
                "data/reports.json" => self.save_sensors_data(data).await?,
                _ => trace!("Unknown data entry {}.", filename),
            }
        }

        Ok(())
    }

    async fn write(&mut self, record: PutRequest) -> Result<(), RuntimeError> {
        self.buffer.push(WriteRequest::builder().put_request(record).build());

        if self.buffer.len() >= CHUNK_SIZE {
            self.save().await
        } else {
            Ok(())
        }
    }

    async fn flush(&mut self) -> Result<(), RuntimeError> {
        if self.buffer.is_empty() {
            Ok(())
        } else {
            self.save().await
        }
    }

    async fn save(&mut self) -> Result<(), RuntimeError> {
        let response = self
            .client
            .batch_write_item()
            // this passes owned records and also clears buffer
            .request_items(self.table_name.clone(), self.buffer.drain(..).collect())
            .send()
            .await?;
        if let Some(items) = response.unprocessed_items() {
            for record in items.values().flatten() {
                error!(
                    "Rejected record: {:?}",
                    record.put_request.as_ref().map(PutRequest::item)
                );
            }
        }

        Ok(())
    }
}

pub async fn load_reports(
    s3: &S3Client,
    dynamodb: &DynamoDbClient,
    table_name: String,
    bucket_name: String,
    object_key: String,
) -> Result<(), RuntimeError> {
    let stream = s3
        .get_object()
        .bucket(bucket_name)
        .key(object_key.clone())
        .send()
        .await?
        .body
        .into_async_read();

    info!("Processing S3 key {}.", object_key.clone());

    if let Some((_, customer_id, vessel_id)) =
        regex_captures!("^v1/SYNC/([0-9a-f-]{36})/([0-9a-f-]{36})/.*\\.zip$", &object_key)
    {
        let mut buffer = DynamoDbBuffer::new(dynamodb, table_name, customer_id, vessel_id)?;
        let mut zip = ZipFileReader::with_tokio(stream);

        while let Some(mut entry) = zip.next_with_entry().await? {
            let reader = entry.reader_mut();
            let meta = reader.entry().to_owned();

            if !meta.dir()? {
                let mut data = String::with_capacity(meta.uncompressed_size() as usize);
                reader.read_to_string_checked(&mut data).await?;
                buffer.process_entry(meta, data).await?;
            }

            zip = entry.skip().await?;
        }

        buffer.flush().await?;
    }

    Ok(())
}
