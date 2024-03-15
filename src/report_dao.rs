/*
 * This file is part of the IVMS Online.
 *
 * @copyright 2024 © by Rafał Wrzeszcz - Wrzasq.pl.
 */

#[cfg(test)]
mod tests {
    use crate::model::{hash_key_of, sort_key_of, Report, ReportKey};
    use aws_config::load_defaults;
    use aws_sdk_dynamodb::config::Builder;
    use aws_sdk_dynamodb::operation::put_item::{PutItemError, PutItemOutput};
    use aws_sdk_dynamodb::types::{
        AttributeDefinition, AttributeValue::S, BillingMode, GlobalSecondaryIndex, KeySchemaElement, KeyType,
        Projection, ProjectionType, ScalarAttributeType,
    };
    use aws_sdk_dynamodb::Client;
    use aws_smithy_runtime_api::client::behavior_version::BehaviorVersion;
    use aws_smithy_runtime_api::client::orchestrator::HttpResponse;
    use aws_smithy_runtime_api::client::result::SdkError;
    use std::env::var;
    use std::future::join;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use test_context::{test_context, AsyncTestContext};
    use tokio::test as tokio_test;
    use uuid::{uuid, Uuid};
    use wrzasqpl_commons_aws::{DaoError, DynamoDbDao, DynamoDbResultsPage};

    struct DynamoDbTestContext {
        client: Box<Client>,
        dao: Box<DynamoDbDao>,
        table_name: String,
    }

    static NUMBER: AtomicUsize = AtomicUsize::new(0);

    // customers
    static ID_0: Uuid = uuid!("00000000-0000-0000-0000-000000000000");
    // vessels
    static ID_1: Uuid = uuid!("00000000-0000-0000-0000-000000000001");
    static ID_2: Uuid = uuid!("00000000-0000-0000-0000-000000000002");
    // reports
    static REPORT_NAME_0: &str = "2024.week2";
    static REPORT_NAME_1: &str = "2023.month6";
    static FIELD_NAME_0: &str = "total_fuel";
    static FIELD_NAME_1: &str = "warnings_count";

    impl AsyncTestContext for DynamoDbTestContext {
        async fn setup() -> DynamoDbTestContext {
            let table_name = format!("Reports{}", NUMBER.fetch_add(1, Ordering::SeqCst));
            let config = load_defaults(BehaviorVersion::v2023_11_09()).await;
            let local_config = Builder::from(&config)
                .endpoint_url(var("DYNAMODB_LOCAL_HOST").unwrap_or("http://localhost:8000".into()))
                .build();
            let client = Client::from_conf(local_config);

            client
                .create_table()
                .table_name(table_name.as_str())
                .attribute_definitions(
                    AttributeDefinition::builder()
                        .attribute_name("customerAndVesselId")
                        .attribute_type(ScalarAttributeType::S)
                        .build()
                        .unwrap(),
                )
                .attribute_definitions(
                    AttributeDefinition::builder()
                        .attribute_name("reportKey")
                        .attribute_type(ScalarAttributeType::S)
                        .build()
                        .unwrap(),
                )
                .attribute_definitions(
                    AttributeDefinition::builder()
                        .attribute_name("reportName")
                        .attribute_type(ScalarAttributeType::S)
                        .build()
                        .unwrap(),
                )
                .key_schema(
                    KeySchemaElement::builder()
                        .attribute_name("customerAndVesselId")
                        .key_type(KeyType::Hash)
                        .build()
                        .unwrap(),
                )
                .key_schema(
                    KeySchemaElement::builder()
                        .attribute_name("reportKey")
                        .key_type(KeyType::Range)
                        .build()
                        .unwrap(),
                )
                .global_secondary_indexes(
                    GlobalSecondaryIndex::builder()
                        .index_name("vesselReports")
                        .key_schema(
                            KeySchemaElement::builder()
                                .attribute_name("customerAndVesselId")
                                .key_type(KeyType::Hash)
                                .build()
                                .unwrap(),
                        )
                        .key_schema(
                            KeySchemaElement::builder()
                                .attribute_name("reportName")
                                .key_type(KeyType::Range)
                                .build()
                                .unwrap(),
                        )
                        .projection(Projection::builder().projection_type(ProjectionType::All).build())
                        .build()
                        .unwrap(),
                )
                .billing_mode(BillingMode::PayPerRequest)
                .send()
                .await
                .unwrap();

            let context = DynamoDbTestContext {
                client: Box::new(client.clone()),
                dao: Box::new(DynamoDbDao::new(client, table_name.clone())),
                table_name: table_name.clone(),
            };

            let (res1, res2, res3) = join!(
                context.create_record(&ID_0, &ID_1, REPORT_NAME_0, FIELD_NAME_0, "100", "Test_Count"),
                context.create_record(&ID_0, &ID_1, REPORT_NAME_0, FIELD_NAME_1, "101", "Test_Count"),
                context.create_record(&ID_0, &ID_2, REPORT_NAME_1, FIELD_NAME_0, "102", "Test_Count"),
            )
            .await;

            res1.unwrap();
            res2.unwrap();
            res3.unwrap();

            context
        }

        async fn teardown(self) {
            self.client
                .delete_table()
                .table_name(self.table_name)
                .send()
                .await
                .unwrap();
        }
    }

    #[test_context(DynamoDbTestContext)]
    #[tokio_test]
    async fn create_report(ctx: &DynamoDbTestContext) -> Result<(), DaoError> {
        ctx.dao
            .save(&mut Report {
                customer_id: ID_0,
                vessel_id: ID_2,
                report_name: REPORT_NAME_1.to_string(),
                field_name: FIELD_NAME_1.to_string(),
                value: "123".into(),
                label: "Test_Count".into(),
            })
            .await?;

        let report = ctx
            .client
            .get_item()
            .table_name(ctx.table_name.as_str())
            .key("customerAndVesselId", S(hash_key_of(&ID_0, &ID_2)))
            .key("reportKey", S(sort_key_of(&REPORT_NAME_1.into(), &FIELD_NAME_1.into())))
            .send()
            .await
            .unwrap();
        assert_eq!("123", report.item.as_ref().unwrap()["value"].as_s().unwrap());
        assert_eq!("Test_Count", report.item.as_ref().unwrap()["label"].as_s().unwrap());

        Ok(())
    }

    #[test_context(DynamoDbTestContext)]
    #[tokio_test]
    async fn get_report(ctx: &DynamoDbTestContext) -> Result<(), DaoError> {
        let report = ctx
            .dao
            .load::<Report>(ReportKey {
                customer_and_vessel_id: hash_key_of(&ID_0, &ID_1),
                report_key: sort_key_of(&REPORT_NAME_0.into(), &FIELD_NAME_0.into()),
            })
            .await?
            .unwrap();
        assert_eq!("100", report.value);

        Ok(())
    }

    #[test_context(DynamoDbTestContext)]
    #[tokio_test]
    async fn get_report_unexisting(ctx: &DynamoDbTestContext) -> Result<(), DaoError> {
        let unexisting = ctx
            .dao
            .load::<Report>(ReportKey {
                customer_and_vessel_id: hash_key_of(&ID_0, &ID_1),
                report_key: sort_key_of(&REPORT_NAME_1.into(), &FIELD_NAME_1.into()),
            })
            .await?;
        assert!(unexisting.is_none());

        Ok(())
    }

    #[test_context(DynamoDbTestContext)]
    #[tokio_test]
    async fn delete_report(ctx: &DynamoDbTestContext) -> Result<(), DaoError> {
        ctx.dao
            .delete::<Report>(ReportKey {
                customer_and_vessel_id: hash_key_of(&ID_0, &ID_1),
                report_key: sort_key_of(&REPORT_NAME_0.into(), &FIELD_NAME_0.into()),
            })
            .await?;

        let report = ctx
            .client
            .get_item()
            .table_name(ctx.table_name.as_str())
            .key("customerAndVesselId", S(hash_key_of(&ID_0, &ID_1)))
            .key(
                "reportKey",
                S(sort_key_of(&REPORT_NAME_0.to_string(), &FIELD_NAME_0.to_string())),
            )
            .send()
            .await
            .unwrap();
        assert!(report.item.is_none());

        Ok(())
    }

    #[test_context(DynamoDbTestContext)]
    #[tokio_test]
    async fn delete_report_unexisting(ctx: &DynamoDbTestContext) -> Result<(), DaoError> {
        ctx.dao
            .delete::<Report>(ReportKey {
                customer_and_vessel_id: hash_key_of(&ID_0, &ID_1),
                report_key: sort_key_of(&REPORT_NAME_1.into(), &FIELD_NAME_1.into()),
            })
            .await?;

        Ok(())
    }

    #[test_context(DynamoDbTestContext)]
    #[tokio_test]
    async fn list_reports(ctx: &DynamoDbTestContext) -> Result<(), DaoError> {
        let results: DynamoDbResultsPage<Report, ReportKey> = ctx.dao.query(hash_key_of(&ID_0, &ID_1), None).await?;

        assert_eq!(2, results.items.len());
        assert_eq!(REPORT_NAME_0, results.items[0].report_name);
        assert_eq!(FIELD_NAME_0, results.items[0].field_name);

        Ok(())
    }

    #[test_context(DynamoDbTestContext)]
    #[tokio_test]
    async fn list_reports_page(ctx: &DynamoDbTestContext) -> Result<(), DaoError> {
        let results: DynamoDbResultsPage<Report, ReportKey> = ctx
            .dao
            .query(
                hash_key_of(&ID_0, &ID_1),
                Some(ReportKey {
                    customer_and_vessel_id: hash_key_of(&ID_0, &ID_1),
                    report_key: sort_key_of(&REPORT_NAME_0.into(), &FIELD_NAME_0.into()),
                }),
            )
            .await?;

        assert_eq!(1, results.items.len());
        assert_eq!(REPORT_NAME_0, results.items[0].report_name);
        assert_eq!(FIELD_NAME_1, results.items[0].field_name);

        Ok(())
    }

    #[test_context(DynamoDbTestContext)]
    #[tokio_test]
    async fn list_reports_unexisting(ctx: &DynamoDbTestContext) -> Result<(), DaoError> {
        let results: DynamoDbResultsPage<Report, ReportKey> = ctx.dao.query(hash_key_of(&ID_1, &ID_2), None).await?;

        assert!(results.items.is_empty());
        assert!(results.last_evaluated_key.is_none());

        Ok(())
    }

    impl DynamoDbTestContext {
        async fn create_record(
            &self,
            customer_id: &Uuid,
            vessel_id: &Uuid,
            report_name: &str,
            field_name: &str,
            value: &str,
            label: &str,
        ) -> Result<PutItemOutput, SdkError<PutItemError, HttpResponse>> {
            self.client
                .put_item()
                .table_name(self.table_name.as_str())
                .item("customerAndVesselId", S(hash_key_of(customer_id, vessel_id)))
                .item(
                    "reportKey",
                    S(sort_key_of(&report_name.to_string(), &field_name.to_string())),
                )
                .item("customerId", S(customer_id.to_string()))
                .item("vesselId", S(vessel_id.to_string()))
                .item("reportName", S(report_name.into()))
                .item("fieldName", S(field_name.into()))
                .item("value", S(value.to_string()))
                .item("label", S(label.to_string()))
                .send()
                .await
        }
    }
}
