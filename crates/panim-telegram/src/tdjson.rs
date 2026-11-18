//! A thin layer over the JSON interface of TDLib (`td_json_client.h`).
//!
//! The layer has four functions of `libtdjson.so`. Each request and each answer is one JSON object.
//! All the clients of the process share one queue of answers and updates. Only one thread can call `receive`.

use std::ffi::{CStr, CString, c_char, c_double, c_int};

use serde_json::Value;

#[link(name = "tdjson")]
unsafe extern "C" {
    fn td_create_client_id() -> c_int;
    fn td_send(client_id: c_int, request: *const c_char);
    fn td_receive(timeout: c_double) -> *const c_char;
    fn td_execute(request: *const c_char) -> *const c_char;
}

/// Makes the id of a new client. TDLib starts the client when it gets the first request for that id.
pub fn create_client_id() -> i32 {
    // SAFETY: the function has no arguments and no conditions.
    unsafe { td_create_client_id() }
}

/// Sends a request. The answer comes later through `receive`, with the same `@extra` field.
pub fn send(client_id: i32, request: &Value) {
    let Ok(text) = CString::new(request.to_string()) else {
        // `serde_json` writes a NUL character as `\u0000`, so the text of a JSON value has no NUL byte.
        return;
    };
    // SAFETY: `text` is a C string that lives until the call returns. TDLib makes a copy.
    unsafe { td_send(client_id, text.as_ptr()) }
}

/// Waits for the next answer or update of all the clients. The field `@client_id` names the client.
pub fn receive(timeout_seconds: f64) -> Option<Value> {
    // SAFETY: TDLib returns NULL or a C string that stays valid until the next call of `td_receive` or
    // `td_execute` on this thread. We copy the string before we return.
    let text = unsafe {
        let pointer = td_receive(timeout_seconds);
        if pointer.is_null() {
            return None;
        }
        CStr::from_ptr(pointer).to_string_lossy().into_owned()
    };
    serde_json::from_str(&text).ok()
}

/// Runs a synchronous request. Only some requests can run in this manner, for example the log settings.
pub fn execute(request: &Value) -> Option<Value> {
    let text = CString::new(request.to_string()).ok()?;
    // SAFETY: see `receive`.
    let answer = unsafe {
        let pointer = td_execute(text.as_ptr());
        if pointer.is_null() {
            return None;
        }
        CStr::from_ptr(pointer).to_string_lossy().into_owned()
    };
    serde_json::from_str(&answer).ok()
}
