/*
 * This file is part of the IVMS Online.
 *
 * @copyright 2024 © by Rafał Wrzeszcz - Wrzasq.pl.
 */

use crate::model::{Report, VesselReportPageToken};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;
use wrzasqpl_commons_aws::DynamoDbResultsPage;

// api contract

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchRequest {
    pub customer_id: Uuid,
    pub vessel_id: Uuid,
    pub report_name: String,
    pub page_token: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportResponse {
    pub fields: HashMap<String, String>, // TODO: switch value type to numeric type?
    pub page_token: Option<String>,
}

impl From<DynamoDbResultsPage<Report, VesselReportPageToken>> for ReportResponse {
    fn from(value: DynamoDbResultsPage<Report, VesselReportPageToken>) -> Self {
        Self {
            fields: value
                .items
                .into_iter()
                .map(|field| (field.field_name, field.value))
                .collect(),
            page_token: value.last_evaluated_key.map(|key| key.field_name),
        }
    }
}
