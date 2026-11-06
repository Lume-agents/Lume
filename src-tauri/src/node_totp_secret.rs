//! OS-protected TOTP secrets. No plaintext fallback is permitted if a credential store is unavailable.

use std::path::Path;
use zeroize::Zeroizing;

fn valid_key_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

pub(crate) fn store(directory: &Path, id: &str, secret: &[u8]) -> Result<(), String> {
    if !valid_key_id(id) || secret.len() != 20 { return Err("invalid authenticator secret".into()); }
    platform::store(directory, id, secret)
}

pub(crate) fn load(directory: &Path, id: &str) -> Result<Option<Zeroizing<Vec<u8>>>, String> {
    if !valid_key_id(id) { return Err("invalid authenticator identifier".into()); }
    let secret = platform::load(directory, id)?;
    if secret.as_ref().is_some_and(|value| value.len() != 20) { return Err("The stored authenticator is invalid".into()); }
    Ok(secret)
}

pub(crate) fn remove(directory: &Path, id: &str) -> Result<(), String> {
    if !valid_key_id(id) { return Err("invalid authenticator identifier".into()); }
    platform::remove(directory, id)
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use gtk::glib::ffi::{g_error_free, GError};
    use libloading::Library;
    use std::{ffi::{c_char, c_int, c_void, CStr, CString}, ptr};

    type NewSchema = unsafe extern "C" fn(*const c_char, c_int, ...) -> *mut c_void;
    type UnrefSchema = unsafe extern "C" fn(*mut c_void);
    type Store = unsafe extern "C" fn(*const c_void, *const c_char, *const c_char, *const c_char, *mut c_void, *mut *mut GError, ...) -> c_int;
    type Lookup = unsafe extern "C" fn(*const c_void, *mut c_void, *mut *mut GError, ...) -> *mut c_char;
    type Clear = unsafe extern "C" fn(*const c_void, *mut c_void, *mut *mut GError, ...) -> c_int;
    type FreePassword = unsafe extern "C" fn(*mut c_char);

    fn use_store<T>(id: &str, call: impl FnOnce(&Library, *mut c_void, &CString) -> Result<T, String>) -> Result<T, String> {
        let id = CString::new(id).map_err(|_| "invalid authenticator identifier")?;
        // The schema is built through libsecret's API, rather than relying on its private layout.
        unsafe {
            let library = Library::new("libsecret-1.so.0").map_err(|_| "The system credential store is unavailable".to_string())?;
            let new: libloading::Symbol<NewSchema> = library.get(b"secret_schema_new\0").map_err(|_| "The system credential store is unavailable")?;
            let unref: libloading::Symbol<UnrefSchema> = library.get(b"secret_schema_unref\0").map_err(|_| "The system credential store is unavailable")?;
            let schema = new(c"org.lume.NodeOtp".as_ptr(), 0, c"key".as_ptr(), 0i32, ptr::null::<c_char>());
            if schema.is_null() { return Err("The system credential store is unavailable".into()); }
            let result = call(&library, schema, &id);
            unref(schema);
            result
        }
    }

    unsafe fn had_error(error: *mut GError) -> bool {
        if error.is_null() { false } else { g_error_free(error); true }
    }

    pub(super) fn store(_directory: &Path, id: &str, secret: &[u8]) -> Result<(), String> {
        use_store(id, |library, schema, id| unsafe {
            let store: libloading::Symbol<Store> = library.get(b"secret_password_store_sync\0").map_err(|_| "The system credential store is unavailable")?;
            let mut encoded = Zeroizing::new(URL_SAFE_NO_PAD.encode(secret).into_bytes());
            encoded.push(0);
            let mut error = ptr::null_mut();
            let ok = store(schema, ptr::null(), c"Lume Node authenticator".as_ptr(), encoded.as_ptr().cast(), ptr::null_mut(), &mut error, c"key".as_ptr(), id.as_ptr(), ptr::null::<c_char>());
            if had_error(error) || ok == 0 { return Err("Could not save the authenticator in the system credential store".into()); }
            Ok(())
        })
    }

    pub(super) fn load(_directory: &Path, id: &str) -> Result<Option<Zeroizing<Vec<u8>>>, String> {
        use_store(id, |library, schema, id| unsafe {
            let lookup: libloading::Symbol<Lookup> = library.get(b"secret_password_lookup_sync\0").map_err(|_| "The system credential store is unavailable")?;
            let free: libloading::Symbol<FreePassword> = library.get(b"secret_password_free\0").map_err(|_| "The system credential store is unavailable")?;
            let mut error = ptr::null_mut();
            let password = lookup(schema, ptr::null_mut(), &mut error, c"key".as_ptr(), id.as_ptr(), ptr::null::<c_char>());
            if had_error(error) { if !password.is_null() { free(password); } return Err("Could not read the system credential store".into()); }
            if password.is_null() { return Ok(None); }
            let decoded = URL_SAFE_NO_PAD.decode(CStr::from_ptr(password).to_bytes()).map(Zeroizing::new).map_err(|_| "The stored authenticator is invalid".to_string());
            free(password);
            decoded.map(Some)
        })
    }

    pub(super) fn remove(_directory: &Path, id: &str) -> Result<(), String> {
        use_store(id, |library, schema, id| unsafe {
            let clear: libloading::Symbol<Clear> = library.get(b"secret_password_clear_sync\0").map_err(|_| "The system credential store is unavailable")?;
            let mut error = ptr::null_mut();
            clear(schema, ptr::null_mut(), &mut error, c"key".as_ptr(), id.as_ptr(), ptr::null::<c_char>());
            if had_error(error) { return Err("Could not remove the authenticator from the system credential store".into()); }
            Ok(())
        })
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::{fs::{self, OpenOptions}, io::Write, ptr};
    use windows_sys::Win32::{Foundation::LocalFree, Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB}};

    fn path(directory: &Path, id: &str) -> std::path::PathBuf { directory.join(format!("otp-{id}.dpapi")) }

    fn protect(input: &[u8], encrypt: bool) -> Result<Zeroizing<Vec<u8>>, String> {
        let source = CRYPT_INTEGER_BLOB { cbData: input.len().try_into().map_err(|_| "invalid secret")?, pbData: input.as_ptr() as *mut u8 };
        let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: ptr::null_mut() };
        unsafe {
            let ok = if encrypt {
                CryptProtectData(&source, ptr::null(), ptr::null(), ptr::null(), ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut output)
            } else {
                CryptUnprotectData(&source, ptr::null_mut(), ptr::null(), ptr::null(), ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut output)
            };
            if ok == 0 || output.pbData.is_null() { return Err("The Windows credential protector is unavailable".into()); }
            let value = Zeroizing::new(std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec());
            ptr::write_bytes(output.pbData, 0, output.cbData as usize);
            LocalFree(output.pbData.cast());
            Ok(value)
        }
    }

    pub(super) fn store(directory: &Path, id: &str, secret: &[u8]) -> Result<(), String> {
        fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        let encrypted = protect(secret, true)?;
        let mut file = OpenOptions::new().write(true).create_new(true).open(path(directory, id)).map_err(|error| error.to_string())?;
        file.write_all(&encrypted).and_then(|_| file.sync_all()).map_err(|error| error.to_string())
    }

    pub(super) fn load(directory: &Path, id: &str) -> Result<Option<Zeroizing<Vec<u8>>>, String> {
        match fs::read(path(directory, id)) {
            Ok(bytes) => protect(&bytes, false).map(Some),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    pub(super) fn remove(directory: &Path, id: &str) -> Result<(), String> {
        match fs::remove_file(path(directory, id)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use std::{ffi::{c_char, c_void, CString}, ptr};

    const MISSING: i32 = -25300;
    const SERVICE: &str = "org.lume.NodeOtp";
    #[link(name = "Security", kind = "framework")]
    extern "C" {
        fn SecKeychainAddGenericPassword(keychain: *const c_void, service_len: u32, service: *const c_char, account_len: u32, account: *const c_char, secret_len: u32, secret: *const c_void, item: *mut *mut c_void) -> i32;
        fn SecKeychainFindGenericPassword(keychain: *const c_void, service_len: u32, service: *const c_char, account_len: u32, account: *const c_char, secret_len: *mut u32, secret: *mut *mut c_void, item: *mut *mut c_void) -> i32;
        fn SecKeychainItemFreeContent(attributes: *const c_void, secret: *mut c_void) -> i32;
        fn SecKeychainItemDelete(item: *mut c_void) -> i32;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" { fn CFRelease(item: *const c_void); }

    pub(super) fn store(_directory: &Path, id: &str, secret: &[u8]) -> Result<(), String> {
        let account = CString::new(id).map_err(|_| "invalid authenticator identifier")?;
        let status = unsafe { SecKeychainAddGenericPassword(ptr::null(), SERVICE.len() as u32, SERVICE.as_ptr().cast(), id.len() as u32, account.as_ptr(), secret.len() as u32, secret.as_ptr().cast(), ptr::null_mut()) };
        if status != 0 { return Err("Could not save the authenticator in the macOS Keychain".into()); }
        Ok(())
    }

    pub(super) fn load(_directory: &Path, id: &str) -> Result<Option<Zeroizing<Vec<u8>>>, String> {
        let account = CString::new(id).map_err(|_| "invalid authenticator identifier")?;
        let mut size = 0;
        let mut value = ptr::null_mut();
        let status = unsafe { SecKeychainFindGenericPassword(ptr::null(), SERVICE.len() as u32, SERVICE.as_ptr().cast(), id.len() as u32, account.as_ptr(), &mut size, &mut value, ptr::null_mut()) };
        if status == MISSING { return Ok(None); }
        if status != 0 || value.is_null() { return Err("Could not read the macOS Keychain".into()); }
        let secret = unsafe { Zeroizing::new(std::slice::from_raw_parts(value.cast::<u8>(), size as usize).to_vec()) };
        unsafe { SecKeychainItemFreeContent(ptr::null(), value); }
        Ok(Some(secret))
    }

    pub(super) fn remove(_directory: &Path, id: &str) -> Result<(), String> {
        let account = CString::new(id).map_err(|_| "invalid authenticator identifier")?;
        let mut item = ptr::null_mut();
        let status = unsafe { SecKeychainFindGenericPassword(ptr::null(), SERVICE.len() as u32, SERVICE.as_ptr().cast(), id.len() as u32, account.as_ptr(), ptr::null_mut(), ptr::null_mut(), &mut item) };
        if status == MISSING { return Ok(()); }
        if status != 0 || item.is_null() { return Err("Could not access the macOS Keychain".into()); }
        let result = unsafe { let result = SecKeychainItemDelete(item); CFRelease(item); result };
        if result != 0 { return Err("Could not remove the authenticator from the macOS Keychain".into()); }
        Ok(())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod platform {
    use super::*;
    pub(super) fn store(_: &Path, _: &str, _: &[u8]) -> Result<(), String> { Err("The system credential store is unavailable".into()) }
    pub(super) fn load(_: &Path, _: &str) -> Result<Option<Zeroizing<Vec<u8>>>, String> { Err("The system credential store is unavailable".into()) }
    pub(super) fn remove(_: &Path, _: &str) -> Result<(), String> { Err("The system credential store is unavailable".into()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn key_ids_are_path_safe() {
        for invalid in ["", "../other", "a/b", "\\node", "data.txt", " "] { assert!(!valid_key_id(invalid)); }
        assert!(valid_key_id("key_a-b123"));
    }

    #[test]
    #[ignore = "requires a disposable unlocked OS credential store"]
    fn os_credential_store_round_trip() {
        let directory = std::env::temp_dir().join(format!("lume-otp-secret-test-{}", std::process::id()));
        let id = format!("lume-test-{}", std::process::id());
        let secret = [17u8; 20];
        store(&directory, &id, &secret).unwrap();
        assert_eq!(load(&directory, &id).unwrap().unwrap().as_slice(), secret);
        remove(&directory, &id).unwrap();
        assert!(load(&directory, &id).unwrap().is_none());
        let _ = std::fs::remove_dir_all(directory);
    }
}
