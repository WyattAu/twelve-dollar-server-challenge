//! Standalone probe: open the calib seed read-only, walk the posts
//! table via the FFI wrappers, and report cursor states. Run directly
//! (no cargo test needed): the binary is tests/probe or the lib's
//! doctest... simplest: run under `cargo run --release` from the
//! workspace of a tiny bin — instead: `cargo test -p sqlite-ffi
//! --release probe -- --nocapture` with the seed env set.

#[cfg(test)]
mod tests {
    #[test]
    fn probe_posts_walk() {
        use crate::{BtreeConn, TableCursor};
        let db = std::env::var("RANKING_DB")
            .unwrap_or_else(|_| "/home/wyatt/.cache/calib/seed.db".to_string());
        let Ok(conn) = BtreeConn::open_readonly(&db) else {
            eprintln!("probe: cannot open {db}");
            return;
        };
        unsafe {
            let rc = crate::sqlite3_ffi_begin_read(conn.btree);
            eprintln!("begin_read rc={rc}");
        }
        let Some(mut posts) = TableCursor::table(conn.btree, 3) else {
            eprintln!("probe: posts cursor failed");
            return;
        };
        let last = posts.last();
        eprintln!("posts.last() = {last} (1=on-row, 0=exhausted, -1=error)");
        if last == 1 {
            eprintln!("posts.rowid() = {}", posts.rowid());
            eprintln!("posts.payload_size() = {}", posts.payload_size());
        }
        // count a short backward walk
        let mut n = 0;
        while n < 5 && posts.prev() == 1 {
            n += 1;
            eprintln!("  prev[{}] rowid={}", n, posts.rowid());
        }
    }
}
