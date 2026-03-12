use wasmtime::*;
use crate::vm::wasm_runtime::RuntimeState;

// Every host function is implemented as a named free function rather than an
// inline closure.  This simplifies type inference and guarantees that the
// function pointer is `'static`, which avoids the `IntoFunc` trait bound
// errors we were seeing when using closures.

fn storage_read(
    mut caller: Caller<'_, RuntimeState>,
    key_ptr: i32,
    key_len: i32,
    out_ptr: i32,
) -> Result<i32> {
    caller
        .data_mut()
        .gas_meter
        .charge(10)
        .map_err(|_e| Trap::MemoryOutOfBounds)?;

    let memory = caller
        .get_export("memory")
        .and_then(|e| e.into_memory())
        .ok_or(Trap::MemoryOutOfBounds)?;
    let data = memory.data(&caller);

    let start = key_ptr as usize;
    let end = start + key_len as usize;
    if end > data.len() {
        return Err(Trap::MemoryOutOfBounds.into());
    }
    let key = &data[start..end];
    let value = caller.data().storage.read(key);
    let bytes = value.unwrap_or_default();

    let out = out_ptr as usize;
    if out + bytes.len() > data.len() {
        return Err(Trap::MemoryOutOfBounds.into());
    }
    let data_mut = memory.data_mut(&mut caller);
    data_mut[out..out + bytes.len()].copy_from_slice(&bytes);
    Ok(bytes.len() as i32)
}

fn storage_write(
    mut caller: Caller<'_, RuntimeState>,
    key_ptr: i32,
    key_len: i32,
    val_ptr: i32,
    val_len: i32,
) -> Result<i32> {
    caller
        .data_mut()
        .gas_meter
        .charge(20)
        .map_err(|_e| Trap::MemoryOutOfBounds)?;

    let memory = caller
        .get_export("memory")
        .and_then(|e| e.into_memory())
        .ok_or(Trap::MemoryOutOfBounds)?;
    let data = memory.data(&caller);

    let kstart = key_ptr as usize;
    let kend = kstart + key_len as usize;
    let vstart = val_ptr as usize;
    let vend = vstart + val_len as usize;
    if kend > data.len() || vend > data.len() {
        return Err(Trap::MemoryOutOfBounds.into());
    }
    let key = &data[kstart..kend];
    let val = &data[vstart..vend];
    caller.data().storage.write(key, val);
    Ok(0)
}

fn get_caller(mut caller: Caller<'_, RuntimeState>, ptr: i32) -> Result<i32> {
    caller
        .data_mut()
        .gas_meter
        .charge(1)
        .map_err(|_e| Trap::MemoryOutOfBounds)?;

    let memory = caller
        .get_export("memory")
        .and_then(|e| e.into_memory())
        .ok_or(Trap::MemoryOutOfBounds)?;

    // borrow caller data early so we don't hold an immutable borrow while
    // later taking a mutable borrow for the memory buffer.
    let caller_bytes = caller.data().caller;

    let data = memory.data_mut(&mut caller);

    let ptrusize = ptr as usize;
    if ptrusize + 20 > data.len() {
        return Err(Trap::MemoryOutOfBounds.into());
    }
    data[ptrusize..ptrusize + 20].copy_from_slice(&caller_bytes);
    Ok(0)
}

fn get_block_height(mut caller: Caller<'_, RuntimeState>) -> Result<i64> {
    caller
        .data_mut()
        .gas_meter
        .charge(1)
        .map_err(|_e| Trap::MemoryOutOfBounds)?;
    Ok(caller.data().block_height as i64)
}

fn get_timestamp(mut caller: Caller<'_, RuntimeState>) -> Result<i64> {
    caller
        .data_mut()
        .gas_meter
        .charge(1)
        .map_err(|_e| Trap::MemoryOutOfBounds)?;
    Ok(caller.data().timestamp as i64)
}

/// Register host functions into the linker so WASM contracts can call them.
///
/// Returns `Err` if any of the registrations fail, propagating the underlying
/// Wasmtime errors. The return value is ignored in most callers since
/// initialization is expected to succeed.
pub fn register_host_functions(
    linker: &mut Linker<RuntimeState>,
) -> Result<(), Box<dyn std::error::Error>> {
    linker.func_wrap("env", "storage_read", storage_read)?;
    linker.func_wrap("env", "storage_write", storage_write)?;
    linker.func_wrap("env", "get_caller", get_caller)?;
    linker.func_wrap("env", "get_block_height", get_block_height)?;
    linker.func_wrap("env", "get_timestamp", get_timestamp)?;
    Ok(())
}
