//! sqlite-ffi — Rust-native FFI to SQLite's **internal B-tree cursor
//! API** on the UNMODIFIED amalgamation (vendored in csrc/).
//!
//! Ruling context (arjay, 2026-11-28): the benchmark's read path must
//! go through the SQLite library's own B-tree functions — reimplementing
//! the file format in Rust is not allowed. This crate binds those
//! internal functions directly (they are exported symbols of the
//! compiled amalgamation); a ~20-line C glue (`csrc/glue.c`) covers the
//! only two accesses that need C struct internals: the `Btree*` handle
//! in `sqlite3::aDb[i].pBt`, and descending `KeyInfo` flag setup.
//!
//! Record payload DECODING stays in Rust: the cursor hands us raw
//! record bytes, and field extraction reuses the Lean-validated
//! `record`/`varint` machinery from sqlite-btree (the CL-001a fast
//! path transfers).
//!
//! # Safety
//! All B-tree calls happen inside an open read transaction held by the
//! owning [`BtreeConn`]; cursors are allocated with
//! `sqlite3BtreeCursorSize` bytes, zeroed, and bound to the btree
//! handle. Writes go through SQL (rusqlite), never through raw cursors
//! here — the contract sqlite-btree's oracle validated, preserved.

#![allow(non_camel_case_types, non_snake_case)]

use std::ffi::CString;
use std::os::raw::{c_char, c_int};

pub type Pgno = u32;
pub type BtShared = [u8; 0];
pub type BtCursor = [u8; 0];
pub type Btree = [u8; 0];
pub type KeyInfo = [u8; 0];
pub type sqlite3 = [u8; 0];

pub const SQLITE_OPEN_READONLY: c_int = 0x00000001;
pub const SQLITE_OPEN_NOMUTEX: c_int = 0x00008000;

unsafe extern "C" {
    // public API
    pub fn sqlite3_open_v2(
        filename: *const c_char,
        pp_db: *mut *mut sqlite3,
        flags: c_int,
        vfs: *const c_char,
    ) -> c_int;
    pub fn sqlite3_errmsg(db: *mut sqlite3) -> *const c_char;
    pub fn sqlite3_close_v2(db: *mut sqlite3) -> c_int;

    // internal B-tree API (exported symbols of the amalgamation)
    pub fn sqlite3BtreeCursorSize() -> c_int;
    pub fn sqlite3BtreeCursorZero(cur: *mut BtCursor);
    pub fn sqlite3BtreeCursor(
        p: *mut Btree,
        i_table: Pgno,
        wr_flag: c_int,
        p_key_info: *mut KeyInfo,
        p_cur: *mut BtCursor,
    ) -> c_int;
    pub fn sqlite3BtreeBeginTrans(p: *mut Btree, wrflag: c_int, schema_cookie: *mut u32) -> c_int;
    pub fn sqlite3BtreeCommit(p: *mut Btree) -> c_int;
    pub fn sqlite3BtreeLast(p_cur: *mut BtCursor, p_res: *mut c_int) -> c_int;
    pub fn sqlite3BtreePrevious(p_cur: *mut BtCursor, p_res: *mut c_int) -> c_int;
    pub fn sqlite3BtreeFirst(p_cur: *mut BtCursor, p_res: *mut c_int) -> c_int;
    pub fn sqlite3BtreeNext(p_cur: *mut BtCursor, flags: c_int) -> c_int;
    pub fn sqlite3BtreeEof(p_cur: *mut BtCursor) -> c_int;
    pub fn sqlite3BtreePayloadSize(p_cur: *mut BtCursor) -> u32;
    pub fn sqlite3BtreePayload(
        p_cur: *mut BtCursor,
        offset: u32,
        amt: u32,
        p_buf: *mut u8,
    ) -> c_int;
}

unsafe extern "C" {
    // glue.c — struct-internals pokes + per-operation wrappers over the
    // SQLITE_PRIVATE internals (static in the TU; only C inside can call
    // them directly, so every operation the server needs is a wrapper)
    pub fn sqlite3_ffi_btree(db: *mut sqlite3, i_db: c_int) -> *mut Btree;
    pub fn sqlite3_ffi_keyinfo_desc(db: *mut sqlite3, n_field: c_int) -> *mut KeyInfo;
    pub fn sqlite3_ffi_begin_read(bt: *mut Btree) -> c_int;
    pub fn sqlite3_ffi_end_read(bt: *mut Btree) -> c_int;
    pub fn sqlite3_ffi_cursor_new(
        bt: *mut Btree,
        root: Pgno,
        ki: *mut KeyInfo,
    ) -> *mut BtCursor;
    pub fn sqlite3_ffi_cursor_free(cur: *mut BtCursor);
    pub fn sqlite3_ffi_table_last(cur: *mut BtCursor) -> c_int;
    pub fn sqlite3_ffi_table_prev(cur: *mut BtCursor) -> c_int;
    pub fn sqlite3_ffi_table_rowid(cur: *mut BtCursor) -> i64;
    pub fn sqlite3_ffi_payload_size(cur: *mut BtCursor) -> u32;
    pub fn sqlite3_ffi_payload_copy(
        cur: *mut BtCursor,
        buf: *mut u8,
        cap: c_int,
    ) -> c_int;
    pub fn sqlite3_ffi_table_moveto(cur: *mut BtCursor, rowid: i64) -> c_int;
    pub fn sqlite3_ffi_index_first(cur: *mut BtCursor) -> c_int;
    pub fn sqlite3_ffi_index_next(cur: *mut BtCursor) -> c_int;
    pub fn sqlite3_ffi_index_eof(cur: *mut BtCursor) -> c_int;
    pub fn sqlite3_ffi_index_moveto_i64(
        cur: *mut BtCursor,
        ki: *mut KeyInfo,
        key: i64,
    ) -> c_int;
    pub fn sqlite3_ffi_keyinfo_unref(ki: *mut KeyInfo);
}

/// An opened SQLite connection with its main-schema Btree handle.
pub struct BtreeConn {
    db: *mut sqlite3,
    pub btree: *mut Btree,
}

impl BtreeConn {
    /// Opens a descending-integer-key index cursor bound to this
    /// connection (the KeyInfo comes from this connection's pool).
    pub fn desc_int_cursor(&self, root: Pgno) -> Option<IndexCursor> {
        IndexCursor::desc_int_on(self.btree, root, self.db)
    }

    /// Opens the database READ-ONLY, NOMUTEX (single-thread pass loop),
    /// extracts the main Btree handle via the glue.
    pub fn open_readonly(path: &str) -> Result<Self, String> {
        let cpath = CString::new(path).map_err(|_| "path contains NUL")?;
        let mut db: *mut sqlite3 = std::ptr::null_mut();
        let rc = unsafe {
            sqlite3_open_v2(
                cpath.as_ptr(),
                &mut db,
                SQLITE_OPEN_READONLY | SQLITE_OPEN_NOMUTEX,
                std::ptr::null(), // default unix VFS
            )
        };
        if rc != 0 {
            return Err(format!("sqlite3_open_v2 rc={rc}"));
        }
        let btree = unsafe { sqlite3_ffi_btree(db, 0) };
        if btree.is_null() {
            unsafe { sqlite3_close_v2(db) };
            return Err("btree handle extraction failed".into());
        }
        Ok(BtreeConn { db, btree })
    }
}

impl Drop for BtreeConn {
    fn drop(&mut self) {
        unsafe { sqlite3_close_v2(self.db) };
    }
}

// ---- safe wrappers (Cursor Drop frees the sqlite3_malloc'd handle) ----

/// A table cursor in rowid order (feed backward-walk, post/user seek).
pub struct TableCursor {
    cur: *mut BtCursor,
}

impl TableCursor {
    /// Opens a table cursor over `root` (no KeyInfo — rowid order).
    pub fn table(bt: *mut Btree, root: Pgno) -> Option<Self> {
        unsafe {
            let cur = sqlite3_ffi_cursor_new(bt, root, std::ptr::null_mut());
            if cur.is_null() {
                None
            } else {
                Some(TableCursor { cur })
            }
        }
    }

    /// 1 = sitting on a row, 0 = empty/exhausted, -1 = error.
    pub fn last(&self) -> i32 {
        unsafe { sqlite3_ffi_table_last(self.cur) }
    }

    pub fn prev(&self) -> i32 {
        unsafe { sqlite3_ffi_table_prev(self.cur) }
    }

    pub fn rowid(&self) -> i64 {
        unsafe { sqlite3_ffi_table_rowid(self.cur) }
    }

    pub fn payload_size(&self) -> u32 {
        unsafe { sqlite3_ffi_payload_size(self.cur) }
    }

    /// Copies the whole record payload into `buf`. Returns bytes copied
    /// or a negative error.
    pub fn payload_copy(&self, buf: &mut [u8]) -> i32 {
        unsafe { sqlite3_ffi_payload_copy(self.cur, buf.as_mut_ptr(), buf.len() as i32) }
    }

    /// Seeks to `rowid`. Returns true when sitting exactly on it.
    pub fn moveto(&self, rowid: i64) -> bool {
        unsafe { sqlite3_ffi_table_moveto(self.cur, rowid) == 0 }
    }
}

impl Drop for TableCursor {
    fn drop(&mut self) {
        unsafe { sqlite3_ffi_cursor_free(self.cur) };
    }
}

unsafe impl Send for TableCursor {}

/// An index cursor over a DESCENDING KeyInfo (single integer first field).
pub struct IndexCursor {
    cur: *mut BtCursor,
    keyinfo: *mut KeyInfo,
}

impl IndexCursor {
    /// Opens an index cursor with the descending 1-field KeyInfo.
    pub fn desc_int_on(bt: *mut Btree, root: Pgno, db: *mut sqlite3) -> Option<Self> {
        unsafe {
            let ki = sqlite3_ffi_keyinfo_desc(db, 1);
            if ki.is_null() {
                return None;
            }
            let cur = sqlite3_ffi_cursor_new(bt, root, ki);
            if cur.is_null() {
                sqlite3_ffi_keyinfo_unref(ki);
                return None;
            }
            Some(IndexCursor { cur, keyinfo: ki })
        }
    }

    pub fn first(&self) -> i32 {
        unsafe { sqlite3_ffi_index_first(self.cur) }
    }

    pub fn next(&self) -> i32 {
        unsafe { sqlite3_ffi_index_next(self.cur) }
    }

    pub fn eof(&self) -> bool {
        unsafe { sqlite3_ffi_index_eof(self.cur) != 0 }
    }

    /// Seeks to the integer first-field key. true = sitting exactly on it.
    pub fn moveto_i64(&self, key: i64) -> bool {
        unsafe { sqlite3_ffi_index_moveto_i64(self.cur, self.keyinfo, key) == 0 }
    }

    pub fn payload_size(&self) -> u32 {
        unsafe { sqlite3_ffi_payload_size(self.cur) }
    }

    pub fn payload_copy(&self, buf: &mut [u8]) -> i32 {
        unsafe { sqlite3_ffi_payload_copy(self.cur, buf.as_mut_ptr(), buf.len() as i32) }
    }
}

impl Drop for IndexCursor {
    fn drop(&mut self) {
        unsafe {
            sqlite3_ffi_cursor_free(self.cur);
            sqlite3_ffi_keyinfo_unref(self.keyinfo);
        }
    }
}

unsafe impl Send for IndexCursor {}


// ---- public SQL API (writes + health) — same connection, one copy ----

unsafe extern "C" {
    pub fn sqlite3_exec(
        db: *mut sqlite3,
        sql: *const c_char,
        callback: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, *mut *mut c_char, *mut *mut c_char) -> c_int>,
        cb_arg: *mut std::ffi::c_void,
        err_out: *mut *mut c_char,
    ) -> c_int;
    pub fn sqlite3_changes(db: *mut sqlite3) -> c_int;
    pub fn sqlite3_last_insert_rowid(db: *mut sqlite3) -> i64;
    pub fn sqlite3_free(p: *mut std::ffi::c_void);
}

impl BtreeConn {
    /// Executes one SQL statement (no results expected). Rule 6: the
    /// caller wraps writes in BEGIN/COMMIT so durability precedes the
    /// response (group-commit form).
    pub fn exec(&self, sql: &str) -> Result<(), String> {
        let csql = CString::new(sql).map_err(|_| "sql contains NUL")?;
        let mut err: *mut c_char = std::ptr::null_mut();
        let rc = unsafe { sqlite3_exec(self.db, csql.as_ptr(), None, std::ptr::null_mut(), &mut err) };
        if rc != 0 {
            let msg = if err.is_null() {
                format!("exec rc={rc}")
            } else {
                let m = unsafe {
                    std::ffi::CStr::from_ptr(err).to_string_lossy().into_owned()
                };
                unsafe { sqlite3_free(err.cast()) };
                format!("exec: {m}")
            };
            unsafe { sqlite3_free(err.cast()) };
            return Err(msg);
        }
        Ok(())
    }

    pub fn changes(&self) -> u32 {
        unsafe { sqlite3_changes(self.db) as u32 }
    }

    pub fn last_insert_rowid(&self) -> i64 {
        unsafe { sqlite3_last_insert_rowid(self.db) }
    }

    /// Single-value integer query via sqlite3_exec's callback
    /// (health SELECT 1, created_at read-back). Returns the first
    /// column of the first row as i64 when parseable.
    pub fn query_int(&self, sql: &str) -> Result<Option<i64>, String> {
        let csql = CString::new(sql).map_err(|_| "sql contains NUL")?;
        let mut err: *mut c_char = std::ptr::null_mut();
        struct Sink(std::cell::Cell<Option<i64>>);
        unsafe extern "C" fn cb(sink: *mut std::ffi::c_void, _n: c_int, vals: *mut *mut c_char, _cols: *mut *mut c_char) -> c_int {
            let sink = unsafe { &*(sink as *const Sink) };
            let first = unsafe { *vals.add(0) };
            if first.is_null() {
                return 0;
            }
            let text = unsafe { std::ffi::CStr::from_ptr(first) };
            if let Ok(t) = text.to_str() {
                if let Ok(v) = t.parse::<i64>() {
                    sink.0.set(Some(v));
                }
            }
            0
        }
        let sink = Sink(std::cell::Cell::new(None));
        let rc = unsafe {
            sqlite3_exec(
                self.db,
                csql.as_ptr(),
                Some(cb),
                &sink as *const Sink as *mut std::ffi::c_void,
                &mut err,
            )
        };
        if rc != 0 {
            return Err(format!("query_int rc={rc}"));
        }
        Ok(sink.0.get())
    }
}

#[cfg(test)]
mod probe;
