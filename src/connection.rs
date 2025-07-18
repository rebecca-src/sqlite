use core::ffi::{c_char, c_int, c_void};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::path::Path;

use crate::error::Result;
use crate::statement::Statement;

/// A connection.
pub struct Connection {
    raw: Raw,
    busy_callback: Option<Box<dyn FnMut(usize) -> bool + Send>>,
    phantom: PhantomData<ffi::sqlite3>,
}

/// A thread-safe connection.
pub struct ConnectionThreadSafe(Connection);

/// Flags for opening a connection.
#[derive(Clone, Copy, Debug)]
pub struct OpenFlags(c_int);

struct Raw(*mut ffi::sqlite3);

impl Connection {
    /// Open a read-write connection to a new or existing database.
    pub fn open<T: AsRef<Path>>(path: T) -> Result<Connection> {
        Connection::open_with_flags(path, OpenFlags::new().with_create().with_read_write())
    }

    /// Open a connection with specific flags.
    pub fn open_with_flags<T: AsRef<Path>>(path: T, flags: OpenFlags) -> Result<Connection> {
        let mut raw = std::ptr::null_mut();
        unsafe {
            let code = ffi::sqlite3_open_v2(
                path_to_cstr!(path.as_ref()).as_ptr(),
                &mut raw,
                flags.0,
                std::ptr::null(),
            );
            match code {
                ffi::SQLITE_OK => {}
                code => match crate::error::last(raw) {
                    Some(error) => {
                        ffi::sqlite3_close(raw);
                        return Err(error);
                    }
                    _ => {
                        ffi::sqlite3_close(raw);
                        return Err(crate::error::Error {
                            code: Some(code as isize),
                            message: None,
                        });
                    }
                },
            }
        }
        Ok(Connection {
            raw: Raw(raw),
            busy_callback: None,
            phantom: PhantomData,
        })
    }

    /// Open a thread-safe read-write connection to a new or existing database.
    pub fn open_thread_safe<T: AsRef<Path>>(path: T) -> Result<ConnectionThreadSafe> {
        Connection::open_with_flags(
            path,
            OpenFlags::new()
                .with_create()
                .with_read_write()
                .with_full_mutex(),
        )
        .map(ConnectionThreadSafe)
    }

    /// Open a thread-safe connection with specific flags.
    pub fn open_thread_safe_with_flags<T: AsRef<Path>>(
        path: T,
        flags: OpenFlags,
    ) -> Result<ConnectionThreadSafe> {
        Connection::open_with_flags(path, flags.with_full_mutex()).map(ConnectionThreadSafe)
    }

    #[doc(hidden)]
    #[inline]
    pub fn as_raw(&self) -> *mut ffi::sqlite3 {
        self.raw.0
    }

    #[inline]
    pub fn execute_with<T: AsRef<str>>(
        &self,
        statement: T,
        fields: std::slice::Iter<'_, crate::Value>,
    ) -> Result<()> {
        let mut cursor = self.prepare(statement)?;

        let mut idx = 1;
        for val in fields {
            cursor.bind((idx, val))?;
            idx += 1;
        }

        cursor.next()?;
        Ok(())
    }

    #[inline]
    pub fn execute_many<T: AsRef<str>>(
        &self,
        statement: T,
        rows: &Vec<Vec<crate::Value>>,
    ) -> Result<()> {
        let mut cursor = self.prepare(statement)?;

        for fields in rows.iter() {
            cursor.reset()?;
            let mut idx = 1;
            for val in fields {
                cursor.bind((idx, val))?;
                idx += 1;
            }
            cursor.next()?;
        }
        Ok(())
    }

    #[inline]
    pub fn has_table(&self, tbl_name: &str) -> Result<bool> {
        let statement = format!(
            "SELECT COUNT(*) from sqlite_master where tbl_name = '{}'",
            tbl_name
        );
        let mut cursor = self.prepare(&statement)?;
        cursor.next()?;
        let count = cursor.read::<i64, _>(0)?;
        Ok(count > 0)
    }

    #[inline]
    pub fn has_column(&self, tbl_name: &str, column_name: &str) -> Result<bool> {
        let statement = format!(
            "SELECT COUNT(*) FROM PRAGMA_TABLE_INFO('{}') WHERE NAME='{}'",
            tbl_name, column_name
        );
        let mut cursor = self.prepare(&statement)?;
        cursor.next()?;
        let count = cursor.read::<i64, _>(0)?;
        Ok(count > 0)
    }

    #[inline]
    pub fn has_value(
        &self,
        tbl_name: &str,
        column_name: &str,
        value: &crate::Value,
    ) -> Result<bool> {
        let statement = format!(
            "SELECT COUNT(*) FROM \"{}\" WHERE \"{}\" = ?",
            tbl_name, column_name
        );
        let mut cursor = self.prepare(&statement)?;
        cursor.bind((1, value))?;
        cursor.next()?;
        let count = cursor.read::<i64, _>(0)?;
        Ok(count > 0)
    }
}

impl Connection {
    /// Execute a statement without processing the resulting rows if any.
    #[inline]
    pub fn execute<T: AsRef<str>>(&self, statement: T) -> Result<()> {
        unsafe {
            ok!(
                self.raw.0,
                ffi::sqlite3_exec(
                    self.raw.0,
                    str_to_cstr!(statement.as_ref()).as_ptr(),
                    None,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            );
        }
        Ok(())
    }

    /// Execute a statement and process the resulting rows as plain text.
    ///
    /// The callback is triggered for each row. If the callback returns `false`, no more rows will
    /// be processed. For large queries and non-string data types, prepared statement are highly
    /// preferable; see `prepare`.
    #[inline]
    pub fn iterate<T: AsRef<str>, F>(&self, statement: T, callback: F) -> Result<()>
    where
        F: FnMut(&[(&str, Option<&str>)]) -> bool,
    {
        unsafe {
            let callback = Box::new(callback);
            ok!(
                self.raw.0,
                ffi::sqlite3_exec(
                    self.raw.0,
                    str_to_cstr!(statement.as_ref()).as_ptr(),
                    Some(process_callback::<F>),
                    &*callback as *const F as *mut F as *mut _,
                    std::ptr::null_mut(),
                )
            );
        }
        Ok(())
    }

    /// Create a prepared statement.
    #[inline]
    pub fn prepare<T: AsRef<str>>(&self, statement: T) -> Result<Statement<'_>> {
        crate::statement::new(self.raw.0, statement)
    }

    /// Return the number of rows inserted, updated, or deleted by the most recent INSERT, UPDATE,
    /// or DELETE statement.
    #[inline]
    pub fn change_count(&self) -> usize {
        unsafe { ffi::sqlite3_changes(self.raw.0) as usize }
    }

    /// Return the total number of rows inserted, updated, and deleted by all INSERT, UPDATE, and
    /// DELETE statements since the connection was opened.
    #[inline]
    pub fn total_change_count(&self) -> usize {
        unsafe { ffi::sqlite3_total_changes(self.raw.0) as usize }
    }
}

impl Connection {
    /// Set a callback for handling busy events.
    ///
    /// The callback is triggered when the database cannot perform an operation due to processing
    /// of some other request. If the callback returns `true`, the operation will be repeated.
    pub fn set_busy_handler<F>(&mut self, callback: F) -> Result<()>
    where
        F: FnMut(usize) -> bool + Send + 'static,
    {
        self.remove_busy_handler()?;
        unsafe {
            let callback = Box::new(callback);
            let result = ffi::sqlite3_busy_handler(
                self.raw.0,
                Some(busy_callback::<F>),
                &*callback as *const F as *mut F as *mut _,
            );
            self.busy_callback = Some(callback);
            ok!(self.raw.0, result);
        }
        Ok(())
    }

    /// Set an implicit callback for handling busy events that tries to repeat rejected operations
    /// until a timeout expires.
    #[inline]
    pub fn set_busy_timeout(&mut self, milliseconds: usize) -> Result<()> {
        unsafe {
            ok!(
                self.raw.0,
                ffi::sqlite3_busy_timeout(self.raw.0, milliseconds as c_int)
            );
        }
        Ok(())
    }

    /// Remove the callback handling busy events.
    #[inline]
    pub fn remove_busy_handler(&mut self) -> Result<()> {
        self.busy_callback = None;
        unsafe {
            ok!(
                self.raw.0,
                ffi::sqlite3_busy_handler(self.raw.0, None, std::ptr::null_mut())
            );
        }
        Ok(())
    }
}

impl Connection {
    /// Enable loading extensions.
    #[cfg(feature = "extension")]
    #[inline]
    pub fn enable_extension(&self) -> Result<()> {
        unsafe {
            ok!(
                self.raw.0,
                ffi::sqlite3_enable_load_extension(self.raw.0, 1 as c_int)
            );
        }
        Ok(())
    }

    /// Disable loading extensions.
    #[cfg(feature = "extension")]
    #[inline]
    pub fn disable_extension(&self) -> Result<()> {
        unsafe {
            ok!(
                self.raw.0,
                ffi::sqlite3_enable_load_extension(self.raw.0, 0 as c_int)
            );
        }
        Ok(())
    }

    /// Load an extension.
    #[cfg(feature = "extension")]
    #[inline]
    pub fn load_extension<T: AsRef<str>>(&self, name: T) -> Result<()> {
        unsafe {
            ok!(
                self.raw.0,
                ffi::sqlite3_load_extension(
                    self.raw.0,
                    str_to_cstr!(name.as_ref()).as_ptr() as *const c_char,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            );
        }
        Ok(())
    }

    /// Serializes the SQLite database to an in-memory buffer.
    /// This function allows you to serialize an SQLite database to an in-memory buffer.
    /// The output is the same bytes as if the database were to be written to disk.
    #[cfg(feature = "serialize")]
    #[inline]
    pub async fn serialize_to_buffer(&mut self) -> Result<Vec<u8>> {
        let mut res = Vec::new();
        unsafe {
            let mut size: ffi::sqlite3_int64 = 0;
            let data_ptr =
                ffi::sqlite3_serialize(self.raw.0, std::ptr::null(), &mut size as *mut _, 0);

            if data_ptr == std::ptr::null_mut() {
                raise!("Could not create database buffer!");
            }

            for _ in 0..size {
                res.push(data_ptr.read() as u8)
            }
        }
        Ok(res)
    }

    #[cfg(feature = "serialize")]
    #[inline]
    pub fn deserialize_to_readonly_db(&self, data: &[u8]) -> Result<()> {
        unsafe {
            ok!(
                self.raw.0,
                ffi::sqlite3_deserialize(
                    self.raw.0,
                    std::ptr::null(),
                    data.as_ptr() as *mut u8,
                    data.len() as i64,
                    data.len() as i64,
                    ffi::SQLITE_DESERIALIZE_READONLY as u32
                )
            );
        }
        Ok(())
    }
}

impl Connection {
    /// Set the encryption key.
    #[cfg(feature = "encryption")]
    #[inline]
    pub fn set_encryption_key<T: AsRef<str>>(&self, key: T) -> Result<()> {
        unsafe {
            ok!(
                self.raw.0,
                ffi::sqlite3_key_v2(
                    self.raw.0,
                    std::ptr::null() as *const c_char,
                    str_to_cstr!(key.as_ref()).as_ptr() as *const c_void,
                    key.as_ref().len() as c_int,
                )
            );
        }
        Ok(())
    }

    /// Change the encryption key.
    #[cfg(feature = "encryption")]
    #[inline]
    pub fn change_encryption_key<T: AsRef<str>>(&self, new_key: T) -> Result<()> {
        unsafe {
            ok!(
                self.raw.0,
                ffi::sqlite3_rekey_v2(
                    self.raw.0,
                    std::ptr::null() as *const c_char,
                    str_to_cstr!(new_key.as_ref()).as_ptr() as *const c_void,
                    new_key.as_ref().len() as c_int,
                )
            );
        }
        Ok(())
    }
}

impl Drop for Connection {
    #[inline]
    #[allow(unused_must_use)]
    fn drop(&mut self) {
        self.remove_busy_handler();
        unsafe { ffi::sqlite3_close(self.raw.0) };
    }
}

impl OpenFlags {
    /// Create flags for opening a database connection.
    #[inline]
    pub fn new() -> Self {
        OpenFlags(0)
    }

    /// Create the database if it does not already exist.
    pub fn with_create(mut self) -> Self {
        self.0 |= ffi::SQLITE_OPEN_CREATE;
        self
    }

    /// Open the database in the serialized [threading mode][1].
    ///
    /// [1]: https://www.sqlite.org/threadsafe.html
    pub fn with_full_mutex(mut self) -> Self {
        self.0 |= ffi::SQLITE_OPEN_FULLMUTEX;
        self
    }

    /// Opens the database in the multi-thread [threading mode][1].
    ///
    /// [1]: https://www.sqlite.org/threadsafe.html
    pub fn with_no_mutex(mut self) -> Self {
        self.0 |= ffi::SQLITE_OPEN_NOMUTEX;
        self
    }

    /// Open the database for reading only.
    pub fn with_read_only(mut self) -> Self {
        self.0 |= ffi::SQLITE_OPEN_READONLY;
        self
    }

    /// Open the database for reading and writing.
    pub fn with_read_write(mut self) -> Self {
        self.0 |= ffi::SQLITE_OPEN_READWRITE;
        self
    }

    /// Allow the path to be interpreted as a URI.
    pub fn with_uri(mut self) -> Self {
        self.0 |= ffi::SQLITE_OPEN_URI;
        self
    }
}

impl Default for OpenFlags {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for ConnectionThreadSafe {
    type Target = Connection;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for ConnectionThreadSafe {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

unsafe impl Sync for ConnectionThreadSafe {}

unsafe impl Send for Raw {}

extern "C" fn busy_callback<F>(callback: *mut c_void, attempts: c_int) -> c_int
where
    F: FnMut(usize) -> bool,
{
    unsafe { c_int::from((*(callback as *mut F))(attempts as usize)) }
}

extern "C" fn process_callback<F>(
    callback: *mut c_void,
    count: c_int,
    values: *mut *mut c_char,
    columns: *mut *mut c_char,
) -> c_int
where
    F: FnMut(&[(&str, Option<&str>)]) -> bool,
{
    unsafe {
        let mut pairs = Vec::with_capacity(count as usize);
        for index in 0..(count as isize) {
            let column = {
                let pointer = *columns.offset(index);
                debug_assert!(!pointer.is_null());
                c_str_to_str!(pointer).unwrap()
            };
            let value = {
                let pointer = *values.offset(index);
                if pointer.is_null() {
                    None
                } else {
                    Some(c_str_to_str!(pointer).unwrap())
                }
            };
            pairs.push((column, value));
        }
        c_int::from(!(*(callback as *mut F))(&pairs))
    }
}
