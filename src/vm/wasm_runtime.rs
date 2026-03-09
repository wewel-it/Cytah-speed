use wasmtime::*;
use anyhow::Result;
use crate::contracts::contract_registry::ContractRegistry;

/// Shared state passed to host functions inside the Wasm runtime.
pub struct RuntimeState {
    pub contract_address: [u8;20],
    pub caller: [u8;20],
    pub block_height: u64,
    pub timestamp: u64,
    pub storage: crate::contracts::contract_storage::ContractStorage,
}

/// Wasm runtime leveraging Wasmtime for instantiation and execution.
pub struct WasmRuntime {
    engine: Engine,
    linker: Linker<RuntimeState>,
}

impl WasmRuntime {
    /// create new runtime and register host functions
    pub fn new() -> Self {
        let engine = Engine::default();
        let mut linker = Linker::new(&engine);
        crate::vm::host_functions::register_host_functions(&mut linker);
        WasmRuntime { engine, linker }
    }

    /// instantiate contract from bytecode and runtime state
    pub fn instantiate_contract(
        &self,
        bytecode: &[u8],
        state: RuntimeState,
    ) -> Result<(Store<RuntimeState>, Instance)> {
        let module = Module::new(&self.engine, bytecode)?;
        let mut store = Store::new(&self.engine, state);
        let instance = self.linker.instantiate(&mut store, &module)?;
        Ok((store, instance))
    }

    /// call an exported function
    pub fn call_function(
        &self,
        store: &mut Store<RuntimeState>,
        instance: &Instance,
        func_name: &str,
        args: &[Val],
    ) -> Result<Box<[Val]>> {
        let func = instance.get_func(store, func_name)
            .ok_or_else(|| anyhow::anyhow!("function {} not found", func_name))?;
        let ty = func.ty(store);
        let mut results = vec![Val::I32(0); ty.results().len()];
        func.call(store, args, &mut results)?;
        Ok(results.into_boxed_slice())
    }
}

