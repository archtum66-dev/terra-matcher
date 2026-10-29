//! Minimal exports for the browser: copy bytes in, call `run`, read bytes out.
//!
//! No `unsafe` block is used. The lint is relaxed only because `#[no_mangle]` itself counts.
#![allow(unsafe_code)]

use std::cell::RefCell;

use crate::{conformance_report, Session};

thread_local! {
    static INPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static OUTPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
}

fn ok(data: &str) -> String {
    format!("{{\"ok\":true,\"data\":{data}}}")
}

fn err(message: &str) -> String {
    serde_json::json!({ "ok": false, "error": message }).to_string()
}

fn result(outcome: Result<String, String>) -> String {
    match outcome {
        Ok(data) => ok(&data),
        Err(e) => err(&e),
    }
}

fn with_session(f: impl FnOnce(&mut Session) -> Result<String, String>) -> String {
    SESSION.with(|s| match s.borrow_mut().as_mut() {
        Some(session) => result(f(session)),
        None => err("no session: start one first"),
    })
}

/// Makes room for `len` input bytes and returns their address in linear memory.
#[no_mangle]
pub extern "C" fn input_reserve(len: u32) -> u32 {
    INPUT.with(|i| {
        let mut v = i.borrow_mut();
        v.clear();
        v.resize(len as usize, 0);
        v.as_ptr() as usize as u32
    })
}

/// Address of the last result.
#[no_mangle]
pub extern "C" fn output_ptr() -> u32 {
    OUTPUT.with(|o| o.borrow().as_ptr() as usize as u32)
}

/// Runs an operation on the reserved input and returns the length of the result.
/// 0 = start session (input: market JSON), 1 = process message (input: message JSON),
/// 2 = run conformance vectors, 3 = export the session as JSON lines.
#[no_mangle]
pub extern "C" fn run(op: u32) -> u32 {
    let input = INPUT.with(|i| String::from_utf8_lossy(&i.borrow()).into_owned());
    let text = match op {
        0 => match Session::new(&input) {
            Ok(session) => {
                let started = session.start_json();
                SESSION.with(|s| *s.borrow_mut() = Some(session));
                result(started)
            }
            Err(e) => err(&e),
        },
        1 => with_session(|s| s.step(&input)),
        2 => ok(&conformance_report()),
        3 => with_session(|s| {
            s.export_jsonl()
                .map(|t| serde_json::Value::String(t).to_string())
        }),
        _ => err("unknown operation"),
    };
    OUTPUT.with(|o| {
        let mut v = o.borrow_mut();
        *v = text.into_bytes();
        v.len() as u32
    })
}
