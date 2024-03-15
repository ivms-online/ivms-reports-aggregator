/*
 * This file is part of the IVMS Online.
 *
 * @copyright 2024 © by Rafał Wrzeszcz - Wrzasq.pl.
 */

use aws_sdk_dynamodb::operation::put_item::builders::PutItemFluentBuilder;
use aws_sdk_dynamodb::types::AttributeValue::S;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use wrzasqpl_commons_aws::DynamoDbEntity;

#[inline(always)]
pub fn hash_key_of(customer_id: &Uuid, vessel_id: &Uuid) -> String {
    format!("{customer_id}:{vessel_id}")
}

#[inline(always)]
pub fn sort_key_of(report_name: &String, field_name: &String) -> String {
    format!("{report_name}:{field_name}")
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[doc = "Report entry entity."]
pub struct Report {
    #[doc = "Owner ID."]
    pub customer_id: Uuid,
    #[doc = "Vessel ID."]
    pub vessel_id: Uuid,
    #[doc = "Report name."]
    pub report_name: String,
    #[doc = "Report field."]
    pub field_name: String,
    #[doc = "Report field."]
    pub value: String, // TODO: change to some numeric field?
    #[doc = "Description test."]
    pub label: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportKey {
    pub customer_and_vessel_id: String,
    pub report_key: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VesselReportPageToken {
    pub customer_and_vessel_id: String,
    pub field_name: String,
}

impl DynamoDbEntity<'_> for Report {
    type Key = ReportKey;

    fn hash_key_name() -> String {
        "customerAndVesselId".into()
    }

    fn build_key(&self) -> ReportKey {
        ReportKey {
            customer_and_vessel_id: hash_key_of(&self.customer_id, &self.vessel_id),
            report_key: sort_key_of(&self.report_name, &self.field_name),
        }
    }

    fn handle_save(&mut self, request: PutItemFluentBuilder) -> PutItemFluentBuilder {
        request
            .item(
                "customerAndVesselId",
                S(hash_key_of(&self.customer_id, &self.vessel_id)),
            )
            .item("reportKey", S(sort_key_of(&self.report_name, &self.field_name)))
    }
}

#[cfg(test)]
mod tests {
    use crate::model::Report;
    use uuid::{uuid, Uuid};
    use wrzasqpl_commons_aws::DynamoDbEntity;

    const CUSTOMER_ID: Uuid = uuid!("00000000-0000-0000-0000-000000000000");
    const VESSEL_ID: Uuid = uuid!("00000000-0000-0000-0000-000000000001");
    const REPORT_NAME: &str = "2024.week2";
    const FIELD_NAME: &str = "warnings_count";

    #[test]
    fn build_entity_key() {
        let report = Report {
            customer_id: CUSTOMER_ID,
            vessel_id: VESSEL_ID,
            report_name: REPORT_NAME.into(),
            field_name: FIELD_NAME.into(),
            value: "".to_string(),
        };
        let key = report.build_key();

        assert_eq!(format!("{CUSTOMER_ID}:{VESSEL_ID}"), key.customer_and_vessel_id);
        assert_eq!(format!("{REPORT_NAME}:{FIELD_NAME}"), key.report_key);
    }
}
