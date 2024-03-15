<!---
# This file is part of the IVMS Online.
#
# @copyright 2024 © by Rafał Wrzeszcz - Wrzasq.pl.
-->

For general information how to provision account consult
[dedicated page](https://github.com/rafalwrzeszcz-wrzasqpl/infrastructure/blob/master/documentation/shared/account.md).
However this is root account - on one hand has no higher-level resource to which it needs to be bound, but on the other
side there are resources that need to be created to manage all other accounts.

# Steps

## PROD

```bash
aws cloudformation deploy \
    --stack-name online-ivms-v1-reports-aggregator \
    --template-file infrastructure/cloudformation/root.yaml \
    --capabilities CAPABILITY_NAMED_IAM CAPABILITY_AUTO_EXPAND \
    --parameter-overrides \
        EnvironmentName=prod
```

## STAGING

```bash
aws cloudformation deploy \
    --stack-name online-ivms-v1-reports-aggregator \
    --template-file infrastructure/cloudformation/root.yaml \
    --capabilities CAPABILITY_NAMED_IAM CAPABILITY_AUTO_EXPAND \
    --parameter-overrides \
        EnvironmentName=staging \
        HasIntegrationTestStage=true \
        HasNextStage=true
```

## DEV

```bash
aws cloudformation deploy \
    --stack-name online-ivms-v1-reports-aggregator \
    --template-file infrastructure/cloudformation/root.yaml \
    --capabilities CAPABILITY_NAMED_IAM CAPABILITY_AUTO_EXPAND \
    --parameter-overrides \
        EnvironmentName=dev \
        HasNextStage=true
```
