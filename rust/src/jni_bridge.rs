//! JNI entry point. A single function keeps the FFI surface minimal: Kotlin sends one
//! JSON request string and receives one JSON response string. All real work (and all
//! panic handling) lives in [`crate::handle_request`].

use jni::objects::{JObject, JString};
use jni::sys::jstring;
use jni::JNIEnv;
use std::ptr;

/// Kotlin: `com.hydrocore.app.engine.NativeTransport.nativeDispatch(request: String): String?`
///
/// Never panics and never throws: on any failure it returns a JSON error envelope, or
/// `null` if even a Java string could not be allocated (Kotlin handles `null`).
#[no_mangle]
pub extern "system" fn Java_com_hydrocore_app_engine_NativeTransport_nativeDispatch<'local>(
    mut env: JNIEnv<'local>,
    _this: JObject<'local>,
    request: JString<'local>,
) -> jstring {
    let response = match env.get_string(&request) {
        Ok(java_str) => {
            let request: String = java_str.into();
            crate::handle_request(&request)
        }
        Err(_) => crate::error_envelope("could not read request string from JVM"),
    };

    match env.new_string(response) {
        Ok(s) => s.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}
