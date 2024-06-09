use chrono::{DateTime, Local};
use indexmap::IndexMap;
use serde::Serialize;
use sqlx::{error::ErrorKind, prelude::FromRow, SqlitePool};

use crate::method::Method;

#[derive(Serialize, PartialEq, Debug, Clone)]
pub struct RequestLog {
    pub method: Method,
    pub headers: IndexMap<String, String>,
    pub path: String,
    pub query: IndexMap<String, String>,
    pub body: String,
    pub requested_at: DateTime<Local>,
}

#[derive(Clone)]
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
    FOREIGN KEY (session_id) REFERENCES session(id) ON DELETE CASCADE
);

DROP TABLE IF EXISTS request_header;
CREATE TABLE request_header (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    request_log_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    value TEXT NOT NULL,
    FOREIGN KEY (request_log_id) REFERENCES request_log(id) ON DELETE CASCADE
);

DROP TABLE IF EXISTS request_query;
CREATE TABLE request_query (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    request_log_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    value TEXT NOT NULL,
    FOREIGN KEY (request_log_id) REFERENCES request_log(id) ON DELETE CASCADE
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
                                "session \"{}\" already exists",
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
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|err| LoggerError::InternalError(err.to_string()))?;

        // Insert request_log
        let request_log_id = sqlx::query("INSERT INTO request_log (session_id, method, path, body, requested_at) VALUES ((SELECT id FROM session WHERE name = ?), ?, ?, ?, ?)")
            .bind(session)
            .bind(log.method.to_string())
            .bind(log.path.as_str())
            .bind(log.body.as_str())
            .bind(log.requested_at)
            .execute(&mut *tx)
            .await
            .map(|qr| qr.last_insert_rowid())
            .map_err(|err| {
                match err.as_database_error() {
                    Some(derr) if derr.kind() == ErrorKind::NotNullViolation =>
                        LoggerError::InvalidSession(format!("session \"{}\" is not found", session),
                    ),
                    _ => LoggerError::InternalError(err.to_string()),
                }
            })?;

        // Insert request_header
        if !log.headers.is_empty() {
            let prepared = format!(
                "INSERT INTO request_header (request_log_id, name, value) VALUES {}",
                vec!["(?, ?, ?)"; log.headers.len()].join(", ")
            );

            log.headers
                .iter()
                .fold(sqlx::query(&prepared), |query, (name, value)| {
                    query
                        .bind(request_log_id)
                        .bind(name.as_str())
                        .bind(value.as_str())
                })
                .execute(&mut *tx)
                .await
                .map_err(|err| LoggerError::InternalError(err.to_string()))?;
        }

        // Insert request_query
        if !log.query.is_empty() {
            let prepared = format!(
                "INSERT INTO request_query (request_log_id, name, value) VALUES {}",
                vec!["(?, ?, ?)"; log.query.len()].join(", ")
            );

            log.query
                .iter()
                .fold(sqlx::query(&prepared), |query, (name, value)| {
                    query
                        .bind(request_log_id)
                        .bind(name.as_str())
                        .bind(value.as_str())
                })
                .execute(&mut *tx)
                .await
                .map_err(|err| LoggerError::InternalError(err.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|err| LoggerError::InternalError(err.to_string()))?;
        Ok(())
    }

    pub async fn get_session_history(&self, session: &str) -> LoggerResult<Vec<RequestLog>> {
        #[derive(FromRow)]
        struct SessionRow {
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

        let session_id = sqlx::query_as::<_, SessionRow>("SELECT id FROM session WHERE name = ?")
            .bind(session)
            .fetch_optional(&self.pool)
            .await
            .map_err(|err| LoggerError::InternalError(err.to_string()))
            .and_then(|session_opt| {
                session_opt.ok_or_else(|| {
                    LoggerError::InvalidSession(format!("session \"{}\" is not found", session))
                })
            })?
            .id;

        let logs: Vec<RequestLogRow> = sqlx::query_as(
            "SELECT id, method, path, body, requested_at FROM request_log WHERE session_id = ?",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|err| LoggerError::InternalError(err.to_string()))?;

        let all_headers: Vec<RequestHeaderRow> = sqlx::query_as(
            "SELECT request_log_id, name, value FROM request_header LEFT JOIN request_log ON request_log.id = request_header.request_log_id WHERE request_log.session_id = ?",
        ).bind(session_id).fetch_all(&self.pool).await.map_err(|err| LoggerError::InternalError(err.to_string()))?;

        let headers: IndexMap<i64, Vec<RequestHeaderRow>> =
            all_headers
                .into_iter()
                .fold(IndexMap::new(), |mut acc, row| {
                    acc.entry(row.request_log_id).or_default().push(row);
                    acc
                });

        let all_queries: Vec<RequestQueryRow> = sqlx::query_as(
            "SELECT request_log_id, name, value FROM request_query LEFT JOIN request_log ON request_log.id = request_query.request_log_id WHERE request_log.session_id = ?",
        ).bind(session_id).fetch_all(&self.pool).await.map_err(|err| LoggerError::InternalError(err.to_string()))?;

        let queries: IndexMap<i64, Vec<RequestQueryRow>> =
            all_queries
                .into_iter()
                .fold(IndexMap::new(), |mut acc, row| {
                    acc.entry(row.request_log_id).or_default().push(row);
                    acc
                });

        logs.into_iter()
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

                log.method
                    .as_str()
                    .try_into()
                    .map_err(|err: String| LoggerError::InternalError(err))
                    .map(|method| RequestLog {
                        method,
                        headers,
                        path: log.path,
                        query: queries,
                        body: log.body,
                        requested_at: log.requested_at,
                    })
            })
            .collect::<Result<Vec<_>, _>>()
    }
}

#[cfg(test)]
pub mod testutil {
    use super::*;

    pub async fn new_logger() -> RequestLogger {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .connect(":memory:")
            .await
            .unwrap();
        let logger = RequestLogger::new(pool).unwrap();
        logger.init().await.unwrap();
        logger
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT_SESSION: &str = "default_session";

    async fn new_logger_with_default_session() -> RequestLogger {
        let logger = testutil::new_logger().await;
        logger.create_session(DEFAULT_SESSION).await.unwrap();
        logger
    }

    mod create_session {
        use super::*;
        use pretty_assertions::assert_eq;

        #[tokio::test]
        async fn with_unique_sessions() {
            let logger = new_logger_with_default_session().await;
            assert_eq!(logger.create_session("new_session").await, Ok(()));
        }

        #[tokio::test]
        async fn with_duplicated_sessions() {
            let logger = new_logger_with_default_session().await;
            assert_eq!(
                logger.create_session(DEFAULT_SESSION).await,
                Err(LoggerError::InvalidSession(format!(
                    "session \"{}\" already exists",
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
            let logger = new_logger_with_default_session().await;
            assert_eq!(logger.delete_session(DEFAULT_SESSION).await, Ok(()));
        }

        #[tokio::test]
        async fn with_not_exist_sessions() {
            let logger = new_logger_with_default_session().await;
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

            let logger = new_logger_with_default_session().await;

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
            let logger = new_logger_with_default_session().await;

            assert_eq!(
                Ok(vec![]),
                logger.get_session_history(DEFAULT_SESSION).await,
            );
        }

        #[tokio::test]
        async fn when_unknown_session_is_passed_to_log_request() {
            let logger = new_logger_with_default_session().await;

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

        #[tokio::test]
        async fn when_unknown_session_is_passed_to_get_session_history() {
            let logger = new_logger_with_default_session().await;

            assert_eq!(
                Err(LoggerError::InvalidSession(
                    "session \"unknown_session\" is not found".to_string()
                )),
                logger.get_session_history("unknown_session",).await,
            );
        }
    }
}
