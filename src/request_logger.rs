use std::time::Instant;

use axum::{
    body::Body,
    extract::{FromRequest, Request},
};
use chrono::{DateTime, Local};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Serialize, Deserialize, Hash, PartialEq, Eq, Debug, Clone)]
pub enum Method {
    #[serde(rename = "get")]
    Get,
    #[serde(rename = "post")]
    Post,
    #[serde(rename = "put")]
    Put,
    #[serde(rename = "delete")]
    Delete,
    #[serde(rename = "patch")]
    Patch,
}

#[derive(Serialize, PartialEq, Debug, Clone)]
pub struct RequestLog {
    pub method: Method,
    pub headers: IndexMap<String, String>,
    pub path: String,
    pub query: IndexMap<String, String>,
    pub body: String,
    pub requested_at: DateTime<Local>,
}

pub struct RequestLogger {
    pool: SqlitePool,
}

const SCHEMA: &str = r#"
DROP TABLE IF EXISTS session;
CREATE TABLE session (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name VARCBAR(255) UNIQUE NOT NULL
);

DROP TABLE IF EXISTS request_log;
CREATE TABLE request_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id INTEGER NOT NULL,
    method VARCHAR(255) NOT NULL,
    path VARCHAR(255) NOT NULL,
    body TEXT NOT NULL,
    requested_at TIMESTAMP NOT NULL,
    FOREIGN KEY (session_id) REFERENCES session(id)
);

DROP TABLE IF EXISTS request_header;
CREATE TABLE request_header (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    request_log_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    value TEXT NOT NULL,
    FOREIGN KEY (request_log_id) REFERENCES request_log(id)
);

DROP TABLE IF EXISTS request_query;
CREATE TABLE request_query (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    request_log_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    value TEXT NOT NULL,
    FOREIGN KEY (request_log_id) REFERENCES request_log(id)
);
"#;

#[derive(Debug, PartialEq)]
pub enum LoggerError {
    InvalidSession(String),
    InternalError(String),
}

pub type LoggerResult<T> = Result<T, LoggerError>;

impl RequestLogger {
    pub fn new(pool: SqlitePool) -> Result<Self, String> {
        Ok(Self { pool })
    }

    pub async fn init(&self) -> LoggerResult<()> {
        sqlx::query(SCHEMA)
            .execute(&self.pool)
            .await
            .map_err(|err| LoggerError::InternalError(err.to_string()))?;
        Ok(())
    }

    pub async fn create_session(&self, session: &str) -> LoggerResult<()> {
        sqlx::query("INSERT INTO session (name) VALUES (?)")
            .bind(session)
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(|err| {
                err.as_database_error()
                    .and_then(|derr| {
                        if derr.is_unique_violation() {
                            Some(LoggerError::InvalidSession(format!(
                                "session \"{}\" is already exists",
                                session
                            )))
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| LoggerError::InvalidSession(err.to_string()))
            })?;
        Ok(())
    }

    pub async fn delete_session(&self, session: &str) -> LoggerResult<()> {
        let qr = sqlx::query("DELETE FROM session WHERE name = ?")
            .bind(session)
            .execute(&self.pool)
            .await
            .map_err(|err| LoggerError::InternalError(err.to_string()))?;

        if qr.rows_affected() == 0 {
            Err(LoggerError::InvalidSession(format!(
                "session \"{}\" is not found",
                session
            )))
        } else {
            Ok(())
        }
    }

    pub async fn log_request(&self, session: &str, log: &RequestLog) -> LoggerResult<()> {
        Ok(())
    }

    pub async fn get_session_history(&self, session: &str) -> LoggerResult<Vec<RequestLog>> {
        unimplemented!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT_SESSION: &str = "default_session";

    async fn new_logger() -> RequestLogger {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .connect(":memory:")
            .await
            .unwrap();
        let logger = RequestLogger::new(pool).unwrap();
        logger.init().await.unwrap();
        logger.create_session(DEFAULT_SESSION).await.unwrap();
        logger
    }

    mod create_session {
        use super::*;
        use pretty_assertions::assert_eq;

        #[tokio::test]
        async fn with_unique_sessions() {
            let logger = new_logger().await;
            assert_eq!(logger.create_session("new_session").await, Ok(()));
        }

        #[tokio::test]
        async fn with_duplicated_sessions() {
            let logger = new_logger().await;
            assert_eq!(
                logger.create_session(DEFAULT_SESSION).await,
                Err(LoggerError::InvalidSession(format!(
                    "session \"{}\" is already exists",
                    DEFAULT_SESSION
                )))
            );
        }
    }

    mod delete_session {
        use super::*;
        use pretty_assertions::assert_eq;

        #[tokio::test]
        async fn with_exist_session() {
            let logger = new_logger().await;
            assert_eq!(logger.delete_session(DEFAULT_SESSION).await, Ok(()));
        }

        #[tokio::test]
        async fn with_not_exist_sessions() {
            let logger = new_logger().await;
            assert_eq!(
                logger.delete_session("new_session").await,
                Err(LoggerError::InvalidSession(
                    "session \"new_session\" is not found".to_string()
                ))
            );
        }
    }
}
