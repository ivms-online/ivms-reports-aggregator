/*
 * This file is part of the IVMS Online.
 *
 * @copyright 2024 © by Rafał Wrzeszcz - Wrzasq.pl.
 */

#![feature(future_join)]

use aws_config::load_defaults;
use aws_sdk_dynamodb::types::{AttributeValue, AttributeValue::S};
use aws_sdk_dynamodb::Client as DynamoDbClient;
use aws_sdk_lambda::operation::invoke::{InvokeError, InvokeOutput};
use aws_sdk_lambda::Client as LambdaClient;
use aws_sdk_s3::Client as S3Client;
use aws_smithy_runtime_api::client::behavior_version::BehaviorVersion;
use aws_smithy_runtime_api::client::orchestrator::HttpResponse;
use aws_smithy_types::Blob;
use cucumber::{given, then, when, World};
use serde_json::{from_slice, json, to_vec};
use std::collections::HashMap;
use std::env::{var, VarError};
use std::future::join;
use tokio::main as tokio_main;

macro_rules! serialize_blob {
    ($($data:tt)+) => {
        Blob::new(
            to_vec(&json!($($data)+)).unwrap()
        )
    };
}

#[derive(World, Debug)]
#[world(init = Self::new)]
struct TestWorld {
    // initialization scope
    upload_bucket: String,
    reports_table: String,
    fetcher_lambda: String,
    s3: S3Client,
    dynamodb: DynamoDbClient,
    lambda: LambdaClient,
    // test run scope
    cleanup_customer_id: Option<String>,
    cleanup_iam_user_name: Option<String>,
    invoke_response: Option<Result<InvokeOutput, SdkError<InvokeError, HttpResponse>>>,
    customer_id: Option<String>,
    customer: Option<HashMap<String, AttributeValue>>,
}

impl TestWorld {
    async fn new() -> Result<Self, VarError> {
        let config = &load_defaults(BehaviorVersion::v2023_11_09()).await;

        Ok(Self {
            upload_bucket: var("UPLOAD_BUCKET")?,
            reports_table: var("REPORTS_TABLE")?,
            fetcher_lambda: var("FETCHER_LAMBDA")?,
            s3: S3Client::new(config),
            dynamodb: DynamoDbClient::new(config),
            lambda: LambdaClient::new(config),
            cleanup_customer_id: None,
            cleanup_iam_user_name: None,
            invoke_response: None,
            customer_id: None,
            customer: None,
        })
    }
}

async fn delete_customer(world: &TestWorld, customer_id: &Option<String>) {
    if let Some(customer_id) = customer_id {
        world
            .dynamodb
            .delete_item()
            .table_name(world.customers_table.as_str())
            .key("customerId", S(customer_id.clone()))
            .send()
            .await
            .unwrap();
    }
}

async fn delete_iam_user_keys(world: &TestWorld, iam_user_name: &String) {
    let keys = world
        .iam
        .list_access_keys()
        .user_name(iam_user_name.clone())
        .send()
        .await
        .ok()
        .map(|keys| keys.access_key_metadata)
        .unwrap();

    world
        .iam
        .delete_access_key()
        .user_name(iam_user_name.clone())
        .access_key_id(keys[0].access_key_id().unwrap())
        .send()
        .await
        .unwrap();
}

async fn delete_iam_user(world: &TestWorld, iam_user_name: &Option<String>) {
    if let Some(iam_user_name) = iam_user_name {
        let wait = join!(
            world
                .iam
                .remove_user_from_group()
                .user_name(iam_user_name.clone())
                .group_name(world.monitoring_group.clone())
                .send(),
            delete_iam_user_keys(world, iam_user_name),
        )
        .await;
        wait.0.unwrap();

        world
            .iam
            .delete_user()
            .user_name(iam_user_name.clone())
            .send()
            .await
            .unwrap();
    }
}

#[tokio_main]
async fn main() {
    TestWorld::cucumber()
        .after(|_feature, _rule, _scenario, _finished, world| {
            Box::pin(async move {
                if let Some(&mut ref cleanup) = world {
                    join!(
                        delete_customer(&cleanup, &cleanup.cleanup_customer_id),
                        delete_customer(&cleanup, &cleanup.customer_id),
                        delete_iam_user(&cleanup, &cleanup.cleanup_iam_user_name),
                        delete_iam_user(
                            &cleanup,
                            &cleanup.iam_user.as_ref().map(|user| user.user_name()).map(String::from)
                        ),
                    )
                    .await;
                }
            })
        })
        .run_and_exit("tests/features")
        .await;
}

// Given …

#[given(expr = "There is a customer {string} with name {string} and IAM user {string}")]
async fn there_is_a_customer(world: &mut TestWorld, customer_id: String, name: String, iam_user_name: String) {
    world.cleanup_customer_id = Some(customer_id.clone());
    world.cleanup_iam_user_name = Some(iam_user_name.clone());

    world
        .iam
        .create_user()
        .user_name(iam_user_name.clone())
        .tags(Tag::builder().key("ivms:role").value("remote").build().unwrap())
        .send()
        .await
        .unwrap();

    let wait = join!(
        world
            .iam
            .add_user_to_group()
            .user_name(iam_user_name.clone())
            .group_name(world.monitoring_group.clone())
            .send(),
        world.iam.create_access_key().user_name(iam_user_name.clone()).send(),
    )
    .await;
    wait.0.unwrap();
    let key = wait.1.unwrap();

    world
        .dynamodb
        .put_item()
        .table_name(world.customers_table.as_str())
        .item("customerId", S(customer_id))
        .item("name", S(name))
        .item("iamUserName", S(iam_user_name.clone()))
        .item(
            "iamAccessKeyId",
            S(key.access_key.map(|access_key| access_key.access_key_id).unwrap()),
        )
        .item("iamSecretAccessKey", S("TEST".into()))
        .send()
        .await
        .unwrap();
}

#[given(expr = "There is no customer {string}")]
async fn there_is_no_customer(world: &mut TestWorld, customer_id: String) {
    delete_customer(world, &Some(customer_id)).await;
}

// When …

#[when(expr = "I delete customer {string}")]
async fn i_delete_customer(world: &mut TestWorld, customer_id: String) {
    world.invoke_response = Some(
        world
            .lambda
            .invoke()
            .function_name(world.deleter_lambda.to_string())
            .payload(serialize_blob!({
                "customerId": customer_id,
            }))
            .send()
            .await,
    );

    world.cleanup_iam_user_name = None;
}

#[when(expr = "I create customer with name {string}")]
async fn i_create_customer(world: &mut TestWorld, name: String) {
    world.invoke_response = Some(
        world
            .lambda
            .invoke()
            .function_name(world.creator_lambda.to_string())
            .payload(serialize_blob!({
                "name": name,
            }))
            .send()
            .await,
    );
}

#[when(expr = "I fetch customer {string}")]
async fn i_fetch_customer(world: &mut TestWorld, customer_id: String) {
    world.invoke_response = Some(
        world
            .lambda
            .invoke()
            .function_name(world.fetcher_lambda.to_string())
            .payload(serialize_blob!({
                "customerId": customer_id,
            }))
            .send()
            .await,
    );
}

// Then …

#[then(expr = "Customer {string} does not exist")]
async fn customer_does_not_exist(world: &mut TestWorld, customer_id: String) {
    assert!(world
        .dynamodb
        .get_item()
        .table_name(world.customers_table.as_str())
        .key("customerId", S(customer_id.to_string()))
        .send()
        .await
        .unwrap()
        .item
        .is_none())
}

#[then(expr = "IAM user {string} does not exist")]
async fn iam_user_does_not_exist(world: &mut TestWorld, iam_user_name: String) {
    assert!(world.iam.get_user().user_name(iam_user_name).send().await.is_err())
}

#[then(expr = "I get {string} API error response")]
async fn i_get_api_error(world: &mut TestWorld, message: String) {
    let response: HashMap<String, String> = from_slice(
        world
            .invoke_response
            .as_ref()
            .and_then(|response| response.as_ref().ok())
            .and_then(|response| response.payload())
            .unwrap()
            .as_ref(),
    )
    .unwrap();

    assert_eq!(message, response["errorMessage"]);
}

#[then(expr = "I can read customer name as {string}")]
async fn i_can_read_customer_name(world: &mut TestWorld, name: String) {
    let response: HashMap<String, String> = from_slice(
        world
            .invoke_response
            .as_ref()
            .and_then(|response| response.as_ref().ok())
            .and_then(|response| response.payload())
            .unwrap()
            .as_ref(),
    )
    .unwrap();

    assert_eq!(name, response["name"]);
}

#[then("I can read customer ID")]
async fn i_can_read_customer_id(world: &mut TestWorld) {
    let response: String = from_slice(
        world
            .invoke_response
            .as_ref()
            .and_then(|response| response.as_ref().ok())
            .and_then(|response| response.payload())
            .unwrap()
            .as_ref(),
    )
    .unwrap();

    world.customer_id = Some(response);
    assert!(world.customer_id.is_some());
}

#[then(expr = "Customer with that ID exists with name {string}")]
async fn customer_with_that_id_exists(world: &mut TestWorld, name: String) {
    world.customer = world
        .dynamodb
        .get_item()
        .table_name(world.customers_table.as_str())
        .key("customerId", S(world.customer_id.clone().unwrap()))
        .send()
        .await
        .unwrap()
        .item;
    assert!(world.customer.is_some());
    assert_eq!(
        name,
        *world
            .customer
            .as_ref()
            .and_then(|item| item["name"].as_s().ok())
            .unwrap()
    );
}

#[then("IAM user for that customer exists")]
async fn iam_user_for_that_customer_exists(world: &mut TestWorld) {
    world.iam_user = world
        .iam
        .get_user()
        .user_name(
            world
                .customer
                .as_ref()
                .and_then(|item| item["iamUserName"].as_s().ok())
                .unwrap(),
        )
        .send()
        .await
        .unwrap()
        .user;
    assert!(world.iam_user.is_some())
}

#[then("That IAM user is in monitoring group")]
async fn that_iam_user_is_in_group(world: &mut TestWorld) {
    let groups = world
        .iam
        .list_groups_for_user()
        .user_name(world.iam_user.as_ref().map(|user| user.user_name()).unwrap())
        .send()
        .await
        .unwrap()
        .groups;

    assert!(groups
        .iter()
        .find(|&group| group.group_name() == world.monitoring_group)
        .is_some());
}

#[then("That IAM user has access key")]
async fn that_iam_user_has_access_key(world: &mut TestWorld) {
    assert!(!world
        .iam
        .list_access_keys()
        .user_name(world.iam_user.as_ref().map(|user| user.user_name()).unwrap())
        .send()
        .await
        .unwrap()
        .access_key_metadata
        .is_empty());
}
