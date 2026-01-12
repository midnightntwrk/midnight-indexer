// This file is part of midnight-indexer.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

use super::{AppState, ContextExt, Db, Metrics};
use async_graphql::{
    Context, EmptyMutation, EmptySubscription, Object, Schema, http::GraphiQLSource,
};
use async_graphql_axum::{GraphQL, GraphQLSubscription};
use axum::{
    Router,
    response::{Html, IntoResponse},
    routing::{get, post_service},
};
use indexer_common::domain::NetworkId;
use log::{info, warn};
use regex::Regex;
// No rust_decimal to keep sqlx decoding simple; we parse numerics as strings when needed.

const DEFAULT_PERFORMANCE_LIMIT: i64 = 20;

type EpochPerfRow = (
    i64,            // epoch_no (BIGINT)
    String,         // spo_sk_hex
    i32,            // produced_blocks (INT)
    i32,            // expected_blocks (INT)
    Option<String>, // identity_label
    Option<String>, // stake_snapshot
    Option<String>, // pool_id_hex
    Option<String>, // validator_class
);

pub fn make_app(
    network_id: NetworkId,
    max_complexity: usize,
    max_depth: usize,
    db: Option<Db>,
) -> Router<AppState> {
    let schema = Schema::build(Query::default(), EmptyMutation, EmptySubscription)
        .limit_complexity(max_complexity)
        .limit_depth(max_depth)
        .data(network_id)
        .data(Metrics::default())
        .data(db)
        // Inject optional Db from AppState via Router state in handlers.
        .finish();

    // Runtime confirmation that extended schema is present.
    if schema.sdl().contains("spoCompositeByPoolId") {
        info!("graphQL schema includes spoCompositeByPoolId");
    } else {
        warn!("spoCompositeByPoolId missing from schema – ensure service rebuilt without cache");
    }

    Router::new()
        // Support both /graphql and /graphql/ to avoid 404 (empty body -> GraphiQL JSON parse
        // error).
        .route("/graphql", get(graphiql))
        .route("/graphql/", get(graphiql))
        .route("/graphql", post_service(GraphQL::new(schema.clone())))
        .route("/graphql/", post_service(GraphQL::new(schema.clone())))
        .route_service("/graphql/ws", GraphQLSubscription::new(schema.clone()))
        .route_service("/graphql/ws/", GraphQLSubscription::new(schema))
}

#[derive(Default)]
pub struct Query;

#[Object(rename_fields = "camelCase")]
impl Query {
    async fn service_info(&self, cx: &Context<'_>) -> ServiceInfo {
        let network = cx.get_network_id().to_string();
        ServiceInfo {
            name: "spo-api".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            network,
        }
    }

    /// Cumulative total of currently registered SPOs over an epoch range, using first-seen epochs.
    ///
    /// Semantics:
    /// - Domain is limited to pools present in spo_stake_snapshot ("current" pools), so the final
    ///   value equals spo_count by construction.
    /// - First-seen epoch per pool is computed as the minimum epoch where that pool_id appears in
    ///   any of: spo_history (via spo_identity), committee_membership (via spo_identity),
    ///   spo_epoch_performance (via spo_identity).
    /// - If a current pool has no appearances in those sources, it is assigned first_seen_epoch =
    ///   to_epoch (it will enter at the end of the requested window so totals match spo_count).
    async fn registered_totals_series(
        &self,
        cx: &Context<'_>,
        from_epoch: i64,
        to_epoch: i64,
    ) -> Vec<RegisteredTotals> {
        let start = from_epoch.min(to_epoch);
        let end = to_epoch.max(from_epoch);
        if let Some(Db(pool)) = cx.data_opt::<Option<Db>>().and_then(|o| o.as_ref()) {
            let sql = r#"
                WITH rng AS (
                    SELECT generate_series($1::BIGINT, $2::BIGINT) AS epoch_no
                ),
                cur AS (
                    SELECT s.pool_id
                    FROM spo_stake_snapshot s
                ),
                union_firsts AS (
                    SELECT si.pool_id AS pool_id, MIN(sh.epoch_no)::BIGINT AS first_seen_epoch
                    FROM spo_history sh
                    LEFT JOIN spo_identity si ON si.spo_sk = sh.spo_sk
                    WHERE si.pool_id IS NOT NULL
                    GROUP BY si.pool_id
                    UNION ALL
                    SELECT si.pool_id AS pool_id, MIN(cm.epoch_no)::BIGINT AS first_seen_epoch
                    FROM committee_membership cm
                    LEFT JOIN spo_identity si ON si.sidechain_pubkey = cm.sidechain_pubkey
                    WHERE si.pool_id IS NOT NULL
                    GROUP BY si.pool_id
                    UNION ALL
                    SELECT si.pool_id AS pool_id, MIN(sep.epoch_no)::BIGINT AS first_seen_epoch
                    FROM spo_epoch_performance sep
                    LEFT JOIN spo_identity si ON si.spo_sk = sep.spo_sk
                    WHERE si.pool_id IS NOT NULL
                    GROUP BY si.pool_id
                ),
                firsts0 AS (
                    SELECT pool_id, MIN(first_seen_epoch)::BIGINT AS first_seen_epoch
                    FROM union_firsts
                    GROUP BY pool_id
                ),
                firsts_cur AS (
                    SELECT c.pool_id,
                           COALESCE(f0.first_seen_epoch, $2::BIGINT) AS first_seen_epoch
                    FROM cur c
                    LEFT JOIN firsts0 f0 ON f0.pool_id = c.pool_id
                ),
                agg AS (
                    SELECT r.epoch_no,
                           COUNT(*) FILTER (WHERE fc.first_seen_epoch <= r.epoch_no) AS total_registered,
                           COUNT(*) FILTER (WHERE fc.first_seen_epoch = r.epoch_no) AS newly_registered
                    FROM rng r
                    CROSS JOIN firsts_cur fc
                    GROUP BY r.epoch_no
                )
                SELECT epoch_no, total_registered, newly_registered
                FROM agg
                ORDER BY epoch_no
            "#;
            match sqlx::query_as::<_, (i64, i64, i64)>(sql)
                .bind(start)
                .bind(end)
                .fetch_all(&**pool)
                .await
            {
                Ok(rows) => rows
                    .into_iter()
                    .map(
                        |(epoch_no, total_registered, newly_registered)| RegisteredTotals {
                            epoch_no,
                            total_registered,
                            newly_registered,
                        },
                    )
                    .collect(),
                Err(error) => {
                    warn!("registered_totals_series query failed: {error}");
                    vec![]
                }
            }
        } else {
            vec![]
        }
    }

    // -------------------------------------------------
    // Identity (no metadata) queries
    // -------------------------------------------------
    async fn spo_identities(
        &self,
        cx: &Context<'_>,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> Vec<SpoIdentity> {
        let limit = limit.unwrap_or(50).clamp(1, 500) as i64;
        let offset = offset.unwrap_or(0).max(0) as i64;
        if let Some(Db(pool)) = cx.data_opt::<Option<Db>>().and_then(|o| o.as_ref()) {
            let sql = r#"
                SELECT pool_id AS pool_id_hex,
                       mainchain_pubkey AS mainchain_pubkey_hex,
                       sidechain_pubkey AS sidechain_pubkey_hex,
                       aura_pubkey AS aura_pubkey_hex,
                       'UNKNOWN' AS validator_class
                FROM spo_identity
                WHERE pool_id IS NOT NULL
                ORDER BY mainchain_pubkey
                LIMIT $1 OFFSET $2
            "#;
            match sqlx::query_as::<_, (String, String, String, Option<String>, String)>(sql)
                .bind(limit)
                .bind(offset)
                .fetch_all(&**pool)
                .await
            {
                Ok(rows) => rows
                    .into_iter()
                    .map(
                        |(
                            pool_id_hex,
                            mainchain_pubkey_hex,
                            sidechain_pubkey_hex,
                            aura_pubkey_hex,
                            validator_class,
                        )| SpoIdentity {
                            pool_id_hex,
                            mainchain_pubkey_hex,
                            sidechain_pubkey_hex,
                            aura_pubkey_hex,
                            validator_class,
                        },
                    )
                    .collect(),
                Err(error) => {
                    warn!("spo_identities query failed: {error}");
                    vec![]
                }
            }
        } else {
            vec![]
        }
    }

    async fn spo_identity_by_pool_id(
        &self,
        cx: &Context<'_>,
        pool_id_hex: String,
    ) -> Option<SpoIdentity> {
        let pool_id_hex = normalize_hex(&pool_id_hex)?;
        if let Some(Db(pool)) = cx.data_opt::<Option<Db>>().and_then(|o| o.as_ref()) {
            let sql = r#"
                SELECT pool_id AS pool_id_hex,
                       mainchain_pubkey AS mainchain_pubkey_hex,
                       sidechain_pubkey AS sidechain_pubkey_hex,
                       aura_pubkey AS aura_pubkey_hex,
                       'UNKNOWN' AS validator_class
                FROM spo_identity
                WHERE pool_id = $1
                LIMIT 1
            "#;
            match sqlx::query_as::<_, (String, String, String, Option<String>, String)>(sql)
                .bind(&pool_id_hex)
                .fetch_optional(&**pool)
                .await
            {
                Ok(Some((
                    pool_id_hex,
                    mainchain_pubkey_hex,
                    sidechain_pubkey_hex,
                    aura_pubkey_hex,
                    validator_class,
                ))) => Some(SpoIdentity {
                    pool_id_hex,
                    mainchain_pubkey_hex,
                    sidechain_pubkey_hex,
                    aura_pubkey_hex,
                    validator_class,
                }),
                Ok(None) => None,
                Err(error) => {
                    warn!("spo_identity_by_pool_id query failed: {error}");
                    None
                }
            }
        } else {
            None
        }
    }

    // -------------------------------------------------
    // Metadata queries
    // -------------------------------------------------
    async fn pool_metadata(&self, cx: &Context<'_>, pool_id_hex: String) -> Option<PoolMetadata> {
        let pool_id_hex = normalize_hex(&pool_id_hex)?;
        if let Some(Db(pool)) = cx.data_opt::<Option<Db>>().and_then(|o| o.as_ref()) {
            let sql = r#"
                SELECT pool_id AS pool_id_hex,
                       hex_id AS hex_id,
                       name, ticker, homepage_url, url AS logo_url
                FROM pool_metadata_cache
                WHERE pool_id = $1
                LIMIT 1
            "#;
            match sqlx::query_as::<
                _,
                (
                    String,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                ),
            >(sql)
            .bind(&pool_id_hex)
            .fetch_optional(&**pool)
            .await
            {
                Ok(Some((pool_id_hex, hex_id, name, ticker, homepage_url, logo_url))) => {
                    Some(PoolMetadata {
                        pool_id_hex,
                        hex_id,
                        name,
                        ticker,
                        homepage_url,
                        logo_url,
                    })
                }
                Ok(None) => None,
                Err(error) => {
                    warn!("pool_metadata query failed: {error}");
                    None
                }
            }
        } else {
            None
        }
    }

    async fn pool_metadata_list(
        &self,
        cx: &Context<'_>,
        limit: Option<i32>,
        offset: Option<i32>,
        with_name_only: Option<bool>,
    ) -> Vec<PoolMetadata> {
        let limit = limit.unwrap_or(50).clamp(1, 500) as i64;
        let offset = offset.unwrap_or(0).max(0) as i64;
        let name_only = with_name_only.unwrap_or(false);
        if let Some(Db(pool)) = cx.data_opt::<Option<Db>>().and_then(|o| o.as_ref()) {
            let sql = if name_only {
                r#"
                SELECT pool_id AS pool_id_hex,
                       hex_id AS hex_id,
                       name, ticker, homepage_url, url AS logo_url
                FROM pool_metadata_cache
                WHERE name IS NOT NULL OR ticker IS NOT NULL
                ORDER BY pool_id
                LIMIT $1 OFFSET $2
            "#
            } else {
                r#"
                SELECT pool_id AS pool_id_hex,
                       hex_id AS hex_id,
                       name, ticker, homepage_url, url AS logo_url
                FROM pool_metadata_cache
                ORDER BY pool_id
                LIMIT $1 OFFSET $2
            "#
            };
            match sqlx::query_as::<
                _,
                (
                    String,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                ),
            >(sql)
            .bind(limit)
            .bind(offset)
            .fetch_all(&**pool)
            .await
            {
                Ok(rows) => rows
                    .into_iter()
                    .map(
                        |(pool_id_hex, hex_id, name, ticker, homepage_url, logo_url)| {
                            PoolMetadata {
                                pool_id_hex,
                                hex_id,
                                name,
                                ticker,
                                homepage_url,
                                logo_url,
                            }
                        },
                    )
                    .collect(),
                Err(error) => {
                    warn!("pool_metadata_list query failed: {error}");
                    vec![]
                }
            }
        } else {
            vec![]
        }
    }

    // -------------------------------------------------
    // Composite query
    // -------------------------------------------------
    async fn spo_composite_by_pool_id(
        &self,
        cx: &Context<'_>,
        pool_id_hex: String,
    ) -> Option<SpoComposite> {
        let pool_id_hex = normalize_hex(&pool_id_hex)?;
        let Some(Db(pool)) = cx.data_opt::<Option<Db>>().and_then(|o| o.as_ref()) else {
            return None;
        };

        let identity_sql = r#"
            SELECT pool_id AS pool_id_hex,
                   mainchain_pubkey AS mainchain_pubkey_hex,
                   sidechain_pubkey AS sidechain_pubkey_hex,
                   aura_pubkey AS aura_pubkey_hex,
                   'UNKNOWN' AS validator_class
            FROM spo_identity
            WHERE pool_id = $1
            LIMIT 1
        "#;
        let identity = match sqlx::query_as::<_, (String, String, String, Option<String>, String)>(
            identity_sql,
        )
        .bind(&pool_id_hex)
        .fetch_optional(&**pool)
        .await
        {
            Ok(Some((
                pool_id_hex,
                mainchain_pubkey_hex,
                sidechain_pubkey_hex,
                aura_pubkey_hex,
                validator_class,
            ))) => Some(SpoIdentity {
                pool_id_hex,
                mainchain_pubkey_hex,
                sidechain_pubkey_hex,
                aura_pubkey_hex,
                validator_class,
            }),
            Ok(None) => None,
            Err(error) => {
                warn!("spo_composite_by_pool_id identity query failed: {error}");
                None
            }
        };

        let metadata_sql = r#"
            SELECT pool_id AS pool_id_hex,
                   hex_id AS hex_id,
                   name, ticker, homepage_url, url AS logo_url
            FROM pool_metadata_cache
            WHERE pool_id = $1
            LIMIT 1
        "#;
        let metadata = match sqlx::query_as::<
            _,
            (
                String,
                Option<String>,
                Option<String>,
                Option<String>,
                Option<String>,
                Option<String>,
            ),
        >(metadata_sql)
        .bind(&pool_id_hex)
        .fetch_optional(&**pool)
        .await
        {
            Ok(Some((pool_id_hex, hex_id, name, ticker, homepage_url, logo_url))) => {
                Some(PoolMetadata {
                    pool_id_hex,
                    hex_id,
                    name,
                    ticker,
                    homepage_url,
                    logo_url,
                })
            }
            Ok(None) => None,
            Err(error) => {
                warn!("spo_composite_by_pool_id metadata query failed: {error}");
                None
            }
        };

        let performance = if let Some(identity_ref) = identity.as_ref() {
            // Performance rows are keyed by sidechain_pubkey (sep.spo_sk).
            let sk_hex = &identity_ref.sidechain_pubkey_hex;
            let perf_sql = r#"
                SELECT sep.epoch_no,
                       sep.spo_sk AS spo_sk_hex,
                       sep.produced_blocks,
                       sep.expected_blocks,
                       sep.identity_label,
                       NULL::TEXT AS stake_snapshot,
                       si.pool_id AS pool_id_hex,
                       'UNKNOWN' AS validator_class
                FROM spo_epoch_performance sep
                LEFT JOIN spo_identity si ON si.spo_sk = sep.spo_sk
                WHERE sep.spo_sk = $1
                ORDER BY sep.epoch_no DESC
                LIMIT $2
            "#;
            match sqlx::query_as::<
                _,
                (
                    i64,
                    String,
                    i32,
                    i32,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                ),
            >(perf_sql)
            .bind(sk_hex)
            .bind(DEFAULT_PERFORMANCE_LIMIT)
            .fetch_all(&**pool)
            .await
            {
                Ok(rows) => rows.into_iter().map(EpochPerf::from_tuple).collect(),
                Err(error) => {
                    warn!("spo_composite_by_pool_id performance query failed: {error}");
                    vec![]
                }
            }
        } else {
            vec![]
        };

        Some(SpoComposite {
            identity,
            metadata,
            performance,
        })
    }

    /// List stake pool operator identifiers (placeholder – returns empty if table missing / error).
    async fn stake_pool_operators(&self, cx: &Context<'_>, limit: Option<i32>) -> Vec<String> {
        let limit = limit.unwrap_or(20).clamp(1, 100) as i64;
        // Access optional Db from Router state (AppState)
        if let Some(Db(pool)) = cx.data_opt::<Option<Db>>().and_then(|o| o.as_ref()) {
            let sql = r#"
                SELECT encode(sep.spo_sk,'hex') AS spo_sk_hex
                FROM spo_epoch_performance sep
                GROUP BY sep.spo_sk
                ORDER BY MAX(sep.produced_blocks) DESC
                LIMIT $1
            "#;
            match sqlx::query_scalar::<_, String>(sql)
                .bind(limit)
                .fetch_all(&**pool)
                .await
            {
                Ok(rows) => rows,
                Err(error) => {
                    warn!("stake_pool_operators query failed: {error}");
                    vec![]
                }
            }
        } else {
            vec![]
        }
    }

    /// Latest SPO performance entries ordered by epoch (desc) and produced blocks (desc).
    async fn spo_performance_latest(
        &self,
        cx: &Context<'_>,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> Vec<EpochPerf> {
        let limit = limit
            .unwrap_or(DEFAULT_PERFORMANCE_LIMIT as i32)
            .clamp(1, 500) as i64;
        let offset = offset.unwrap_or(0).max(0) as i64;
        if let Some(Db(pool)) = cx.data_opt::<Option<Db>>().and_then(|o| o.as_ref()) {
            let sql = r#"
                SELECT sep.epoch_no,
                       sep.spo_sk AS spo_sk_hex,
                       sep.produced_blocks,
                       sep.expected_blocks,
                       sep.identity_label,
                       NULL::TEXT AS stake_snapshot,
                       si.pool_id AS pool_id_hex,
                       'UNKNOWN' AS validator_class
                FROM spo_epoch_performance sep
                LEFT JOIN spo_identity si ON si.spo_sk = sep.spo_sk
                ORDER BY sep.epoch_no DESC, sep.produced_blocks DESC
                LIMIT $1 OFFSET $2
            "#;
            match sqlx::query_as::<_, EpochPerfRow>(sql)
                .bind(limit)
                .bind(offset)
                .fetch_all(&**pool)
                .await
            {
                Ok(rows) => rows.into_iter().map(EpochPerf::from_tuple).collect(),
                Err(error) => {
                    warn!("spo_performance_latest query failed: {error}");
                    vec![]
                }
            }
        } else {
            vec![]
        }
    }

    /// Performance history for a single SPO (identified by its side/mainchain key hex
    /// representation).
    async fn spo_performance_by_spo_sk(
        &self,
        cx: &Context<'_>,
        spo_sk_hex: String,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> Vec<EpochPerf> {
        let spo_sk_hex = match normalize_hex(&spo_sk_hex) {
            Some(hex) => hex,
            None => return vec![],
        };
        let limit = limit.unwrap_or(100).clamp(1, 500) as i64;
        let offset = offset.unwrap_or(0).max(0) as i64;
        if let Some(Db(pool)) = cx.data_opt::<Option<Db>>().and_then(|o| o.as_ref()) {
            let sql = r#"
                SELECT sep.epoch_no,
                       sep.spo_sk AS spo_sk_hex,
                       sep.produced_blocks,
                       sep.expected_blocks,
                       sep.identity_label,
                       NULL::TEXT AS stake_snapshot,
                       si.pool_id AS pool_id_hex,
                       'UNKNOWN' AS validator_class
                FROM spo_epoch_performance sep
                LEFT JOIN spo_identity si ON si.spo_sk = sep.spo_sk
                WHERE sep.spo_sk = $1
                ORDER BY sep.epoch_no DESC
                LIMIT $2 OFFSET $3
            "#;
            match sqlx::query_as::<_, EpochPerfRow>(sql)
                .bind(&spo_sk_hex)
                .bind(limit)
                .bind(offset)
                .fetch_all(&**pool)
                .await
            {
                Ok(rows) => rows.into_iter().map(EpochPerf::from_tuple).collect(),
                Err(error) => {
                    warn!("spo_performance_by_spo_sk query failed: {error}");
                    vec![]
                }
            }
        } else {
            vec![]
        }
    }

    /// Epoch performance for a given epoch, tolerant of missing identity records.
    async fn epoch_performance(
        &self,
        cx: &Context<'_>,
        epoch: i64,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> Vec<EpochPerf> {
        let limit = limit.unwrap_or(100).clamp(1, 500) as i64;
        let offset = offset.unwrap_or(0).max(0) as i64;
        if let Some(Db(pool)) = cx.data_opt::<Option<Db>>().and_then(|o| o.as_ref()) {
            let sql = r#"
                SELECT sep.epoch_no,
                       sep.spo_sk AS spo_sk_hex,
                       sep.produced_blocks,
                       sep.expected_blocks,
                       sep.identity_label,
                       NULL::TEXT AS stake_snapshot,
                       si.pool_id AS pool_id_hex,
                       'UNKNOWN' AS validator_class
                FROM spo_epoch_performance sep
                LEFT JOIN spo_identity si ON si.spo_sk = sep.spo_sk
                WHERE sep.epoch_no = $1
                ORDER BY sep.produced_blocks DESC
                LIMIT $2 OFFSET $3
            "#;
            match sqlx::query_as::<_, EpochPerfRow>(sql)
                .bind(epoch)
                .bind(limit)
                .bind(offset)
                .fetch_all(&**pool)
                .await
            {
                Ok(rows) => rows.into_iter().map(EpochPerf::from_tuple).collect(),
                Err(error) => {
                    warn!("epoch_performance query failed: {error}");
                    vec![]
                }
            }
        } else {
            vec![]
        }
    }

    /// List SPOs with optional metadata, paginated.
    async fn spo_list(
        &self,
        cx: &Context<'_>,
        limit: Option<i32>,
        offset: Option<i32>,
        search: Option<String>,
    ) -> Vec<Spo> {
        let limit = limit.unwrap_or(20).clamp(1, 200) as i64;
        let offset = offset.unwrap_or(0).max(0) as i64;
        let search = search.as_ref().and_then(|s| {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        });
        if let Some(Db(pool)) = cx.data_opt::<Option<Db>>().and_then(|o| o.as_ref()) {
            // Use spo_stake_snapshot as the canonical current set to align counts with spo_count.
            let sql = if search.is_some() {
                r#"
          SELECT s.pool_id AS pool_id_hex,
                 'UNKNOWN' AS validator_class,
                 si.sidechain_pubkey AS sidechain_pubkey_hex,
                 si.aura_pubkey AS aura_pubkey_hex,
                 pm.name, pm.ticker, pm.homepage_url, pm.url AS logo_url
            FROM spo_stake_snapshot s
            LEFT JOIN spo_identity si ON si.pool_id = s.pool_id
            LEFT JOIN pool_metadata_cache pm ON pm.pool_id = s.pool_id
            WHERE (
                    pm.name ILIKE $3 OR pm.ticker ILIKE $3 OR pm.homepage_url ILIKE $3 OR s.pool_id ILIKE $4
                 OR si.sidechain_pubkey ILIKE $4 OR si.aura_pubkey ILIKE $4 OR si.mainchain_pubkey ILIKE $4
              )
            ORDER BY COALESCE(si.mainchain_pubkey, s.pool_id)
            LIMIT $1 OFFSET $2
                "#
            } else {
                r#"
          SELECT s.pool_id AS pool_id_hex,
                 'UNKNOWN' AS validator_class,
                 si.sidechain_pubkey AS sidechain_pubkey_hex,
                 si.aura_pubkey AS aura_pubkey_hex,
                 pm.name, pm.ticker, pm.homepage_url, pm.url AS logo_url
            FROM spo_stake_snapshot s
            LEFT JOIN spo_identity si ON si.pool_id = s.pool_id
            LEFT JOIN pool_metadata_cache pm ON pm.pool_id = s.pool_id
            ORDER BY COALESCE(si.mainchain_pubkey, s.pool_id)
            LIMIT $1 OFFSET $2
                "#
            };

            // Build bind params
            let mut q = sqlx::query_as::<
                _,
                (
                    String,
                    String,
                    String,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                ),
            >(sql);
            q = q.bind(limit).bind(offset);
            if let Some(s) = search {
                // For text fields use %term% ; for hex-like identifiers also use %term_no_0x%
                let s_like = format!("%{s}%");
                let s_hex = normalize_hex(&s).unwrap_or_else(|| s.to_ascii_lowercase());
                let s_hex_like = format!("%{s_hex}%");
                q = q.bind(s_like).bind(s_hex_like);
            }

            match q.fetch_all(&**pool).await {
                Ok(rows) => rows
                    .into_iter()
                    .map(
                        |(
                            pool_id_hex,
                            validator_class,
                            sidechain_pubkey_hex,
                            aura_pubkey_hex,
                            name,
                            ticker,
                            homepage_url,
                            logo_url,
                        )| Spo {
                            pool_id_hex,
                            validator_class,
                            sidechain_pubkey_hex,
                            aura_pubkey_hex,
                            name,
                            ticker,
                            homepage_url,
                            logo_url,
                        },
                    )
                    .collect(),
                Err(error) => {
                    warn!("spo_list query failed: {error}");
                    vec![]
                }
            }
        } else {
            vec![]
        }
    }

    /// Stake distribution for registered SPOs, based on spo_stake_snapshot (latest values).
    async fn stake_distribution(
        &self,
        cx: &Context<'_>,
        limit: Option<i32>,
        offset: Option<i32>,
        search: Option<String>,
        order_by_stake_desc: Option<bool>,
    ) -> Vec<StakeShare> {
        let limit = limit.unwrap_or(50).clamp(1, 500) as i64;
        let offset = offset.unwrap_or(0).max(0) as i64;
        let search = search.as_ref().and_then(|s| {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        });

        if let Some(Db(pool)) = cx.data_opt::<Option<Db>>().and_then(|o| o.as_ref()) {
            // Compute total across pools first for share calculation
            let total_sql = r#"
                SELECT COALESCE(SUM(s.live_stake), 0)::TEXT
                FROM spo_stake_snapshot s
            "#;
            let total_live_str: String =
                match sqlx::query_scalar(total_sql).fetch_one(&**pool).await {
                    Ok(v) => v,
                    Err(error) => {
                        warn!("stake_distribution total stake query failed: {error}");
                        "0".to_owned()
                    }
                };
            let total_live_f64: f64 = total_live_str.parse::<f64>().unwrap_or(0.0);

            let order_desc = order_by_stake_desc.unwrap_or(true);
            let base_select = if search.is_some() {
                r#"
                SELECT 
                    pm.pool_id AS pool_id_hex,
                    pm.name, pm.ticker, pm.homepage_url, pm.url AS logo_url,
                    (s.live_stake)::TEXT, (s.active_stake)::TEXT, s.live_delegators, s.live_saturation,
                    (s.declared_pledge)::TEXT, (s.live_pledge)::TEXT
                FROM spo_stake_snapshot s
                JOIN pool_metadata_cache pm ON pm.pool_id = s.pool_id
                WHERE (
                    pm.name ILIKE $3 OR pm.ticker ILIKE $3 OR pm.homepage_url ILIKE $3 OR pm.pool_id ILIKE $4
                )
                ORDER BY COALESCE(s.live_stake, 0) DESC, pm.pool_id
                LIMIT $1 OFFSET $2
                "#
            } else {
                r#"
                SELECT 
                    pm.pool_id AS pool_id_hex,
                    pm.name, pm.ticker, pm.homepage_url, pm.url AS logo_url,
                    (s.live_stake)::TEXT, (s.active_stake)::TEXT, s.live_delegators, s.live_saturation,
                    (s.declared_pledge)::TEXT, (s.live_pledge)::TEXT
                FROM spo_stake_snapshot s
                JOIN pool_metadata_cache pm ON pm.pool_id = s.pool_id
                ORDER BY COALESCE(s.live_stake, 0) DESC, pm.pool_id
                LIMIT $1 OFFSET $2
                "#
            };

            // Optionally flip order if ascending requested.
            let sql = if order_desc {
                base_select.to_string()
            } else {
                base_select.replace("DESC", "ASC")
            };

            let mut q = sqlx::query_as::<
                _,
                (
                    String,         // pool_id_hex
                    Option<String>, // name
                    Option<String>, // ticker
                    Option<String>, // homepage_url
                    Option<String>, // logo_url
                    Option<String>, // live_stake (TEXT)
                    Option<String>, // active_stake (TEXT)
                    Option<i32>,    // live_delegators
                    Option<f64>,    // live_saturation
                    Option<String>, // declared_pledge (TEXT)
                    Option<String>, // live_pledge (TEXT)
                ),
            >(&sql)
            .bind(limit)
            .bind(offset);

            if let Some(s) = search {
                let s_like = format!("%{s}%");
                q = q.bind(s_like.clone()).bind(s_like);
            }

            match q.fetch_all(&**pool).await {
                Ok(rows) => rows
                    .into_iter()
                    .map(
                        |(
                            pool_id_hex,
                            name,
                            ticker,
                            homepage_url,
                            logo_url,
                            live_stake,
                            active_stake,
                            live_delegators,
                            live_saturation,
                            declared_pledge,
                            live_pledge,
                        )| {
                            // Compute share = live_stake / total_live
                            let share = {
                                let ls = live_stake.as_deref().unwrap_or("0");
                                let lv = ls.parse::<f64>().unwrap_or(0.0);
                                if total_live_f64 > 0.0 {
                                    lv / total_live_f64
                                } else {
                                    0.0
                                }
                            };
                            let live_delegators_i64 = live_delegators.map(|v| v as i64);
                            StakeShare {
                                pool_id_hex,
                                name,
                                ticker,
                                homepage_url,
                                logo_url,
                                live_stake,
                                active_stake,
                                live_delegators: live_delegators_i64,
                                live_saturation,
                                declared_pledge,
                                live_pledge,
                                stake_share: Some(share),
                            }
                        },
                    )
                    .collect(),
                Err(error) => {
                    warn!("stake_distribution query failed: {error}");
                    vec![]
                }
            }
        } else {
            vec![]
        }
    }

    /// Find single SPO by pool ID (hex string).
    async fn spo_by_pool_id(&self, cx: &Context<'_>, pool_id_hex: String) -> Option<Spo> {
        let pool_id_hex = normalize_hex(&pool_id_hex)?;
        if let Some(Db(pool)) = cx.data_opt::<Option<Db>>().and_then(|o| o.as_ref()) {
            // Accept hex string; decode on DB side. pool_id is BYTEA.
            let query = r#"
          SELECT si.pool_id AS pool_id_hex,
                 'UNKNOWN' AS validator_class,
                 si.sidechain_pubkey AS sidechain_pubkey_hex,
                 si.aura_pubkey AS aura_pubkey_hex,
                 pm.name, pm.ticker, pm.homepage_url, pm.url AS logo_url
            FROM spo_identity si
            LEFT JOIN pool_metadata_cache pm ON pm.pool_id = si.pool_id
            WHERE si.pool_id = $1
            LIMIT 1
            "#;
            match sqlx::query_as::<
                _,
                (
                    String,
                    String,
                    String,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                ),
            >(query)
            .bind(&pool_id_hex)
            .fetch_optional(&**pool)
            .await
            {
                Ok(Some((
                    pool_id_hex,
                    validator_class,
                    sidechain_pubkey_hex,
                    aura_pubkey_hex,
                    name,
                    ticker,
                    homepage_url,
                    logo_url,
                ))) => Some(Spo {
                    pool_id_hex,
                    validator_class,
                    sidechain_pubkey_hex,
                    aura_pubkey_hex,
                    name,
                    ticker,
                    homepage_url,
                    logo_url,
                }),
                Err(error) => {
                    warn!("spo_by_pool_id query failed: {error}");
                    None
                }
                Ok(None) => None,
            }
        } else {
            None
        }
    }

    // -------------------------------------------------
    // KPI / Dashboard helpers
    // -------------------------------------------------
    /// Current epoch info with duration and elapsed seconds.
    async fn current_epoch_info(&self, cx: &Context<'_>) -> Option<EpochInfo> {
        if let Some(Db(pool)) = cx.data_opt::<Option<Db>>().and_then(|o| o.as_ref()) {
            let sql = r#"
                WITH last AS (
                    SELECT 
                        epoch_no,
                        EXTRACT(EPOCH FROM starts_at)::BIGINT AS starts_s,
                        EXTRACT(EPOCH FROM ends_at)::BIGINT AS ends_s,
                        EXTRACT(EPOCH FROM (ends_at - starts_at))::BIGINT AS dur_s,
                        EXTRACT(EPOCH FROM NOW())::BIGINT AS now_s
                    FROM epochs
                    ORDER BY epoch_no DESC
                    LIMIT 1
                ), calc AS (
                    SELECT 
                        epoch_no, starts_s, ends_s, dur_s, now_s,
                        CASE WHEN ends_s > now_s THEN 0
                             ELSE ((now_s - ends_s) / dur_s)::BIGINT + 1 END AS n
                    FROM last
                ), synth AS (
                    SELECT 
                        (epoch_no + n) AS epoch_no,
                        dur_s AS duration_seconds,
                        CASE WHEN n = 0 THEN LEAST(GREATEST(now_s - starts_s, 0), dur_s)
                             ELSE LEAST(GREATEST(now_s - (ends_s + (n - 1) * dur_s), 0), dur_s)
                        END AS elapsed_seconds
                    FROM calc
                )
                SELECT epoch_no, duration_seconds, elapsed_seconds FROM synth
            "#;
            match sqlx::query_as::<_, (i64, i64, i64)>(sql)
                .fetch_optional(&**pool)
                .await
            {
                Ok(Some((epoch_no, duration_seconds, elapsed_seconds))) => Some(EpochInfo {
                    epoch_no,
                    duration_seconds,
                    elapsed_seconds,
                }),
                Ok(None) => None,
                Err(error) => {
                    warn!("current_epoch_info query failed: {error}");
                    None
                }
            }
        } else {
            None
        }
    }

    /// Epoch-wide block utilization = sum(produced) / sum(expected) (0.0 if no data or expected ==
    /// 0).
    async fn epoch_utilization(&self, cx: &Context<'_>, epoch: i32) -> Option<f64> {
        if let Some(Db(pool)) = cx.data_opt::<Option<Db>>().and_then(|o| o.as_ref()) {
            let sql = r#"
                SELECT COALESCE(
                    CASE WHEN SUM(expected_blocks) > 0
                         THEN SUM(produced_blocks)::DOUBLE PRECISION / SUM(expected_blocks)
                         ELSE 0.0 END,
                    0.0) AS utilization
                FROM spo_epoch_performance
                WHERE epoch_no = $1
            "#;
            match sqlx::query_scalar::<_, Option<f64>>(sql)
                .bind(epoch as i64)
                .fetch_one(&**pool)
                .await
            {
                Ok(v) => v.or(Some(0.0)),
                Err(error) => {
                    warn!("epoch_utilization query failed: {error}");
                    None
                }
            }
        } else {
            None
        }
    }

    /// Number of SPO identities (with a pool_id present).
    async fn spo_count(&self, cx: &Context<'_>) -> Option<i64> {
        if let Some(Db(pool)) = cx.data_opt::<Option<Db>>().and_then(|o| o.as_ref()) {
            // Single source of truth for current SPOs: spo_stake_snapshot
            let sql = r#"
                SELECT COUNT(1)::BIGINT FROM spo_stake_snapshot
            "#;
            match sqlx::query_scalar::<_, i64>(sql).fetch_one(&**pool).await {
                Ok(count) => Some(count),
                Err(error) => {
                    warn!("spo_count query failed: {error}");
                    None
                }
            }
        } else {
            None
        }
    }

    /// Committee membership for an epoch (ordered by position), with identity enrichment when
    /// available.
    async fn committee(&self, cx: &Context<'_>, epoch: i64) -> Vec<CommitteeMember> {
        if let Some(Db(pool)) = cx.data_opt::<Option<Db>>().and_then(|o| o.as_ref()) {
            let sql = r#"
                SELECT 
                    cm.epoch_no,
                    cm.position,
                    cm.sidechain_pubkey AS sidechain_pubkey_hex,
                    cm.expected_slots,
                    si.aura_pubkey AS aura_pubkey_hex,
                    si.pool_id AS pool_id_hex,
                    si.spo_sk AS spo_sk_hex
                FROM committee_membership cm
                LEFT JOIN spo_identity si ON si.sidechain_pubkey = cm.sidechain_pubkey
                WHERE cm.epoch_no = $1
                ORDER BY cm.position
            "#;
            match sqlx::query_as::<
                _,
                (
                    i64,            // epoch_no
                    i32,            // position
                    String,         // sidechain_pubkey_hex
                    i32,            // expected_slots
                    Option<String>, // aura_pubkey_hex
                    Option<String>, // pool_id_hex
                    Option<String>, // spo_sk_hex
                ),
            >(sql)
            .bind(epoch)
            .fetch_all(&**pool)
            .await
            {
                Ok(rows) => rows
                    .into_iter()
                    .map(
                        |(
                            epoch_no,
                            position,
                            sidechain_pubkey_hex,
                            expected_slots,
                            aura_pubkey_hex,
                            pool_id_hex,
                            spo_sk_hex,
                        )| CommitteeMember {
                            epoch_no,
                            position,
                            sidechain_pubkey_hex,
                            expected_slots,
                            aura_pubkey_hex,
                            pool_id_hex,
                            spo_sk_hex,
                        },
                    )
                    .collect(),
                Err(error) => {
                    warn!("committee query failed: {error}");
                    vec![]
                }
            }
        } else {
            vec![]
        }
    }

    /// Registration counts series for an epoch range. Uses DB when possible.
    async fn registered_spo_series(
        &self,
        cx: &Context<'_>,
        from_epoch: i64,
        to_epoch: i64,
    ) -> Vec<RegisteredStat> {
        let start = from_epoch.min(to_epoch);
        let end = to_epoch.max(from_epoch);
        if let Some(Db(pool)) = cx.data_opt::<Option<Db>>().and_then(|o| o.as_ref()) {
            // Simplified: return raw per-epoch counts directly from DB sources.
            // - federated_valid_count: distinct committee members with expected_slots > 0
            // - registered_valid_count: distinct VALID in spo_history per epoch
            // - registered_invalid_count: distinct INVALID in spo_history per epoch
            // - federated_invalid_count: 0 (not tracked)
            // - dparam: same as registered_valid_count as DOUBLE PRECISION (frontend can derive
            //   other metrics)
            let sql = r#"
                WITH rng AS (
                    SELECT generate_series($1::BIGINT, $2::BIGINT) AS epoch_no
                ),
                hist_valid AS (
                                        SELECT sh.epoch_no,
                                                     COUNT(DISTINCT si.pool_id) AS cnt
                    FROM spo_history sh
                    LEFT JOIN spo_identity si ON si.spo_sk = sh.spo_sk
                    WHERE sh.status IN ('VALID','Valid')
                      AND sh.epoch_no BETWEEN $1::BIGINT AND $2::BIGINT
                                            AND si.pool_id IS NOT NULL
                    GROUP BY sh.epoch_no
                ),
                hist_invalid AS (
                                        SELECT sh.epoch_no,
                                                     COUNT(DISTINCT si.pool_id) AS cnt
                    FROM spo_history sh
                    LEFT JOIN spo_identity si ON si.spo_sk = sh.spo_sk
                    WHERE sh.status IN ('INVALID','Invalid')
                      AND sh.epoch_no BETWEEN $1::BIGINT AND $2::BIGINT
                                            AND si.pool_id IS NOT NULL
                    GROUP BY sh.epoch_no
                ),
                fed AS (
                    SELECT c.epoch_no,
                           COUNT(DISTINCT c.sidechain_pubkey) FILTER (WHERE c.expected_slots > 0) AS federated_valid_count,
                           0::BIGINT AS federated_invalid_count
                    FROM committee_membership c
                    WHERE c.epoch_no BETWEEN $1::BIGINT AND $2::BIGINT
                    GROUP BY c.epoch_no
                )
                SELECT r.epoch_no,
                       COALESCE(f.federated_valid_count, 0) AS federated_valid_count,
                       COALESCE(f.federated_invalid_count, 0) AS federated_invalid_count,
                       COALESCE(hv.cnt, 0) AS registered_valid_count,
                       COALESCE(hi.cnt, 0) AS registered_invalid_count,
                       COALESCE(hv.cnt, 0)::DOUBLE PRECISION AS dparam
                FROM rng r
                LEFT JOIN hist_valid hv ON hv.epoch_no = r.epoch_no
                LEFT JOIN hist_invalid hi ON hi.epoch_no = r.epoch_no
                LEFT JOIN fed f ON f.epoch_no = r.epoch_no
                ORDER BY r.epoch_no
            "#;
            match sqlx::query_as::<_, (i64, i64, i64, i64, i64, Option<f64>)>(sql)
                .bind(start)
                .bind(end)
                .fetch_all(&**pool)
                .await
            {
                Ok(rows) => rows
                    .into_iter()
                    .map(
                        |(epoch_no, f_valid, f_invalid, r_valid, r_invalid, dparam)| {
                            RegisteredStat {
                                epoch_no,
                                federated_valid_count: f_valid,
                                federated_invalid_count: f_invalid,
                                registered_valid_count: r_valid,
                                registered_invalid_count: r_invalid,
                                dparam,
                            }
                        },
                    )
                    .collect(),
                Err(error) => {
                    warn!("registered_spo_series query failed: {error}");
                    vec![]
                }
            }
        } else {
            vec![]
        }
    }

    /// Raw presence events for SPO identity per epoch across sources (history, committee,
    /// performance). Frontend can reconstruct totals/new registrations from these events.
    async fn registered_presence(
        &self,
        cx: &Context<'_>,
        from_epoch: i64,
        to_epoch: i64,
    ) -> Vec<PresenceEvent> {
        let start = from_epoch.min(to_epoch);
        let end = to_epoch.max(from_epoch);
        if let Some(Db(pool)) = cx.data_opt::<Option<Db>>().and_then(|o| o.as_ref()) {
            let sql = r#"
                WITH history AS (
                    SELECT sh.epoch_no::BIGINT AS epoch_no,
                           COALESCE(si.pool_id, sh.spo_sk) AS id_key,
                           'history'::TEXT AS source,
                           sh.status::TEXT AS status
                    FROM spo_history sh
                    LEFT JOIN spo_identity si ON si.spo_sk = sh.spo_sk
                    WHERE sh.epoch_no BETWEEN $1::BIGINT AND $2::BIGINT
                ),
                committee AS (
                    SELECT cm.epoch_no::BIGINT AS epoch_no,
                           COALESCE(si.pool_id, cm.sidechain_pubkey) AS id_key,
                           'committee'::TEXT AS source,
                           NULL::TEXT AS status
                    FROM committee_membership cm
                    LEFT JOIN spo_identity si ON si.sidechain_pubkey = cm.sidechain_pubkey
                    WHERE cm.epoch_no BETWEEN $1::BIGINT AND $2::BIGINT
                ),
                performance AS (
                    SELECT sep.epoch_no::BIGINT AS epoch_no,
                           COALESCE(si.pool_id, sep.spo_sk) AS id_key,
                           'performance'::TEXT AS source,
                           NULL::TEXT AS status
                    FROM spo_epoch_performance sep
                    LEFT JOIN spo_identity si ON si.spo_sk = sep.spo_sk
                    WHERE sep.epoch_no BETWEEN $1::BIGINT AND $2::BIGINT
                )
                SELECT epoch_no, id_key, source, status FROM history
                UNION ALL
                SELECT epoch_no, id_key, source, status FROM committee
                UNION ALL
                SELECT epoch_no, id_key, source, status FROM performance
                ORDER BY epoch_no, source, id_key
            "#;
            match sqlx::query_as::<_, (i64, String, String, Option<String>)>(sql)
                .bind(start)
                .bind(end)
                .fetch_all(&**pool)
                .await
            {
                Ok(rows) => rows
                    .into_iter()
                    .map(|(epoch_no, id_key, source, status)| PresenceEvent {
                        epoch_no,
                        id_key,
                        source,
                        status,
                    })
                    .collect(),
                Err(error) => {
                    warn!("registered_presence query failed: {error}");
                    vec![]
                }
            }
        } else {
            vec![]
        }
    }

    /// First valid epoch per identity (based on spo_history status VALID). Optional cutoff to bound
    /// the scan.
    async fn registered_first_valid_epochs(
        &self,
        cx: &Context<'_>,
        upto_epoch: Option<i64>,
    ) -> Vec<FirstValidEpoch> {
        if let Some(Db(pool)) = cx.data_opt::<Option<Db>>().and_then(|o| o.as_ref()) {
            let sql = r#"
                SELECT COALESCE(si.pool_id, sh.spo_sk) AS id_key,
                       MIN(sh.epoch_no)::BIGINT AS first_valid_epoch
                FROM spo_history sh
                LEFT JOIN spo_identity si ON si.spo_sk = sh.spo_sk
                WHERE sh.status IN ('VALID','Valid')
                  AND ($1::BIGINT IS NULL OR sh.epoch_no <= $1::BIGINT)
                GROUP BY 1
                ORDER BY first_valid_epoch
            "#;
            match sqlx::query_as::<_, (String, i64)>(sql)
                .bind(upto_epoch)
                .fetch_all(&**pool)
                .await
            {
                Ok(rows) => rows
                    .into_iter()
                    .map(|(id_key, first_valid_epoch)| FirstValidEpoch {
                        id_key,
                        first_valid_epoch,
                    })
                    .collect(),
                Err(error) => {
                    warn!("registered_first_valid_epochs query failed: {error}");
                    vec![]
                }
            }
        } else {
            vec![]
        }
    }
}

#[derive(async_graphql::SimpleObject)]
#[graphql(rename_fields = "camelCase")]
pub struct ServiceInfo {
    pub name: String,
    pub version: String,
    pub network: String,
}

#[derive(async_graphql::SimpleObject)]
#[graphql(rename_fields = "camelCase")]
pub struct EpochInfo {
    pub epoch_no: i64,
    pub duration_seconds: i64,
    pub elapsed_seconds: i64,
}

#[derive(async_graphql::SimpleObject)]
#[graphql(rename_fields = "camelCase")]
pub struct EpochPerf {
    pub epoch_no: i64,
    pub spo_sk_hex: String,
    pub produced: i64,
    pub expected: i64,
    pub identity_label: Option<String>,
    pub stake_snapshot: Option<String>,
    pub pool_id_hex: Option<String>,
    pub validator_class: Option<String>,
}

#[derive(async_graphql::SimpleObject)]
#[graphql(rename_fields = "camelCase")]
pub struct Spo {
    pub pool_id_hex: String,
    pub validator_class: String,
    pub sidechain_pubkey_hex: String,
    pub aura_pubkey_hex: Option<String>,
    pub name: Option<String>,
    pub ticker: Option<String>,
    pub homepage_url: Option<String>,
    pub logo_url: Option<String>,
}

#[derive(async_graphql::SimpleObject)]
#[graphql(rename_fields = "camelCase")]
pub struct SpoIdentity {
    pub pool_id_hex: String,
    pub mainchain_pubkey_hex: String,
    pub sidechain_pubkey_hex: String,
    pub aura_pubkey_hex: Option<String>,
    pub validator_class: String,
}

#[derive(async_graphql::SimpleObject)]
#[graphql(rename_fields = "camelCase")]
pub struct PoolMetadata {
    pub pool_id_hex: String,
    pub hex_id: Option<String>,
    pub name: Option<String>,
    pub ticker: Option<String>,
    pub homepage_url: Option<String>,
    pub logo_url: Option<String>,
}

#[derive(async_graphql::SimpleObject)]
#[graphql(rename_fields = "camelCase")]
pub struct SpoComposite {
    pub identity: Option<SpoIdentity>,
    pub metadata: Option<PoolMetadata>,
    pub performance: Vec<EpochPerf>,
}

#[derive(async_graphql::SimpleObject)]
#[graphql(rename_fields = "camelCase")]
pub struct PresenceEvent {
    pub epoch_no: i64,
    pub id_key: String,
    pub source: String,
    pub status: Option<String>,
}

#[derive(async_graphql::SimpleObject)]
#[graphql(rename_fields = "camelCase")]
pub struct FirstValidEpoch {
    pub id_key: String,
    pub first_valid_epoch: i64,
}

#[derive(async_graphql::SimpleObject)]
#[graphql(rename_fields = "camelCase")]
pub struct CommitteeMember {
    pub epoch_no: i64,
    pub position: i32,
    pub sidechain_pubkey_hex: String,
    pub expected_slots: i32,
    pub aura_pubkey_hex: Option<String>,
    pub pool_id_hex: Option<String>,
    pub spo_sk_hex: Option<String>,
}

#[derive(async_graphql::SimpleObject)]
#[graphql(rename_fields = "camelCase")]
pub struct RegisteredStat {
    pub epoch_no: i64,
    pub federated_valid_count: i64,
    pub federated_invalid_count: i64,
    pub registered_valid_count: i64,
    pub registered_invalid_count: i64,
    pub dparam: Option<f64>,
}

#[derive(async_graphql::SimpleObject)]
#[graphql(rename_fields = "camelCase")]
pub struct RegisteredTotals {
    pub epoch_no: i64,
    pub total_registered: i64,
    pub newly_registered: i64,
}

impl EpochPerf {
    fn from_tuple(row: EpochPerfRow) -> Self {
        let (
            epoch_no,
            spo_sk_hex,
            produced_i32,
            expected_i32,
            identity_label,
            stake_snapshot,
            pool_id_hex,
            validator_class,
        ) = row;
        Self {
            epoch_no,
            spo_sk_hex,
            produced: produced_i32 as i64,
            expected: expected_i32 as i64,
            identity_label,
            stake_snapshot,
            pool_id_hex,
            validator_class,
        }
    }
}

#[derive(async_graphql::SimpleObject)]
#[graphql(rename_fields = "camelCase")]
pub struct StakeShare {
    pub pool_id_hex: String,
    pub name: Option<String>,
    pub ticker: Option<String>,
    pub homepage_url: Option<String>,
    pub logo_url: Option<String>,
    pub live_stake: Option<String>,
    pub active_stake: Option<String>,
    pub live_delegators: Option<i64>,
    pub live_saturation: Option<f64>,
    pub declared_pledge: Option<String>,
    pub live_pledge: Option<String>,
    pub stake_share: Option<f64>,
}

async fn graphiql() -> impl IntoResponse {
    info!("serving GraphiQL at /graphql");
    // Because this router is nested under /api/v1, we must point the JS client to the
    // fully-qualified path. Otherwise the generated GraphiQL page will attempt requests to
    // /graphql (404) -> empty body -> JSON parse error.
    Html(
        GraphiQLSource::build()
            .endpoint("/api/v1/graphql")
            .subscription_endpoint("/api/v1/graphql/ws")
            .finish(),
    )
}

// -------------------------------------------------
// Helpers
// -------------------------------------------------
fn normalize_hex(input: &str) -> Option<String> {
    if input.is_empty() {
        return None;
    }
    let s = input
        .strip_prefix("0x")
        .unwrap_or(input)
        .strip_prefix("0X")
        .unwrap_or(input);
    // Accept only even-length hex (bytea) and reasonable size (<= 256 chars to avoid abuse)
    if s.len() % 2 != 0 || s.len() > 256 {
        return None;
    }
    // Cheap validation (compiled once at runtime). If regex creation fails, we fallback to
    // returning original.
    static HEX_RE: std::sync::LazyLock<Regex> =
        std::sync::LazyLock::new(|| Regex::new("^[0-9a-fA-F]+$").unwrap());
    if !HEX_RE.is_match(s) {
        return None;
    }
    Some(s.to_ascii_lowercase())
}

/// Export the GraphQL schema in SDL format.
pub fn export_schema() -> String {
    Schema::build(Query::default(), EmptyMutation, EmptySubscription)
        .finish()
        .sdl()
}
