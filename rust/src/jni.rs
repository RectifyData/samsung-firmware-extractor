//! JNI bridge used by the Kotlin side (`com.rectify.fwextract.Native`).
//!
//! Kotlin opens each firmware with ContentResolver.openFileDescriptor(), calls detachFd(),
//! and passes the descriptor here. Rust takes ownership and closes it when done.

use crate::{extract_images, list_images, Events, CANCEL};
use anyhow::Result;
use jni::objects::{JClass, JObject, JString, JValue};
use jni::sys::{jint, jstring};
use jni::JNIEnv;
use std::collections::HashSet;
use std::fs::File;
use std::os::unix::io::FromRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

/// Returns one line per image: "<size or -1>\t<path>\n".
#[no_mangle]
pub extern "system" fn Java_com_rectify_fwextract_Native_scan<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    fd: jint,
    name: JString<'local>,
) -> jstring {
    match scan_inner(&mut env, fd, &name) {
        Ok(text) => text,
        Err(e) => throw(&mut env, e),
    }
}

/// Extracts the newline-separated `selected` image paths into `work_dir`, reporting through `callback`.
#[no_mangle]
pub extern "system" fn Java_com_rectify_fwextract_Native_extract<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    fd: jint,
    name: JString<'local>,
    selected: JString<'local>,
    work_dir: JString<'local>,
    callback: JObject<'local>,
) {
    if let Err(e) = extract_inner(&mut env, fd, &name, &selected, &work_dir, &callback) {
        // If a Kotlin callback threw, that exception is already pending; don't stack another one.
        if !env.exception_check().unwrap_or(false) {
            let _ = env.throw_new("java/lang/RuntimeException", format!("{e:#}"));
        }
    }
}

/// Asks any running scan or extraction to stop at its next read.
#[no_mangle]
pub extern "system" fn Java_com_rectify_fwextract_Native_cancel<'local>(_env: JNIEnv<'local>, _class: JClass<'local>) {
    CANCEL.store(true, Ordering::Relaxed);
}

fn scan_inner<'l>(env: &mut JNIEnv<'l>, fd: jint, name: &JString<'l>) -> Result<jstring> {
    let name = get_string(env, name)?;
    // SAFETY: the caller detached this descriptor from its ParcelFileDescriptor; we now own it.
    let file = unsafe { File::from_raw_fd(fd) };
    CANCEL.store(false, Ordering::Relaxed);

    let images = list_images(file, &name, &CANCEL)?;
    let text: String = images
        .iter()
        .map(|i| format!("{}\t{}\n", i.size.map(|s| s as i64).unwrap_or(-1), i.path))
        .collect();
    Ok(env.new_string(text)?.into_raw())
}

fn extract_inner<'l>(
    env: &mut JNIEnv<'l>,
    fd: jint,
    name: &JString<'l>,
    selected: &JString<'l>,
    work_dir: &JString<'l>,
    callback: &JObject<'l>,
) -> Result<()> {
    let name = get_string(env, name)?;
    let selected: HashSet<String> = get_string(env, selected)?
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect();
    let work = PathBuf::from(get_string(env, work_dir)?);
    // SAFETY: as in scan_inner, Rust owns the detached descriptor.
    let file = unsafe { File::from_raw_fd(fd) };
    CANCEL.store(false, Ordering::Relaxed);

    let mut events = JniEvents { env, callback };
    extract_images(file, &name, &selected, &work, &mut events, &CANCEL)
}

fn get_string(env: &mut JNIEnv, s: &JString) -> Result<String> {
    Ok(env.get_string(s)?.into())
}

fn throw(env: &mut JNIEnv, e: anyhow::Error) -> jstring {
    if !env.exception_check().unwrap_or(false) {
        let _ = env.throw_new("java/lang/RuntimeException", format!("{e:#}"));
    }
    std::ptr::null_mut()
}

/// Forwards progress and finished images to `Native.Callback` on the Kotlin side.
struct JniEvents<'a, 'l> {
    env: &'a mut JNIEnv<'l>,
    callback: &'a JObject<'l>,
}

impl Events for JniEvents<'_, '_> {
    fn log(&mut self, message: &str) {
        if let Ok(text) = self.env.new_string(message) {
            let _ = self.env.call_method(
                self.callback,
                "onLog",
                "(Ljava/lang/String;)V",
                &[JValue::Object(&text)],
            );
        }
    }

    fn image(&mut self, name: &str, path: &Path) -> Result<()> {
        let n = self.env.new_string(name)?;
        let p = self.env.new_string(path.to_string_lossy().into_owned())?;
        self.env.call_method(
            self.callback,
            "onImage",
            "(Ljava/lang/String;Ljava/lang/String;)V",
            &[JValue::Object(&n), JValue::Object(&p)],
        )?;
        Ok(())
    }
}
