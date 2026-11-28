#include "sqlite3.c"

// ---- Rust-facing wrapper layer. The internal B-tree functions are
// SQLITE_PRIVATE (static) — only this translation unit can call them,
// so each operation the server needs gets a non-static wrapper here.
// The amalgamation source itself is unmodified. ----

void *sqlite3_ffi_btree(sqlite3 *db, int iDb) {
    if (db == 0 || iDb < 0 || iDb >= db->nDb) return 0;
    return db->aDb[iDb].pBt;
}

void sqlite3_ffi_keyinfo_unref(KeyInfo *ki) {
    sqlite3KeyInfoUnref(ki);
}

KeyInfo *sqlite3_ffi_keyinfo_desc(sqlite3 *db, int nField) {
    KeyInfo *k = sqlite3KeyInfoAlloc(db, nField, 0);
    if (k == 0) return 0;
    for (int i = 0; i < nField; i++) {
        k->aSortFlags[i] = KEYINFO_ORDER_DESC;
    }
    return k;
}

int sqlite3_ffi_begin_read(Btree *bt) {
    u32 cookie = 0;
    return sqlite3BtreeBeginTrans(bt, 0, &cookie);
}

int sqlite3_ffi_end_read(Btree *bt) {
    return sqlite3BtreeCommit(bt);
}

BtCursor *sqlite3_ffi_cursor_new(Btree *bt, Pgno root, KeyInfo *ki) {
    int sz = sqlite3BtreeCursorSize();
    BtCursor *cur = sqlite3_malloc(sz);
    if (cur == 0) return 0;
    sqlite3BtreeCursorZero(cur);
    if (sqlite3BtreeCursor(bt, root, 0, ki, cur) != SQLITE_OK) {
        sqlite3_free(cur);
        return 0;
    }
    return cur;
}

void sqlite3_ffi_cursor_free(BtCursor *cur) {
    if (cur) sqlite3_free(cur);
}

int sqlite3_ffi_table_last(BtCursor *cur) {
    int res = 0;
    if (sqlite3BtreeLast(cur, &res) != SQLITE_OK) return -1;
    return res == 0 ? 1 : 0; /* 1 = on a row (handler's Moved convention) */
}

int sqlite3_ffi_table_prev(BtCursor *cur) {
    int res = 0;
    if (sqlite3BtreePrevious(cur, 0) != SQLITE_OK) return -1;
    return res == 0 ? 1 : 0;
}

i64 sqlite3_ffi_table_rowid(BtCursor *cur) {
    return sqlite3BtreeIntegerKey(cur);
}

u32 sqlite3_ffi_payload_size(BtCursor *cur) {
    return sqlite3BtreePayloadSize(cur);
}

int sqlite3_ffi_payload_copy(BtCursor *cur, u8 *buf, int cap) {
    u32 amt = sqlite3BtreePayloadSize(cur);
    if (amt > (u32)cap) return SQLITE_TOOBIG;
    if (sqlite3BtreePayload(cur, 0, amt, buf) != SQLITE_OK) return SQLITE_CORRUPT;
    return (int)amt;
}

int sqlite3_ffi_table_moveto(BtCursor *cur, i64 rowid) {
    int res = 0;
    return sqlite3BtreeTableMoveto(cur, rowid, 0, &res) == SQLITE_OK ? res : -1;
}

int sqlite3_ffi_index_first(BtCursor *cur) {
    int res = 0;
    if (sqlite3BtreeFirst(cur, &res) != SQLITE_OK) return -1;
    return res == 0 ? 1 : 0;
}

int sqlite3_ffi_index_next(BtCursor *cur) {
    int res = 0;
    if (sqlite3BtreeNext(cur, 0) != SQLITE_OK) return -1;
    return res == 0 ? 1 : 0;
}

int sqlite3_ffi_index_eof(BtCursor *cur) {
    return sqlite3BtreeEof(cur);
}

// Seeks the descending-KeyInfo index cursor to the integer key.
// 0 = exact, 1 = landed past it, -1 = error.
int sqlite3_ffi_index_moveto_i64(BtCursor *cur, KeyInfo *ki, i64 key) {
    UnpackedRecord u;
    Mem mem;
    int res = 0;
    memset(&u, 0, sizeof(u));
    memset(&mem, 0, sizeof(mem));
    sqlite3VdbeMemSetInt64(&mem, key);
    u.pKeyInfo = ki;
    u.aMem = &mem;
    u.u.i = key;
    u.n = 8;
    u.nField = 1;
    u.default_rc = -1;
    return sqlite3BtreeIndexMoveto(cur, &u, &res);
}
