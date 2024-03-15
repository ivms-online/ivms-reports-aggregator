/*
 * This file is part of the IVMS Online.
 *
 * @copyright 2024 © by Rafał Wrzeszcz - Wrzasq.pl.
 */

#![feature(fn_traits)]
#![feature(future_join)]
#![feature(unboxed_closures)]

mod api;
mod loader;
mod model;
mod report_dao;
mod runtime_error;

use crate::api::{FetchRequest, ReportResponse};
use crate::loader::load_reports as loader;
use crate::model::{hash_key_of, VesselReportPageToken};
use crate::runtime_error::RuntimeError;
use aws_config::load_defaults;
use aws_lambda_events::s3::S3Event;
use aws_lambda_events::sns::SnsEvent;
use aws_sdk_dynamodb::Client as DynamoDbClient;
use aws_sdk_s3::Client as S3Client;
use aws_smithy_runtime_api::client::behavior_version::BehaviorVersion;
use lambda_runtime::{Error, LambdaEvent};
use serde_json::from_str;
use std::env::var;
use std::future::Future;
use std::rc::Rc;
use tokio::main as tokio_main;
use urlencoding::decode;
use wrzasqpl_commons_aws::{run_lambda, DynamoDbDao, LambdaError};

fn fetch_reports(
    dao: Rc<DynamoDbDao>,
) -> impl Fn<(LambdaEvent<FetchRequest>,), Output = impl Future<Output = Result<ReportResponse, RuntimeError>>> {
    move |event: LambdaEvent<FetchRequest>| {
        let dao = dao.clone();
        let hash_key = hash_key_of(&event.payload.customer_id, &event.payload.vessel_id);

        async move {
            dao.query_index(
                "vesselReports".into(),
                "customerAndVesselId".into(),
                hash_key.clone(),
                event.payload.page_token.map(|field_name| VesselReportPageToken {
                    customer_and_vessel_id: hash_key.clone(),
                    field_name,
                }),
            )
            .await
            .map(ReportResponse::from)
            .map_err(RuntimeError::from)
        }
    }
}

fn load_reports(
    s3: Rc<S3Client>,
    dynamo_db: Rc<DynamoDbClient>,
    table: Rc<String>,
) -> impl Fn<(LambdaEvent<SnsEvent>,), Output = impl Future<Output = Result<(), RuntimeError>>> {
    move |event: LambdaEvent<SnsEvent>| {
        let s3 = s3.clone();
        let dynamo_db = dynamo_db.clone();
        let table = table.clone();

        async move {
            for sns_record in event.payload.records {
                // flat_map is not an option because of `?` within closure
                for s3_record in from_str::<S3Event>(sns_record.sns.message.as_str())?.records {
                    loader(
                        s3.as_ref(),
                        dynamo_db.as_ref(),
                        table.as_str().to_string(),
                        s3_record.s3.bucket.name.ok_or(RuntimeError::MalformedS3Event)?,
                        decode(s3_record.s3.object.key.ok_or(RuntimeError::MalformedS3Event)?.as_str())
                            .map_err(|_| RuntimeError::MalformedS3Event)?
                            .into_owned(),
                    )
                    .await?;
                }
            }

            Ok(())
        }
    }
}

#[tokio_main]
async fn main() -> Result<(), Error> {
    let config = &load_defaults(BehaviorVersion::v2023_11_09()).await;
    let client = DynamoDbClient::new(config);
    let table = var("REPORTS_TABLE")?;

    run_lambda!(
        "reports:fetch": fetch_reports(Rc::new(DynamoDbDao::new(client, table))),
        "reports:load": load_reports(
            Rc::new(S3Client::new(config)),
            Rc::new(client),
            Rc::new(table),
        ),
    )
}
