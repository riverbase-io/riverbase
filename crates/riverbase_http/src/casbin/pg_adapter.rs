//! Postgres Casbin [`Adapter`] storing rules in `casbin_rule`.

use async_trait::async_trait;
use casbin::error::AdapterError;
use casbin::{Adapter, Filter, Model, Result as CasbinResult};
use diesel::sql_types::{Integer, Nullable, Text};
use diesel::{sql_query, QueryableByName};
use diesel_async::RunQueryDsl;
use riverbase_core::datastore::PgPool;

const ENSURE_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS casbin_rule (
    id SERIAL PRIMARY KEY,
    ptype VARCHAR(100) NOT NULL,
    v0 VARCHAR(100),
    v1 VARCHAR(100),
    v2 VARCHAR(100),
    v3 VARCHAR(100),
    v4 VARCHAR(100),
    v5 VARCHAR(100)
);
CREATE INDEX IF NOT EXISTS idx_casbin_rule_ptype ON casbin_rule (ptype);
"#;

#[derive(Debug, QueryableByName)]
struct CasbinRuleRow {
    #[diesel(sql_type = Integer)]
    #[allow(dead_code)]
    id: i32,
    #[diesel(sql_type = Text)]
    ptype: String,
    #[diesel(sql_type = Nullable<Text>)]
    v0: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    v1: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    v2: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    v3: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    v4: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    v5: Option<String>,
}

/// Casbin policy adapter backed by Postgres `casbin_rule`.
pub struct PgCasbinAdapter {
    pool: PgPool,
    is_filtered: bool,
}

impl PgCasbinAdapter {
    /// Construct a new value.
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            is_filtered: false,
        }
    }

    /// Ensure schema.
    pub async fn ensure_schema(&self) -> CasbinResult<()> {
        let mut conn = self.pool.get().await.map_err(adapter_err)?;
        for stmt in ENSURE_SCHEMA_SQL
            .split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            sql_query(stmt)
                .execute(&mut *conn)
                .await
                .map_err(adapter_err)?;
        }
        Ok(())
    }

    async fn load_all(&self) -> CasbinResult<Vec<CasbinRuleRow>> {
        let mut conn = self.pool.get().await.map_err(adapter_err)?;
        sql_query("SELECT id, ptype, v0, v1, v2, v3, v4, v5 FROM casbin_rule")
            .load(&mut *conn)
            .await
            .map_err(adapter_err)
    }

    fn row_to_rule(row: &CasbinRuleRow) -> Vec<String> {
        let mut rule = vec![row.ptype.clone()];
        for v in [&row.v0, &row.v1, &row.v2, &row.v3, &row.v4, &row.v5] {
            if let Some(val) = v {
                rule.push(val.clone());
            } else {
                break;
            }
        }
        rule
    }

    fn load_policy_line(rule: &[String], m: &mut dyn Model) {
        if rule.len() < 2 {
            return;
        }
        let sec = rule[0]
            .chars()
            .next()
            .map(|c| c.to_string())
            .unwrap_or_default();
        let ptype = &rule[0];
        let values = rule[1..].to_vec();
        if let Some(ast_map) = m.get_mut_model().get_mut(&sec) {
            if let Some(ast) = ast_map.get_mut(ptype) {
                ast.get_mut_policy().insert(values);
            }
        }
    }

    async fn insert_rule(&self, ptype: &str, rule: &[String]) -> CasbinResult<bool> {
        let mut vals = [
            const { None::<String> },
            const { None::<String> },
            const { None::<String> },
            const { None::<String> },
            const { None::<String> },
            const { None::<String> },
        ];
        for (i, part) in rule.iter().take(6).enumerate() {
            vals[i] = Some(part.clone());
        }
        let mut conn = self.pool.get().await.map_err(adapter_err)?;
        let inserted = sql_query(
            r#"
            INSERT INTO casbin_rule (ptype, v0, v1, v2, v3, v4, v5)
            SELECT $1, $2, $3, $4, $5, $6, $7
            WHERE NOT EXISTS (
                SELECT 1 FROM casbin_rule
                WHERE ptype IS NOT DISTINCT FROM $1
                  AND v0 IS NOT DISTINCT FROM $2
                  AND v1 IS NOT DISTINCT FROM $3
                  AND v2 IS NOT DISTINCT FROM $4
                  AND v3 IS NOT DISTINCT FROM $5
                  AND v4 IS NOT DISTINCT FROM $6
                  AND v5 IS NOT DISTINCT FROM $7
            )
            "#,
        )
        .bind::<Text, _>(ptype)
        .bind::<Nullable<Text>, _>(vals[0].as_deref())
        .bind::<Nullable<Text>, _>(vals[1].as_deref())
        .bind::<Nullable<Text>, _>(vals[2].as_deref())
        .bind::<Nullable<Text>, _>(vals[3].as_deref())
        .bind::<Nullable<Text>, _>(vals[4].as_deref())
        .bind::<Nullable<Text>, _>(vals[5].as_deref())
        .execute(&mut *conn)
        .await
        .map_err(adapter_err)?;
        Ok(inserted > 0)
    }

    async fn delete_rule(&self, ptype: &str, rule: &[String]) -> CasbinResult<bool> {
        let mut vals = [
            const { None::<String> },
            const { None::<String> },
            const { None::<String> },
            const { None::<String> },
            const { None::<String> },
            const { None::<String> },
        ];
        for (i, part) in rule.iter().take(6).enumerate() {
            vals[i] = Some(part.clone());
        }
        let mut conn = self.pool.get().await.map_err(adapter_err)?;
        let deleted = sql_query(
            r#"
            DELETE FROM casbin_rule
            WHERE ptype IS NOT DISTINCT FROM $1
              AND v0 IS NOT DISTINCT FROM $2
              AND v1 IS NOT DISTINCT FROM $3
              AND v2 IS NOT DISTINCT FROM $4
              AND v3 IS NOT DISTINCT FROM $5
              AND v4 IS NOT DISTINCT FROM $6
              AND v5 IS NOT DISTINCT FROM $7
            "#,
        )
        .bind::<Text, _>(ptype)
        .bind::<Nullable<Text>, _>(vals[0].as_deref())
        .bind::<Nullable<Text>, _>(vals[1].as_deref())
        .bind::<Nullable<Text>, _>(vals[2].as_deref())
        .bind::<Nullable<Text>, _>(vals[3].as_deref())
        .bind::<Nullable<Text>, _>(vals[4].as_deref())
        .bind::<Nullable<Text>, _>(vals[5].as_deref())
        .execute(&mut *conn)
        .await
        .map_err(adapter_err)?;
        Ok(deleted > 0)
    }
}

fn adapter_err<E: std::fmt::Display>(err: E) -> casbin::Error {
    casbin::Error::AdapterError(AdapterError(Box::new(std::io::Error::new(
        std::io::ErrorKind::Other,
        err.to_string(),
    ))))
}

#[async_trait]
impl Adapter for PgCasbinAdapter {
    async fn load_policy(&mut self, m: &mut dyn Model) -> CasbinResult<()> {
        self.is_filtered = false;
        for row in self.load_all().await? {
            let rule = Self::row_to_rule(&row);
            Self::load_policy_line(&rule, m);
        }
        Ok(())
    }

    async fn load_filtered_policy<'a>(
        &mut self,
        m: &mut dyn Model,
        f: Filter<'a>,
    ) -> CasbinResult<()> {
        self.is_filtered = false;
        for row in self.load_all().await? {
            let rule = Self::row_to_rule(&row);
            if rule.is_empty() {
                continue;
            }
            let sec = rule[0].chars().next().unwrap_or('p').to_string();
            let mut filtered = false;
            if sec == "p" {
                for (i, r) in f.p.iter().enumerate() {
                    if !r.is_empty() && rule.get(i + 1).map(String::as_str) != Some(r) {
                        filtered = true;
                    }
                }
            } else if sec == "g" {
                for (i, r) in f.g.iter().enumerate() {
                    if !r.is_empty() && rule.get(i + 1).map(String::as_str) != Some(r) {
                        filtered = true;
                    }
                }
            }
            if filtered {
                self.is_filtered = true;
            } else {
                Self::load_policy_line(&rule, m);
            }
        }
        Ok(())
    }

    async fn save_policy(&mut self, m: &mut dyn Model) -> CasbinResult<()> {
        let mut conn = self.pool.get().await.map_err(adapter_err)?;
        sql_query("DELETE FROM casbin_rule")
            .execute(&mut *conn)
            .await
            .map_err(adapter_err)?;
        drop(conn);

        if let Some(ast_map) = m.get_model().get("p") {
            for (ptype, ast) in ast_map {
                for policy in ast.get_policy() {
                    self.insert_rule(ptype, policy).await?;
                }
            }
        }
        if let Some(ast_map) = m.get_model().get("g") {
            for (ptype, ast) in ast_map {
                for policy in ast.get_policy() {
                    self.insert_rule(ptype, policy).await?;
                }
            }
        }
        Ok(())
    }

    async fn clear_policy(&mut self) -> CasbinResult<()> {
        let mut conn = self.pool.get().await.map_err(adapter_err)?;
        sql_query("DELETE FROM casbin_rule")
            .execute(&mut *conn)
            .await
            .map_err(adapter_err)?;
        self.is_filtered = false;
        Ok(())
    }

    async fn add_policy(
        &mut self,
        _sec: &str,
        ptype: &str,
        rule: Vec<String>,
    ) -> CasbinResult<bool> {
        self.insert_rule(ptype, &rule).await
    }

    async fn add_policies(
        &mut self,
        _sec: &str,
        ptype: &str,
        rules: Vec<Vec<String>>,
    ) -> CasbinResult<bool> {
        let mut all = true;
        for rule in rules {
            if !self.insert_rule(ptype, &rule).await? {
                all = false;
            }
        }
        Ok(all)
    }

    async fn remove_policy(
        &mut self,
        _sec: &str,
        ptype: &str,
        rule: Vec<String>,
    ) -> CasbinResult<bool> {
        self.delete_rule(ptype, &rule).await
    }

    async fn remove_policies(
        &mut self,
        _sec: &str,
        ptype: &str,
        rules: Vec<Vec<String>>,
    ) -> CasbinResult<bool> {
        let mut all = true;
        for rule in rules {
            if !self.delete_rule(ptype, &rule).await? {
                all = false;
            }
        }
        Ok(all)
    }

    async fn remove_filtered_policy(
        &mut self,
        _sec: &str,
        ptype: &str,
        field_index: usize,
        field_values: Vec<String>,
    ) -> CasbinResult<bool> {
        if field_values.is_empty() {
            return Ok(false);
        }
        let rows = self.load_all().await?;
        let mut removed = false;
        for row in rows {
            if row.ptype != ptype {
                continue;
            }
            let rule = Self::row_to_rule(&row);
            let values = &rule[1..];
            let mut matched = true;
            for (i, field_value) in field_values.iter().enumerate() {
                if field_value.is_empty() {
                    continue;
                }
                if values.get(field_index + i).map(String::as_str) != Some(field_value.as_str()) {
                    matched = false;
                    break;
                }
            }
            if matched {
                removed |= self.delete_rule(ptype, values).await?;
            }
        }
        Ok(removed)
    }

    fn is_filtered(&self) -> bool {
        self.is_filtered
    }
}
