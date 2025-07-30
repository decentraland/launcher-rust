use std::{collections::VecDeque, path::Path};

use anyhow::{Context, Result};
use log::{error, info};
use rusqlite::{Connection, params};
use segment::message::Message;

use crate::environment::AppEnvironment;

const DEFAULT_EVENT_COUNT_LIMIT: u32 = 200;

#[derive(Clone)]
pub struct AnalyticsEvent {
    pub id: u64,
    pub message: Message,
}

pub trait AnalyticsEventQueue {
    fn enque(&mut self, msg: Message) -> Result<()>;

    fn peek(&self) -> Option<AnalyticsEvent>;

    fn consume(&mut self, id: u64);
}

pub struct PersistentAnalyticsEventQueue {
    conn: Connection,
    event_count_limit: u32,
}

impl PersistentAnalyticsEventQueue {
    pub fn new<P: AsRef<Path>>(path: P, event_count_limit: u32) -> Result<Self> {
        let conn = Connection::open(path).context("Cannot open db")?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS analytics_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp DATETIME NOT NULL DEFAULT (DATETIME('now')),
                message TEXT NOT NULL
            )",
        )
        .context("Cannot create table")?;

        Ok(Self {
            conn,
            event_count_limit,
        })
    }
}

impl AnalyticsEventQueue for PersistentAnalyticsEventQueue {
    fn enque(&mut self, msg: Message) -> Result<()> {
        let json = serde_json::to_string(&msg)?;

        self.conn.execute(
            "INSERT INTO analytics_events (message) VALUES (?1)",
            params![json],
        )?;

        let count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM analytics_events", [], |row| {
                    row.get(0)
                })?;

        if count > i64::from(self.event_count_limit) {
            let to_delete = count.saturating_sub(i64::from(self.event_count_limit));
            self.conn.execute(
                "DELETE FROM analytics_events WHERE id IN (SELECT id FROM analytics_events ORDER BY timestamp ASC LIMIT ?1)",
                params![to_delete],
            )?;
        }

        Ok(())
    }

    fn peek(&self) -> Option<AnalyticsEvent> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, message 
                 FROM analytics_events 
                 ORDER BY timestamp DESC 
                 LIMIT 1",
            )
            .ok()?;

        let mut rows = stmt.query([]).ok()?;
        match rows.next() {
            Ok(row) => {
                if let Some(row) = row {
                    let id: u64 = row.get(0).ok()?;
                    let text: String = row.get(1).ok()?;
                    let message: Message = serde_json::from_str(&text).ok()?;
                    Some(AnalyticsEvent { id, message })
                } else {
                    None
                }
            }

            Err(e) => {
                error!("Cannot read rows from db: {}", e);
                None
            }
        }
    }

    fn consume(&mut self, id: u64) {
        let _ = self
            .conn
            .execute("DELETE FROM analytics_events WHERE id = ?1", params![id]);
    }
}

pub struct InMemoryAnalyticsEventQueue {
    events: VecDeque<AnalyticsEvent>,
    next_id: u64,
    event_count_limit: usize,
}

impl InMemoryAnalyticsEventQueue {
    pub const fn new(event_count_limit: u32) -> Self {
        Self {
            events: VecDeque::new(),
            next_id: 1,
            event_count_limit: event_count_limit as usize,
        }
    }

    fn trim_oldest(&mut self) {
        while self.events.len() > self.event_count_limit {
            self.events.pop_front();
        }
    }
}

impl AnalyticsEventQueue for InMemoryAnalyticsEventQueue {
    fn enque(&mut self, msg: Message) -> Result<()> {
        let event = AnalyticsEvent {
            id: self.next_id,
            message: msg,
        };

        match self.next_id.checked_add(1) {
            Some(new_value) => {
                self.next_id = new_value;
            }
            None => {
                error!("next_id hit limit, behaviour is not supposed");
            }
        }

        self.events.push_back(event);
        self.trim_oldest();

        Ok(())
    }

    fn peek(&self) -> Option<AnalyticsEvent> {
        self.events.back().map(|e| e.clone())
    }

    fn consume(&mut self, id: u64) {
        if let Some(pos) = self.events.iter().position(|e| e.id == id) {
            self.events.remove(pos);
        }
    }
}

pub enum CombinedAnalyticsEventQueue {
    Persistent(PersistentAnalyticsEventQueue),
    InMemory(InMemoryAnalyticsEventQueue),
}

impl AnalyticsEventQueue for CombinedAnalyticsEventQueue {
    fn enque(&mut self, msg: Message) -> Result<()> {
        match self {
            Self::Persistent(queue) => queue.enque(msg),
            Self::InMemory(queue) => queue.enque(msg),
        }
    }

    fn peek(&self) -> Option<AnalyticsEvent> {
        match self {
            Self::Persistent(queue) => queue.peek(),
            Self::InMemory(queue) => queue.peek(),
        }
    }

    fn consume(&mut self, id: u64) {
        match self {
            Self::Persistent(queue) => {
                queue.consume(id);
            }
            Self::InMemory(queue) => {
                queue.consume(id);
            }
        }
    }
}

impl Default for CombinedAnalyticsEventQueue {
    fn default() -> Self {
        if AppEnvironment::cmd_args().force_in_memory_analytics_queue {
            info!(
                "CombinedAnalyticsEventQueue created with InMemory queue by flag, InMemoryAnalyticsEventQueue in use"
            );
            return Self::InMemory(InMemoryAnalyticsEventQueue::new(DEFAULT_EVENT_COUNT_LIMIT));
        }

        let persistent = PersistentAnalyticsEventQueue::new(
            crate::installs::analytics_queue_db_path(),
            DEFAULT_EVENT_COUNT_LIMIT,
        );

        match persistent {
            Ok(persistent) => Self::Persistent(persistent),
            Err(e) => {
                error!(
                    "Cannot create persistent event queue, fallback to InMemory queue: {}",
                    e
                );
                Self::InMemory(InMemoryAnalyticsEventQueue::new(DEFAULT_EVENT_COUNT_LIMIT))
            }
        }
    }
}
