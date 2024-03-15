/*
 * This file is part of the IVMS Online.
 *
 * @copyright 2024 © by Rafał Wrzeszcz - Wrzasq.pl.
 */

use async_zip::error::ZipError;
use aws_sdk_dynamodb::operation::batch_write_item::BatchWriteItemError;
use aws_sdk_s3::operation::get_object::GetObjectError;
use aws_smithy_runtime_api::client::orchestrator::HttpResponse;
use aws_smithy_runtime_api::client::result::SdkError;
use aws_smithy_types::error::operation::BuildError;
use serde_dynamo::Error as DynamoDbSerializationError;
use serde_json::Error as SerializationError;
use std::env::VarError;
use std::fmt::{Debug, Display, Formatter, Result};
use std::num::{ParseFloatError, ParseIntError};
use thiserror::Error;
use uuid::Error as UuidError;
use wrzasqpl_commons_aws::DaoError;

#[derive(Error, Debug)]
pub enum RuntimeError {
    ClientConfigLoadingError(#[from] VarError),
    Dao(#[from] DaoError),
    MalformedS3Event,
    SerializationError(#[from] SerializationError),
    DynamoDbSerializationError(#[from] DynamoDbSerializationError),
    ParseIntError(#[from] ParseIntError),
    ParseFloatError(#[from] ParseFloatError),
    ZipError(#[from] ZipError),
    GetObjectError(#[from] SdkError<GetObjectError, HttpResponse>),
    BatchWriteItemOperation(#[from] SdkError<BatchWriteItemError, HttpResponse>),
    BuildError(#[from] BuildError),
    UuidError(#[from] UuidError),
}

impl Display for RuntimeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result {
        write!(formatter, "{self:?}")
    }
}
