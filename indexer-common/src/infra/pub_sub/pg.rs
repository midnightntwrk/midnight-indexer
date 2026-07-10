// This file is part of midnight-indexer.
// Copyright (C) Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

pub mod publisher;
pub mod subscriber;

use crate::domain::Topic;

/// Map a pub-sub [Topic] to a Postgres `LISTEN`/`NOTIFY` channel name.
///
/// Channel names are identifiers (≤63 bytes) and are case-folded unless double-quoted, so normalise
/// to a stable lowercased, prefixed form. Topic names are short ASCII, well within the limit.
pub(crate) fn channel(topic: Topic) -> String {
    format!("pub_sub_{}", topic.0.to_lowercase())
}

#[cfg(test)]
mod tests {
    use crate::{
        domain::{Publisher, Subscriber, WalletIndexed},
        error::BoxError,
        infra::{
            pool::postgres::{self, PostgresPool},
            pub_sub::pg::{publisher::PgPublisher, subscriber::PgSubscriber},
        },
    };
    use anyhow::Context;
    use futures::{StreamExt, TryStreamExt};
    use std::time::Duration;
    use testcontainers::{ImageExt, runners::AsyncRunner};
    use testcontainers_modules::postgres::Postgres as PostgresImage;
    use uuid::Uuid;

    /// A notification staged inside a transaction is delivered on commit and NOT on rollback.
    #[tokio::test]
    async fn test_transactional_publish_subscribe() -> Result<(), BoxError> {
        let container = PostgresImage::default()
            .with_db_name("indexer")
            .with_user("indexer")
            .with_password(env!("APP__INFRA__STORAGE__PASSWORD"))
            .with_tag("17.1-alpine")
            .start()
            .await
            .context("start Postgres container")?;
        let port = container
            .get_host_port_ipv4(5432)
            .await
            .context("get Postgres port")?;

        let pool = PostgresPool::new_without_tls(postgres::Config {
            host: "localhost".into(),
            port,
            dbname: "indexer".into(),
            user: "indexer".into(),
            password: env!("APP__INFRA__STORAGE__PASSWORD").into(),
            ssl_root_cert: None,
            max_connections: 5,
            idle_timeout: Duration::from_secs(60),
            max_lifetime: Duration::from_secs(60),
        })
        .await
        .context("create PostgresPool")?;

        let publisher = PgPublisher;
        let subscriber = PgSubscriber::new(pool.clone());
        let mut messages = subscriber.subscribe::<WalletIndexed>().boxed();

        // Rolled-back transaction: no delivery.
        let mut tx = pool.begin().await.context("begin rollback tx")?;
        let dropped = WalletIndexed {
            wallet_id: Uuid::from_u128(1),
        };
        publisher
            .stage(&mut tx, &dropped)
            .await
            .context("stage on rollback tx")?;
        tx.rollback().await.context("rollback tx")?;

        // Committed transaction: delivered.
        let mut tx = pool.begin().await.context("begin commit tx")?;
        let delivered = WalletIndexed {
            wallet_id: Uuid::from_u128(2),
        };
        publisher
            .stage(&mut tx, &delivered)
            .await
            .context("stage on commit tx")?;
        tx.commit().await.context("commit tx")?;

        // The first message seen must be the committed one — the rolled-back notification was
        // discarded by Postgres and never reaches the listener.
        let received = tokio::time::timeout(Duration::from_secs(5), messages.try_next())
            .await
            .context("timed out awaiting notification")?
            .context("subscriber stream errored")?
            .context("subscriber stream ended")?;
        assert_eq!(received, delivered);

        Ok(())
    }
}
