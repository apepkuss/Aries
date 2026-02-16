use std::sync::{
    Once,
    atomic::{AtomicI64, Ordering},
};

use rusqlite::Connection;

use super::schema;
use crate::error::{LantaiError, LantaiResult};

/// sqlite-vec 全局注册（进程生命周期内只执行一次）
static SQLITE_VEC_INIT: Once = Once::new();

fn register_sqlite_vec() {
    SQLITE_VEC_INIT.call_once(|| unsafe {
        // sqlite-vec 扩展入口函数，通过 sqlite3_auto_extension 注册
        type AutoExtFn = unsafe extern "C" fn(
            *mut rusqlite::ffi::sqlite3,
            *mut *mut std::ffi::c_char,
            *const rusqlite::ffi::sqlite3_api_routines,
        ) -> std::ffi::c_int;
        let init: AutoExtFn = std::mem::transmute(sqlite_vec::sqlite3_vec_init as *const ());
        rusqlite::ffi::sqlite3_auto_extension(Some(init));
    });
}

/// lantai SQLite 数据库封装
pub struct Database {
    conn: Connection,
    /// 每个 Database 实例独立的 rowid 计数器
    next_rowid: AtomicI64,
}

impl Database {
    /// 打开文件数据库
    pub fn open(path: &str) -> LantaiResult<Self> {
        register_sqlite_vec();

        let conn = Connection::open(path)
            .map_err(|e| LantaiError::Database(format!("Failed to open database: {e}")))?;

        Self::configure_and_init(conn)
    }

    /// 打开内存数据库（测试用）
    pub fn open_in_memory() -> LantaiResult<Self> {
        register_sqlite_vec();

        let conn = Connection::open_in_memory().map_err(|e| {
            LantaiError::Database(format!("Failed to open in-memory database: {e}"))
        })?;

        Self::configure_and_init(conn)
    }

    /// 获取底层连接的不可变引用
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// 获取下一个 rowid（原子递增）
    pub(crate) fn next_rowid(&self) -> i64 {
        self.next_rowid.fetch_add(1, Ordering::Relaxed)
    }

    fn configure_and_init(conn: Connection) -> LantaiResult<Self> {
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| LantaiError::Database(format!("Failed to set pragmas: {e}")))?;

        schema::initialize(&conn)?;

        // 从现有数据恢复 rowid 计数器
        let max_rowid: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(rowid_alias), 0) FROM chunks",
                [],
                |row| row.get(0),
            )
            .map_err(|e| LantaiError::Database(format!("Failed to query max rowid: {e}")))?;

        Ok(Self {
            conn,
            next_rowid: AtomicI64::new(max_rowid + 1),
        })
    }
}
