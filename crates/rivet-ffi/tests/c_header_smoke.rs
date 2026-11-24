//! Checks `include/rivet_ffi.h` against the Rust ABI and links a real C caller
//! against the `librivet_ffi` cdylib (issue #78: "a C-ABI smoke test links and
//! round-trips a handle").
//!
//! The struct layouts, status values, and constants the header declares are
//! compared with `abi.rs` through `_Static_assert`s generated from the Rust
//! types, so any drift fails the C compile. `tests/c/smoke.c` then calls every
//! exported `rivet_ffi_v1_*` symbol through the header's prototypes.
//!
//! Needs the platform C compiler (`$CC`, default `cc`), which is also the
//! linker Rust uses on the supported Unix targets.
#![cfg(unix)]

use std::fmt::Write as _;
use std::mem::{align_of, offset_of, size_of};
use std::path::{Path, PathBuf};
use std::process::Command;

use rivet_ffi::{
    RIVET_EVENT_CANCEL, RIVET_EVENT_CONTINUE, RIVET_FFI_ABI_MAJOR, RIVET_FFI_ABI_MINOR,
    RIVET_FFI_ABI_PATCH, RIVET_FFI_MAX_PAYLOAD_BYTES, RivetAbiVersion, RivetBlockPos,
    RivetByteView, RivetBytesResult, RivetCallbackResult, RivetCallbackToken, RivetEntityHandle,
    RivetEntityHandleResult, RivetEntityInfo, RivetEntityLookupResult, RivetEventResult,
    RivetOwnedBytes, RivetRuntimeHandle, RivetRuntimeResult, RivetStatus, RivetTickResult,
};

macro_rules! layout {
    ($out:ident, $ty:ident { $($field:ident),* $(,)? }) => {
        writeln!(
            $out,
            "_Static_assert(sizeof({0}) == {1}, \"sizeof({0})\");\n\
             _Static_assert(_Alignof({0}) == {2}, \"_Alignof({0})\");",
            stringify!($ty),
            size_of::<$ty>(),
            align_of::<$ty>(),
        )
        .unwrap();
        $(
            writeln!(
                $out,
                "_Static_assert(offsetof({0}, {1}) == {2}, \"offsetof({0}, {1})\");",
                stringify!($ty),
                stringify!($field),
                offset_of!($ty, $field),
            )
            .unwrap();
        )*
    };
}

/// C translation unit asserting the header matches the Rust ABI definitions.
fn layout_assertions() -> String {
    let mut out = String::from("#include <stddef.h>\n#include \"rivet_ffi.h\"\n\n");

    for (name, value) in [
        ("RIVET_FFI_ABI_MAJOR", i64::from(RIVET_FFI_ABI_MAJOR)),
        ("RIVET_FFI_ABI_MINOR", i64::from(RIVET_FFI_ABI_MINOR)),
        ("RIVET_FFI_ABI_PATCH", i64::from(RIVET_FFI_ABI_PATCH)),
        (
            "RIVET_FFI_MAX_PAYLOAD_BYTES",
            i64::from(RIVET_FFI_MAX_PAYLOAD_BYTES),
        ),
        ("RIVET_EVENT_CONTINUE", i64::from(RIVET_EVENT_CONTINUE)),
        ("RIVET_EVENT_CANCEL", i64::from(RIVET_EVENT_CANCEL)),
    ] {
        writeln!(out, "_Static_assert({name} == {value}, \"{name}\");").unwrap();
    }

    for (name, status) in [
        ("RIVET_STATUS_OK", RivetStatus::Ok),
        (
            "RIVET_STATUS_INVALID_ARGUMENT",
            RivetStatus::InvalidArgument,
        ),
        ("RIVET_STATUS_NULL_POINTER", RivetStatus::NullPointer),
        (
            "RIVET_STATUS_ABI_VERSION_MISMATCH",
            RivetStatus::AbiVersionMismatch,
        ),
        ("RIVET_STATUS_HANDLE_INVALID", RivetStatus::HandleInvalid),
        ("RIVET_STATUS_HANDLE_STALE", RivetStatus::HandleStale),
        ("RIVET_STATUS_BUFFER_TOO_LARGE", RivetStatus::BufferTooLarge),
        ("RIVET_STATUS_INVALID_UTF8", RivetStatus::InvalidUtf8),
        (
            "RIVET_STATUS_CALLBACK_NOT_FOUND",
            RivetStatus::CallbackNotFound,
        ),
        (
            "RIVET_STATUS_CALLBACK_REENTRANT_UNREGISTER",
            RivetStatus::CallbackReentrantUnregister,
        ),
        ("RIVET_STATUS_WRONG_THREAD", RivetStatus::WrongThread),
        ("RIVET_STATUS_PANIC_CAUGHT", RivetStatus::PanicCaught),
        (
            "RIVET_STATUS_CAPACITY_EXCEEDED",
            RivetStatus::CapacityExceeded,
        ),
    ] {
        writeln!(
            out,
            "_Static_assert({name} == {}, \"{name}\");",
            status as i32
        )
        .unwrap();
    }
    writeln!(
        out,
        "_Static_assert(sizeof(RivetStatus) == {}, \"sizeof(RivetStatus)\");\n\
         _Static_assert(_Alignof(RivetStatus) == {}, \"_Alignof(RivetStatus)\");",
        size_of::<RivetStatus>(),
        align_of::<RivetStatus>(),
    )
    .unwrap();

    layout!(
        out,
        RivetAbiVersion {
            major,
            minor,
            patch,
            reserved
        }
    );
    layout!(out, RivetRuntimeHandle { slot, generation });
    layout!(
        out,
        RivetEntityHandle {
            slot,
            generation,
            type_id,
            flags
        }
    );
    layout!(out, RivetBlockPos { world_id, x, y, z });
    layout!(out, RivetCallbackToken { slot, generation });
    layout!(out, RivetByteView { ptr, len, reserved });
    layout!(out, RivetOwnedBytes { ptr, len, cap });
    layout!(
        out,
        RivetEntityInfo {
            handle,
            pos,
            alive,
            reserved
        }
    );
    layout!(
        out,
        RivetRuntimeResult {
            status,
            reserved,
            handle
        }
    );
    layout!(
        out,
        RivetTickResult {
            status,
            reserved,
            tick_number
        }
    );
    layout!(
        out,
        RivetEntityHandleResult {
            status,
            reserved,
            handle
        }
    );
    layout!(
        out,
        RivetEntityLookupResult {
            status,
            reserved,
            info
        }
    );
    layout!(
        out,
        RivetCallbackResult {
            status,
            reserved,
            token
        }
    );
    layout!(
        out,
        RivetEventResult {
            status,
            delivered_count,
            cancelled,
            reserved
        }
    );
    layout!(
        out,
        RivetBytesResult {
            status,
            reserved,
            bytes
        }
    );
    out
}

/// Directory holding the cdylib built alongside this test binary
/// (`target/<profile>/deps`).
fn cdylib_dir() -> PathBuf {
    let exe = std::env::current_exe().expect("test executable path");
    let dir = exe
        .parent()
        .expect("test executable directory")
        .to_path_buf();
    let lib = dir.join(format!(
        "{}rivet_ffi{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    ));
    assert!(
        lib.is_file(),
        "rivet-ffi cdylib not found at {}",
        lib.display()
    );
    dir
}

fn run(cmd: &mut Command) -> String {
    let output = cmd
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn {cmd:?}: {e}"));
    assert!(
        output.status.success(),
        "{cmd:?} failed with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn c_header_matches_abi_and_c_caller_round_trips_handles_through_cdylib() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let work = Path::new(env!("CARGO_TARGET_TMPDIR")).join("rivet_ffi_c_header_smoke");
    std::fs::create_dir_all(&work).expect("create work dir");
    let layout_c = work.join("layout_check.c");
    std::fs::write(&layout_c, layout_assertions()).expect("write layout_check.c");
    let exe = work.join("smoke");

    let lib_dir = cdylib_dir();
    let cc = std::env::var_os("CC").unwrap_or_else(|| "cc".into());
    run(Command::new(&cc)
        .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-pedantic"])
        .arg("-I")
        .arg(manifest.join("include"))
        .arg(manifest.join("tests/c/smoke.c"))
        .arg(&layout_c)
        .arg("-o")
        .arg(&exe)
        .arg("-L")
        .arg(&lib_dir)
        .arg("-lrivet_ffi")
        .arg(format!("-Wl,-rpath,{}", lib_dir.display())));

    let stdout = run(&mut Command::new(&exe));
    assert_eq!(stdout.trim(), "rivet_ffi C smoke: ok");
}
