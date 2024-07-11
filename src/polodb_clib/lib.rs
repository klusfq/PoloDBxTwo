#![allow(clippy::missing_safety_doc)]

use polodb_core::{DbContext, DbErr, DbHandle, TransactionType, Config};
use polodb_bson::{ObjectId, Document, Array, Value};
use polodb_bson::ty_int::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::os::raw::{c_char, c_uint, c_int, c_double, c_longlong};
use std::ptr::{null_mut, write_bytes, null};
use std::ffi::{CStr, CString};
use std::borrow::Borrow;

const DB_ERROR_MSG_SIZE: usize = 512;

thread_local! {
    static DB_GLOBAL_ERROR: RefCell<Option<DbErr>> = RefCell::new(None);
    static DB_GLOBAL_ERROR_MSG: RefCell<[c_char; DB_ERROR_MSG_SIZE]> = RefCell::new([0; DB_ERROR_MSG_SIZE]);
}

#[repr(C)]
pub union ValueUnion {
    int_value: i64,
    double_value: c_double,
    bool_value: c_int,
    str: *mut c_char,
    oid: *mut ObjectId,
    arr: *mut Rc<Array>,
    doc: *mut Rc<Document>,
    bin: *mut Rc<Vec<u8>>,
    utc: u64,
}

#[repr(C)]
pub struct ValueMock {
    tag:   u8,
    value: ValueUnion,
}

macro_rules! try_read_utf8 {
    ($action:expr, $ret:expr) => {
        match $action {
            Ok(str) => str,
            Err(err) => {
                set_global_error(err.into());
                return $ret;
            }
        }
    }
}

fn set_global_error(err: DbErr) {
    DB_GLOBAL_ERROR.with(|f| {
        *f.borrow_mut() = Some(err);
    });
}

#[no_mangle]
pub unsafe extern "C" fn PLDB_open(path: *const c_char) -> *mut DbContext {
    let cstr = CStr::from_ptr(path);
    let str = try_read_utf8!(cstr.to_str(), null_mut());
    let db = match DbContext::open_file(str.as_ref(), Config::default()) {
        Ok(db) => db,
        Err(err) => {
            set_global_error(err);
            return null_mut();
        }
    };
    let ptr = Box::new(db);
    Box::into_raw(ptr)
}

#[no_mangle]
pub unsafe extern "C" fn PLDB_start_transaction(db: *mut DbContext, flags: c_int) -> c_int {
    let rust_db = db.as_mut().unwrap();
    let ty = match flags {
        0 => None,
        1 => Some(TransactionType::Read),
        2 => Some(TransactionType::Write),
        _ => {
            set_global_error(DbErr::UnknownTransactionType);
            return PLDB_error_code();
        }
    };
    match rust_db.start_transaction(ty) {
        Ok(()) => 0,
        Err(err) => {
            set_global_error(err);
            PLDB_error_code()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn PLDB_rollback(db: *mut DbContext) -> c_int {
    let rust_db = db.as_mut().unwrap();
    match rust_db.rollback() {
        Ok(()) => 0,
        Err(err) => {
            set_global_error(err);
            PLDB_error_code()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn PLDB_commit(db: *mut DbContext) -> c_int {
    let rust_db = db.as_mut().unwrap();
    match rust_db.commit() {
        Ok(()) => 0,
        Err(err) => {
            set_global_error(err);
            PLDB_error_code()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn PLDB_count(db: *mut DbContext, col_id: c_uint, meta_version: u32) -> c_longlong {
    let rust_db = db.as_mut().unwrap();
    let result = rust_db.count(col_id, meta_version);
    match result {
        Ok(result) => {
            result as c_longlong
        }
        Err(err) => {
            set_global_error(err);
            PLDB_error_code() as c_longlong
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn PLDB_create_collection(db: *mut DbContext,
                                         name: *const c_char,
                                         col_id: *mut c_uint,
                                         meta_version: *mut c_uint) -> c_int {
    let name_str= CStr::from_ptr(name);
    let name_utf8 = try_read_utf8!(name_str.to_str(), PLDB_error_code());
    let oid_result = db.as_mut().unwrap().create_collection(name_utf8);
    match oid_result {
        Ok(meta) => {
            col_id.write(meta.id);
            meta_version.write(meta.meta_version);
            0
        }

        Err(err) => {
            set_global_error(err);
            PLDB_error_code()
        }

    }
}

#[no_mangle]
pub unsafe extern "C" fn PLDB_drop(db: *mut DbContext, col_id: c_uint, meta_version: c_uint) -> c_int {
    let result = db.as_mut().unwrap().drop_collection(col_id, meta_version);
    if let Err(err) = result {
        set_global_error(err);
        return PLDB_error_code();
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn PLDB_get_collection_meta_by_name(db: *mut DbContext, name: *const c_char, id: *mut c_uint, version: *mut c_uint) -> c_int {
    let str = CStr::from_ptr(name);
    let utf8str = try_read_utf8!(str.to_str(), PLDB_error_code());
    let result = db.as_mut().unwrap().get_collection_meta_by_name(utf8str);
    match result {
        Ok(info) => {
            id.write(info.id);
            version.write(info.meta_version);
            0
        }

        Err(err) => {
            set_global_error(err);
            PLDB_error_code()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn PLDB_insert(db: *mut DbContext, col_id: c_uint, meta_version: c_uint, doc: *mut Rc<Document>) -> c_int {
    let local_db = db.as_mut().unwrap();
    let local_doc = doc.as_mut().unwrap();
    let mut_doc = Rc::make_mut(local_doc);
    let insert_result = local_db.insert(col_id, meta_version, mut_doc);
    if let Err(err) = insert_result {
        set_global_error(err);
        return PLDB_error_code();
    }
    match insert_result {
        Ok(true) => 1,
        Ok(false) => 0,
        Err(err) => {
            set_global_error(err);
            PLDB_error_code()
        }
    }
}

/// query is nullable
#[no_mangle]
pub unsafe extern "C" fn PLDB_find(db: *mut DbContext,
                            col_id: c_uint,
                            meta_version: c_uint,
                            query: *const Rc<Document>,
                            out_handle: *mut *mut DbHandle) -> c_int {
    let rust_db = db.as_mut().unwrap();

    let handle_result = match query.as_ref() {
        Some(query_doc) => rust_db.find(col_id, meta_version, Some(query_doc.borrow())),
        None => rust_db.find(col_id, meta_version, None),
    };

    let handle = match handle_result {
        Ok(handle) => handle,
        Err(err) => {
            set_global_error(err);
            return PLDB_error_code();
        }
    };

    let boxed_handle = Box::new(handle);
    let raw_handle = Box::into_raw(boxed_handle);

    out_handle.write(raw_handle);

    0
}

/// query is nullable
#[no_mangle]
pub unsafe extern "C" fn PLDB_update(db: *mut DbContext,
                              col_id: c_uint,
                              meta_version: c_uint,
                              query: *const Rc<Document>,
                              update: *const Rc<Document>) -> c_longlong {
    let result = {
        let rust_db = db.as_mut().unwrap();

        let update_doc = update.as_ref().unwrap();

        match query.as_ref() {
            Some(query) => rust_db.update(col_id, meta_version, Some(query.as_ref()), update_doc),
            None => rust_db.update(col_id, meta_version, None, update_doc),
        }
    };

    match result {
        Ok(result) => result as c_longlong,
        Err(err) => {
            set_global_error(err);
            PLDB_error_code() as c_longlong
        }
    }
}

/// return value represents how many rows are deleted
#[no_mangle]
pub unsafe extern "C" fn PLDB_delete(db: *mut DbContext, col_id: c_uint, meta_version: c_uint, query: *const Rc<Document>) -> c_longlong {
    let rust_db = db.as_mut().unwrap();
    let query_doc = query.as_ref().unwrap();
    let result = rust_db.delete(col_id, meta_version, query_doc.as_ref());

    match result {
        Ok(size) => size as c_longlong,
        Err(err) => {
            set_global_error(err);
            PLDB_error_code() as c_longlong
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn PLDB_delete_all(db: *mut DbContext, col_id: c_uint, meta_version: c_uint) -> c_longlong {
    let result = {
        let rust_db = db.as_mut().unwrap();
        rust_db.delete_all(col_id, meta_version)
    };

    match result {
        Ok(size) => size as c_longlong,
        Err(err) => {
            set_global_error(err);
            PLDB_error_code() as c_longlong
        }
    }
}

#[no_mangle]
pub extern "C" fn PLDB_error_code() -> c_int {
    DB_GLOBAL_ERROR.with(|f| {
        if let Some(err) = f.borrow().as_ref() {
            let code = error_code_of_db_err(err) * -1;
            return code
        }
        0
    })
}

#[no_mangle]
pub unsafe extern "C" fn PLDB_error_msg() -> *const c_char {
    DB_GLOBAL_ERROR.with(|f| {
        if let Some(err) = f.borrow_mut().as_ref() {
            return DB_GLOBAL_ERROR_MSG.with(|msg| {
                write_bytes(msg.borrow_mut().as_mut_ptr(), 0, DB_ERROR_MSG_SIZE);
                let err_msg = err.to_string();
                let str_size = err_msg.len();
                let err_cstring = CString::new(err_msg).unwrap();
                let expected_size: usize = std::cmp::min(str_size, DB_ERROR_MSG_SIZE - 1);
                err_cstring.as_ptr().copy_to(msg.borrow_mut().as_mut_ptr(), expected_size);

                msg.borrow().as_ptr()
            });
        }

        null()
    })
}

#[no_mangle]
pub unsafe extern "C" fn PLDB_version(buffer: *mut c_char, buffer_size: c_uint) -> c_uint {
    let version_str = DbContext::get_version();
    let str_size = version_str.len();
    let cstring = CString::new(version_str).unwrap();
    let c_ptr = cstring.as_ptr();
    let expected_size: usize = std::cmp::min(str_size, buffer_size as usize);
    c_ptr.copy_to(buffer, expected_size);
    expected_size as c_uint
}

#[no_mangle]
pub unsafe extern "C" fn PLDB_close(db: *mut DbContext) {
    let _ptr = Box::from_raw(db);
}

#[no_mangle]
pub unsafe extern "C" fn PLDB_mk_doc() -> *mut Rc<Document> {
    let doc = Rc::new(Document::new_without_id());
    let rdoc = Box::new(doc);
    Box::into_raw(rdoc)
}

#[no_mangle]
pub unsafe extern "C" fn PLDB_free_doc(doc: *mut Rc<Document>) {
    let _ptr = Box::from_raw(doc);
}

#[no_mangle]
pub unsafe extern "C" fn PLDB_doc_set(doc: *mut Rc<Document>, key: *const c_char, val: ValueMock) -> c_uint {
    let rdoc = doc.as_mut().unwrap();

    let ckey = CStr::from_ptr(key);
    let rkey = ckey.to_str().unwrap();

    // debug_mem(&val);
    let v = value_parse(&val);

    let _ = Rc::get_mut(rdoc).unwrap().insert(rkey.to_string(), v);

    // TODO: error handle
    0
}

#[no_mangle]
pub unsafe extern "C" fn PLDB_doc_get(doc: *mut Rc<Document>, key: *const c_char, out_val: *mut ValueMock) -> c_uint {
    println!("call doc_get");
    let rdoc = doc.as_ref().unwrap();

    let rkey = CStr::from_ptr(key).to_str().unwrap();

    println!("call doc_get({}) from: {:p}", rkey, out_val);
    let rval = rdoc.get(rkey).unwrap();

    out_val.write(value_build(rval));

    println!("out val: {:p}", out_val);

    // TODO: error handle
    0
}

#[no_mangle]
pub unsafe extern "C" fn PLDB_doc_len(doc: *mut Rc<Document>) -> c_uint {
    let rdoc = doc.as_ref().unwrap();
    
    rdoc.len() as c_uint
}

unsafe fn value_build(val: &Value) -> ValueMock {
    let mut v_inner = ValueUnion {
        int_value: 0,
    };
    match val.ty_int() {
        NULL => v_inner.int_value = 0,
        DOUBLE => v_inner.double_value = val.unwrap_double(),
        BOOLEAN => {
            v_inner.bool_value = val.unwrap_boolean().into();
        },
        INT => v_inner.int_value = val.unwrap_int(),
        STRING => v_inner.str = CString::new(val.unwrap_string().to_owned()).unwrap().into_raw(),
        OBJECT_ID => {
            let poid = val.unwrap_objectid();
            v_inner.oid = Box::into_raw(Box::new(poid.as_ref().clone()));
        },
        ARRAY => {
            let parr = val.unwrap_array();
            v_inner.arr = Box::into_raw(Box::new(parr.clone()));
        },
        DOCUMENT => {
            let pdoc = val.unwrap_document();
            v_inner.doc = Box::into_raw(Box::new(pdoc.clone()));
        },
        BINARY => {
            let pbin = val.unwrap_binary();
            v_inner.bin = Box::into_raw(Box::new(pbin.clone()));
        },
        UTC_DATETIME => {
            let putc = val.unwrap_utc();
            v_inner.utc = putc.as_ref().timestamp();
        },
        _ => {
            v_inner.int_value = 0;
        }
    }

    ValueMock {
        tag: val.ty_int(),
        value: v_inner,
    }
}

unsafe fn value_parse(vmock: &ValueMock) -> Value {
    let val = &(*vmock).value;
    let tag = (*vmock).tag;
    match tag {
        NULL => Value::Null,
        DOUBLE => Value::from(val.double_value),
        BOOLEAN => Value::from(val.bool_value),
        INT => Value::from(val.int_value),
        STRING => {
            let cvstr = CStr::from_ptr(val.str);
            let vstr = try_read_utf8!(cvstr.to_str(), Value::Null);
            Value::from(vstr)
        },
        OBJECT_ID => {
            let oid = Box::from_raw(val.oid as *mut ObjectId);
            Value::from(*oid)
        },
        ARRAY => {
            let arr = Rc::clone(Box::from_raw(val.arr as *mut Rc<Array>).as_ref());
            Value::from(arr.as_ref().clone())
        },
        DOCUMENT => {
            let doc = Rc::clone(Box::from_raw(val.doc as *mut Rc<Document>).as_ref());
            Value::from(doc.as_ref().clone())
        },
        BINARY => {
            let bin = Rc::clone(Box::from_raw(val.bin as *mut Rc<Vec<u8>>).as_ref());
            Value::from(bin.as_ref().clone())
        },
        UTC_DATETIME => Value::from(val.utc),

        _ => Value::Null,
    }
}


fn debug_mem(s: &ValueMock) {
    // 获取结构体的字节表示
    let bytes: &[u8] = unsafe {
        std::slice::from_raw_parts(
            s as *const ValueMock as *const u8,
            std::mem::size_of::<ValueMock>()
        )
    };

    // 打印字节流
    print!("Bytes:");
    for (_, byte) in bytes.iter().enumerate() {
        print!(" {:#02x}", byte);
    }
    println!();
}

fn error_code_of_db_err(err: &DbErr) -> i32 {
    match err {
        DbErr::UnexpectedIdType(_, _) => 1,
        DbErr::NotAValidKeyType(_) => 2,
        DbErr::ValidationError(_) => 3,
        DbErr::InvalidOrderOfIndex(_) => 4,
        DbErr::IndexAlreadyExists(_) => 5,
        DbErr::FieldTypeUnexpected(_) => 6,
        DbErr::ParseError(_) => 7,
        DbErr::IOErr(_) => 9,
        DbErr::UTF8Err(_) => 10,
        DbErr::DataSizeTooLarge(_, _) => 12,
        DbErr::DecodeEOF => 13,
        DbErr::BsonErr(_) => 14,
        DbErr::DataOverflow => 15,
        DbErr::DataExist(_) => 16,
        DbErr::PageSpaceNotEnough => 17,
        DbErr::DataHasNoPrimaryKey => 18,
        DbErr::ChecksumMismatch => 19,
        DbErr::JournalPageSizeMismatch(_, _) => 20,
        DbErr::SaltMismatch => 21,
        DbErr::PageMagicMismatch(_) => 22,
        DbErr::ItemSizeGreaterThanExpected => 23,
        DbErr::CollectionNotFound(_) => 24,
        DbErr::CollectionIdNotFound(_) => 25,
        DbErr::MetaPageIdError => 26,
        DbErr::CannotWriteDbWithoutTransaction => 27,
        DbErr::StartTransactionInAnotherTransaction => 28,
        DbErr::RollbackNotInTransaction => 29,
        DbErr::IllegalCollectionName(_) => 30,
        DbErr::UnexpectedHeaderForBtreePage(_) => 31,
        DbErr::KeyTypeOfBtreeShouldNotBeZero => 32,
        DbErr::UnexpectedPageHeader => 33,
        DbErr::UnexpectedPageType => 34,
        DbErr::UnknownTransactionType => 35,
        DbErr::BufferNotEnough(_) => 36,
        DbErr::UnknownUpdateOperation(_) => 37,
        DbErr::IncrementNullField => 38,
        DbErr::VmIsHalt => 39,
        DbErr::MetaVersionMismatched(_, _) => 40,
        DbErr::Busy => 41,
        DbErr::InvalidField(_) => 42,
        DbErr::CollectionAlreadyExits(_) => 43,
        DbErr::UnableToUpdatePrimaryKey => 44,
        DbErr::UnexpectedTypeForOp(_) => 45,
        DbErr::NotAValidDatabase => 46,
        DbErr::DatabaseOccupied => 47,
        DbErr::Multiple(_) => 48,
        DbErr::VersionMismatch(_) => 49,
        DbErr::EnumError(_) => 50,
    }
}
