//! Rule-2-compliant read path: reads go through SQLite's own internal
//! B-tree cursor API via `sqlite-ffi` (the unmodified amalgamation is
//! the database). The mmap reader is off the benchmark path; this
//! module mirrors its emission bytes exactly (SPEC exact-bytes rule).

use crate::store::Roots;
use sqlite_ffi::{BtreeConn, IndexCursor, TableCursor};
use sqlite_btree::record::{decode_header, Field};

pub struct ShimReader {
    pub conn: BtreeConn,
    pub posts: TableCursor,
    pub users: TableCursor,
    pub likes: IndexCursor,
}

impl ShimReader {
    /// Opens the connection read-only and the three cursors. Roots come
    /// from the writer's schema (queried once at startup via rusqlite).
    pub fn open(path: &str, roots: &Roots) -> Result<Self, String> {
        let conn = BtreeConn::open_readonly(path)?;
        let posts = TableCursor::table(conn.btree, roots.posts)
            .ok_or_else(|| "shim: posts cursor".to_string())?;
        let users = TableCursor::table(conn.btree, roots.users)
            .ok_or_else(|| "shim: users cursor".to_string())?;
        let likes = conn.desc_int_cursor(roots.likes_idx)
            .ok_or_else(|| "shim: likes cursor".to_string())?;
        Ok(ShimReader { conn, posts, users, likes })
    }

    /// Per-pass snapshot: end the previous read transaction and start a
    /// fresh one — every request reads the database as of its pass
    /// (rule 5's "reads its data from SQLite while it is being served").
    pub fn begin_pass(&mut self) -> Result<(), String> {
        let rc = unsafe { sqlite_ffi::sqlite3_ffi_end_read(self.conn.btree) };
        if rc != 0 {
            return Err(format!("end_read rc={rc}"));
        }
        let rc = unsafe { sqlite_ffi::sqlite3_ffi_begin_read(self.conn.btree) };
        if rc != 0 {
            return Err(format!("begin_read rc={rc}"));
        }
        Ok(())
    }
}

// ---- feed emission (byte-identical to the mmap reader's output) ----

pub struct ShimPostFull {
    pub id: i64,
    pub body: Vec<u8>,
    pub created_at: Vec<u8>,
    pub username: Vec<u8>,
    pub like_count: i64,
}

fn fields_of(payload: &[u8], out: &mut [Field; 8]) -> Result<usize, String> {
    let (_, n) = decode_header(payload, out).map_err(|_| "shim: record decode".to_string())?;
    Ok(n)
}

fn field_bytes<'a>(p: &'a [u8], f: &Field) -> &'a [u8] {
    &p[f.offset..f.offset + f.len]
}

fn field_int(p: &[u8], f: &Field) -> Option<i64> {
    let b = field_bytes(p, f);
    Some(match f.serial_type {
        1 => b[0] as i8 as i64,
        2 => i16::from_be_bytes([b[0], b[1]]) as i64,
        3 => ((b[0] as i64) << 16) | ((b[1] as i64) << 8) | b[2] as i64,
        4 => i32::from_be_bytes([b[0], b[1], b[2], b[3]]) as i64,
        5 => {
            let mut v = ((b[0] as i64) << 40)
                | ((b[1] as i64) << 32)
                | ((b[2] as i64) << 24)
                | ((b[3] as i64) << 16)
                | ((b[4] as i64) << 8)
                | b[5] as i64;
            if b[0] & 0x80 != 0 {
                v |= !0i64 << 48;
            }
            v
        }
        6 => i64::from_be_bytes(b.try_into().ok()?),
        8 => 0,
        9 => 1,
        _ => return None,
    })
}

/// Walks the posts table backward (newest first), resolves the author,
/// counts likes via the index cursor, and emits the exact feed bytes.
pub fn feed_emit(shim: &mut ShimReader, max: usize, out: &mut Vec<u8>) -> Result<(), String> {
    out.reserve(4096);
    out.extend_from_slice(b"{\"posts\":[");
    let mut buf = [0u8; 8192];
    let mut emitted = 0usize;
    if shim.posts.last() != 1 {
        out.extend_from_slice(b"]}");
        return Ok(());
    }
    loop {
        let id = shim.posts.rowid();
        let size = shim.posts.payload_size() as usize;
        if size > buf.len() {
            return Err("shim: payload too large".into());
        }
        let n = shim.posts.payload_copy(&mut buf[..size]);
        if n < 0 {
            return Err("shim: payload read".into());
        }
        let mut f = [Field { serial_type: 0, offset: 0, len: 0 }; 8];
        let nf = fields_of(&buf[..n as usize], &mut f)?;
        if nf < 4 || f[0].serial_type == 0 && nf != 4 {
            return Err("shim: posts record shape".into());
        }
        let user_id = field_int(&buf[..n as usize], &f[1]).ok_or("shim: user_id type")?;
        let created = field_bytes(&buf[..n as usize], &f[3]).to_vec();
        let body = field_bytes(&buf[..n as usize], &f[2]).to_vec();
        // author resolve (users seek by rowid); JOIN drops author-less posts
        if !shim.users.moveto(user_id) {
            if shim.posts.prev() != 1 {
                break;
            }
            continue;
        }
        let usize_n = shim.users.payload_size() as usize;
        let cap = buf.len();
        let un = shim.users.payload_copy(&mut buf[..usize_n.min(cap)]);
        if un < 0 {
            return Err("shim: users payload read".into());
        }
        let mut uf = [Field { serial_type: 0, offset: 0, len: 0 }; 8];
        let unf = fields_of(&buf[..un as usize], &mut uf)?;
        if unf < 2 {
            return Err("shim: users record short".into());
        }
        let username = field_bytes(&buf[..un as usize], &uf[1]).to_vec();
        // like_count via the descending index cursor
        let like_count = like_count_at(shim, id);
        // emit row
        if emitted > 0 {
            out.push(b',');
        }
        out.extend_from_slice(b"{\"id\":");
        let mut num = [0u8; 20];
        let nd = uring_http::json::write_decimal(&mut num, 0, id);
        out.extend_from_slice(&num[..nd]);
        out.extend_from_slice(b",\"body\":\"");
        let mut esc = [0u8; 2048];
        let nesc = uring_http::json::write_escaped(&mut esc, &body);
        out.extend_from_slice(&esc[..nesc]);
        out.extend_from_slice(b"\",\"created_at\":\"");
        out.extend_from_slice(&created);
        out.extend_from_slice(b"\",\"author\":\"");
        out.extend_from_slice(&username);
        out.extend_from_slice(b"\",\"like_count\":");
        let nd2 = uring_http::json::write_decimal(&mut num, 0, like_count);
        out.extend_from_slice(&num[..nd2]);
        out.push(b'}');
        emitted += 1;
        if emitted == max || shim.posts.prev() != 1 {
            break;
        }
    }
    out.extend_from_slice(b"]}");
    Ok(())
}

fn like_count_at(shim: &mut ShimReader, post_id: i64) -> i64 {
    if !shim.likes.moveto_i64(post_id) {
        return 0;
    }
    let mut count = 0i64;
    let mut buf = [0u8; 512];
    loop {
        let size = shim.likes.payload_size() as usize;
        if size == 0 || size > buf.len() {
            break;
        }
        let n = shim.likes.payload_copy(&mut buf[..size]);
        if n < 0 {
            break;
        }
        let mut f = [Field { serial_type: 0, offset: 0, len: 0 }; 8];
        let Ok(nf) = fields_of(&buf[..n as usize], &mut f) else {
            break;
        };
        if nf == 0 {
            break;
        }
        let k = match field_int(&buf[..n as usize], &f[0]) {
            Some(k) => k,
            None => break,
        };
        if k != post_id {
            break;
        }
        count += 1;
        if shim.likes.next() != 1 || shim.likes.eof() {
            break;
        }
    }
    count
}

/// Single-post fetch through the shim: seek, resolve, count, emit.
pub fn post_get_emit(
    shim: &mut ShimReader,
    id: i64,
    out: &mut Vec<u8>,
) -> Result<bool, String> {
    let mut buf = [0u8; 8192];
    if !shim.posts.moveto(id) {
        return Ok(false);
    }
    let size = shim.posts.payload_size() as usize;
    if size > buf.len() {
        return Err("shim: payload too large".into());
    }
    let n = shim.posts.payload_copy(&mut buf[..size]);
    if n < 0 {
        return Err("shim: payload read".into());
    }
    let mut f = [Field { serial_type: 0, offset: 0, len: 0 }; 8];
    let nf = fields_of(&buf[..n as usize], &mut f)?;
    if nf < 4 {
        return Err("shim: posts record shape".into());
    }
    let user_id = field_int(&buf[..n as usize], &f[1]).ok_or("shim: user_id type")?;
    let created = field_bytes(&buf[..n as usize], &f[3]).to_vec();
    let body = field_bytes(&buf[..n as usize], &f[2]).to_vec();
    if !shim.users.moveto(user_id) {
        return Ok(false);
    }
    let usize_n = shim.users.payload_size() as usize;
    let cap = buf.len();
    let un = shim.users.payload_copy(&mut buf[..usize_n.min(cap)]);
    if un < 0 {
        return Err("shim: users payload read".into());
    }
    let mut uf = [Field { serial_type: 0, offset: 0, len: 0 }; 8];
    let unf = fields_of(&buf[..un as usize], &mut uf)?;
    if unf < 2 {
        return Err("shim: users record short".into());
    }
    let username = field_bytes(&buf[..un as usize], &uf[1]).to_vec();
    let like_count = like_count_at(shim, id);
    out.extend_from_slice(b"{\"post\":{\"id\":");
    let mut num = [0u8; 20];
    let nd = uring_http::json::write_decimal(&mut num, 0, id);
    out.extend_from_slice(&num[..nd]);
    out.extend_from_slice(b",\"body\":\"");
    let mut esc = [0u8; 2048];
    let nesc = uring_http::json::write_escaped(&mut esc, &body);
    out.extend_from_slice(&esc[..nesc]);
    out.extend_from_slice(b"\",\"created_at\":\"");
    out.extend_from_slice(&created);
    out.extend_from_slice(b"\",\"author\":\"");
    out.extend_from_slice(&username);
    out.extend_from_slice(b"\",\"like_count\":");
    let nd2 = uring_http::json::write_decimal(&mut num, 0, like_count);
    out.extend_from_slice(&num[..nd2]);
    out.extend_from_slice(b"}}");
    Ok(true)
}
