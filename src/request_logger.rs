use std::fmt::Display;

use chrono::{DateTime, Local};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use sqlx::{error::ErrorKind, prelude::FromRow, SqlitePool};

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

impl Display for Method {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Method::Get => write!(f, "get"),
            Method::Post => write!(f, "post"),
            Method::Put => write!(f, "put"),
            Method::Delete => write!(f, "delete"),
            Method::Patch => write!(f, "patch"),
        }
    }
}

impl TryFrom<&str> for Method {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "get" => Ok(Method::Get),
            "post" => Ok(Method::Post),
            "put" => Ok(Method::Put),
            "delete" => Ok(Method::Delete),
            "patch" => Ok(Method::Patch),
            _ => Err(format!("unknown method: {}", value)),
        }
    }
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
        // FIXME: remove .unwrap()

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|err| LoggerError::InternalError(err.to_string()))?;

        // Insert request_log
        let qr = sqlx::query("INSERT INTO request_log (session_id, method, path, body, requested_at) VALUES ((SELECT id FROM session WHERE name = ?), ?, ?, ?, ?)")
            .bind(session)
            .bind(log.method.to_string())
            .bind(log.path.as_str())
            .bind(log.body.as_str())
            .bind(log.requested_at)
            .execute(&mut *tx)
            .await;

        if let Err(err) = qr {
            return match err.as_database_error() {
                Some(derr) if derr.kind() == ErrorKind::NotNullViolation => Err(
                    LoggerError::InvalidSession(format!("session \"{}\" is not found", session)),
                ),
                _ => Err(LoggerError::InternalError(err.to_string())),
            };
        }

        let request_log_id = qr.unwrap().last_insert_rowid();

        // Insert request_header
        for (name, value) in &log.headers {
            sqlx::query(
                "INSERT INTO request_header (request_log_id, name, value) VALUES (?, ?, ?)",
            )
            .bind(request_log_id)
            .bind(name.as_str())
            .bind(value.as_str())
            .execute(&mut *tx)
            .await
            .unwrap();
        }

        // Insert request_query
        for (name, value) in &log.query {
            sqlx::query("INSERT INTO request_query (request_log_id, name, value) VALUES (?, ?, ?)")
                .bind(request_log_id)
                .bind(name.as_str())
                .bind(value.as_str())
                .execute(&mut *tx)
                .await
                .unwrap();
        }

        tx.commit().await.unwrap();
        Ok(())
    }

    pub async fn get_session_history(&self, session: &str) -> LoggerResult<Vec<RequestLog>> {
        // FIXME: remove .unwrap()

        #[derive(FromRow)]
        struct SeesionRow {
            id: i64,
        }

        #[derive(FromRow)]
        struct RequestLogRow {
            id: i64,
            method: String,
            path: String,
            body: String,
            requested_at: DateTime<Local>,
        }

        #[derive(FromRow)]
        struct RequestHeaderRow {
            request_log_id: i64,
            name: String,
            value: String,
        }

        #[derive(FromRow)]
        struct RequestQueryRow {
            request_log_id: i64,
            name: String,
            value: String,
        }

        let session: SeesionRow = sqlx::query_as("SELECT id FROM session WHERE name = ?")
            .bind(session)
            .fetch_one(&self.pool)
            .await
            .map_err(|err| LoggerError::InternalError(err.to_string()))
            .unwrap();

        let session_id = session.id;

        let logs: Vec<RequestLogRow> = sqlx::query_as(
            "SELECT id, method, path, body, requested_at FROM request_log WHERE session_id = ?",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await
        .unwrap();

        let all_headers: Vec<RequestHeaderRow> = sqlx::query_as(
            "SELECT request_log_id, name, value FROM request_header LEFT JOIN request_log ON request_log.id = request_header.request_log_id WHERE request_log.session_id = ?",
        ).bind(session_id).fetch_all(&self.pool).await.unwrap();

        let headers: IndexMap<i64, Vec<RequestHeaderRow>> =
            all_headers
                .into_iter()
                .fold(IndexMap::new(), |mut acc, row| {
                    acc.entry(row.request_log_id).or_default().push(row);
                    acc
                });

        let all_queries: Vec<RequestQueryRow> = sqlx::query_as(
            "SELECT request_log_id, name, value FROM request_query LEFT JOIN request_log ON request_log.id = request_query.request_log_id WHERE request_log.session_id = ?",
        ).bind(session_id).fetch_all(&self.pool).await.unwrap();

        let queries: IndexMap<i64, Vec<RequestQueryRow>> =
            all_queries
                .into_iter()
                .fold(IndexMap::new(), |mut acc, row| {
                    acc.entry(row.request_log_id).or_default().push(row);
                    acc
                });

        Ok(logs
            .into_iter()
            .map(|log| {
                let headers = headers
                    .get(&log.id)
                    .map(|rows| {
                        rows.iter()
                            .map(|row| (row.name.clone(), row.value.clone()))
                            .collect()
                    })
                    .unwrap_or_default();

                let queries = queries
                    .get(&log.id)
                    .map(|rows| {
                        rows.iter()
                            .map(|row| (row.name.clone(), row.value.clone()))
                            .collect()
                    })
                    .unwrap_or_default();

                RequestLog {
                    method: log.method.as_str().try_into().unwrap(),
                    headers,
                    path: log.path,
                    query: queries,
                    body: log.body,
                    requested_at: log.requested_at,
                }
            })
            .collect())
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

    mod log_request_and_get_session_history {
        use super::*;
        use chrono::NaiveDate;
        use chrono::TimeZone;
        use indexmap::indexmap;
        use pretty_assertions::assert_eq;

        #[tokio::test]
        async fn when_two_requests_are_logged() {
            let log1_requested_at = Local
                .from_local_datetime(
                    &NaiveDate::from_ymd_opt(2024, 1, 2)
                        .unwrap()
                        .and_hms_opt(3, 4, 5)
                        .unwrap(),
                )
                .unwrap();
            let log1 = RequestLog {
                method: Method::Get,
                headers: IndexMap::new(),
                path: "/hello".to_string(),
                query: indexmap! {
                    "qname1".to_string() => "qvalue1".to_string(),
                    "qname2".to_string() => "qvalue2".to_string(),
                },
                body: "".to_string(),
                requested_at: log1_requested_at,
            };

            let log2_requested_at = Local
                .from_local_datetime(
                    &NaiveDate::from_ymd_opt(2024, 1, 2)
                        .unwrap()
                        .and_hms_opt(3, 4, 6)
                        .unwrap(),
                )
                .unwrap();
            let log2 = RequestLog {
                method: Method::Post,
                headers: indexmap! {
                    "hname1".to_string() => "hvalue1".to_string(),
                    "hname2".to_string() => "hvalue2".to_string(),
                },
                path: "/greet".to_string(),
                query: IndexMap::new(),
                body: r#"{"message":"hi"}"#.to_string(),
                requested_at: log2_requested_at,
            };

            let log3_requested_at = Local
                .from_local_datetime(
                    &NaiveDate::from_ymd_opt(2024, 1, 2)
                        .unwrap()
                        .and_hms_opt(3, 4, 7)
                        .unwrap(),
                )
                .unwrap();
            let log3 = RequestLog {
                method: Method::Delete,
                headers: indexmap! {},
                path: "/bye".to_string(),
                query: IndexMap::new(),
                body: "".to_string(),
                requested_at: log3_requested_at,
            };

            let logger = new_logger().await;

            let another_session = "another_session";
            logger.create_session(another_session).await.unwrap();

            logger.log_request(DEFAULT_SESSION, &log1).await.unwrap();
            logger.log_request(DEFAULT_SESSION, &log2).await.unwrap();
            logger.log_request(another_session, &log3).await.unwrap();

            assert_eq!(
                Ok(vec![log1, log2]),
                logger.get_session_history(DEFAULT_SESSION).await,
            );
        }

        #[tokio::test]
        async fn when_no_requests_are_logged() {
            let logger = new_logger().await;

            assert_eq!(
                Ok(vec![]),
                logger.get_session_history(DEFAULT_SESSION).await,
            );
        }

        #[tokio::test]
        async fn when_unknown_session_is_passed_to_log_request() {
            let logger = new_logger().await;

            assert_eq!(
                Err(LoggerError::InvalidSession(
                    "session \"unknown_session\" is not found".to_string()
                )),
                logger
                    .log_request(
                        "unknown_session",
                        &RequestLog {
                            method: Method::Get,
                            headers: IndexMap::new(),
                            path: "/hello".to_string(),
                            query: IndexMap::new(),
                            body: "".to_string(),
                            requested_at: Local::now(),
                        }
                    )
                    .await,
            );
        }
    }
}
