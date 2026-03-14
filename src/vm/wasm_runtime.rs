use wasmtime::*;
use anyhow::Result;

/// Shared state passed to host functions inside the Wasm runtime.
pub trait Storage: Send + Sync + std::any::Any {
    fn read(&self, key: &[u8]) -> Option<Vec<u8>>;
    fn write(&self, key: &[u8], value: &[u8]);
    fn box_clone(&self) -> Box<dyn Storage>;
    fn as_any(&self) -> &dyn std::any::Any;
}

impl Clone for Box<dyn Storage> {
    fn clone(&self) -> Box<dyn Storage> {
        self.box_clone()
    }
}

/// Shared state passed to host functions inside the Wasm runtime.
pub struct RuntimeState {
    pub contract_address: [u8;20],
    pub caller: [u8;20],
    pub block_height: u64,
    pub timestamp: u64,
    pub storage: Box<dyn Storage>,
    pub gas_meter: crate::vm::gas_meter::GasMeter,
    /// optional per-instance memory/table limiter kept in the state so the
    /// `Store::limiter` callback can return a reference to it.
    pub memory_limiter: Option<Box<dyn ResourceLimiter>>,
}

/// Wasm runtime leveraging Wasmtime for instantiation and execution.
#[derive(Debug)]
pub struct WasmRuntime {
    engine: Engine,
    linker: Linker<RuntimeState>,
}

impl WasmRuntime {
    /// create new runtime and register host functions
    pub fn new() -> Self {
        // configure engine to consume fuel so we can charge per-instruction
        let mut config = Config::new();
        config.consume_fuel(true);
        // note: resource limiter is attached per-store below rather than via
        // `Config` which no longer exposes `resource_limiter`.
        let engine = Engine::new(&config).expect("failed to create engine");
        let mut linker = Linker::new(&engine);
        crate::vm::host_functions::register_host_functions(&mut linker)
            .expect("failed to register host functions");
        WasmRuntime { engine, linker }
    }

    /// instantiate contract from bytecode and runtime state
    pub fn instantiate_contract(
        &self,
        bytecode: &[u8],
        mut state: RuntimeState,
        gas_limit: u64,
    ) -> Result<(Store<RuntimeState>, Instance)> {
        // initialize gas meter
        state.gas_meter = crate::vm::gas_meter::GasMeter::new(gas_limit);
        // attach memory limiter instance inside the state so the store can
        // reference it later; limiting to 16 pages (~1MiB)
        #[derive(Clone)]
        struct MemoryLimiter {
            max_pages: u32,
        }
        impl ResourceLimiter for MemoryLimiter {
            fn memory_growing(
                &mut self,
                _current: usize,
                desired: usize,
                _maximum: Option<usize>,
            ) -> Result<bool, wasmtime::Error> {
                Ok(desired <= self.max_pages as usize)
            }
            fn table_growing(
                &mut self,
                _current: usize,
                _desired: usize,
                _maximum: Option<usize>,
            ) -> Result<bool, wasmtime::Error> {
                Ok(true)
            }
        }
        state.memory_limiter = Some(Box::new(MemoryLimiter { max_pages: 16 }));

        let module = Module::new(&self.engine, bytecode)?;
        let mut store = Store::new(&self.engine, state);
        // install the limiter callback that returns the limiter stored in
        // the runtime state (dereference the Box to the trait object)
        store.limiter(|state| state.memory_limiter.as_mut().unwrap().as_mut());

        // older versions of Wasmtime used fuel; we rely on our own GasMeter
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
        // reborrow the store each time we pass it to a Wasmtime API to
        // avoid move/borrow errors. `&mut *store` creates a fresh mutable
        // reference.
        let func = instance
            .get_func(&mut *store, func_name)
            .ok_or_else(|| anyhow::anyhow!("function {} not found", func_name))?;
        let ty = func.ty(&mut *store);
        let mut results = vec![Val::I32(0); ty.results().len()];
        func.call(&mut *store, args, &mut results)?;
        Ok(results.into_boxed_slice())
    }
}

#[cfg(test)]
mod tests {
        use super::*;
        use wasmtime::{ResourceLimiter, Config};

        #[test]
        fn test_memory_limiter_rejects_big_growth() {
            // replicate limiter logic
            struct TestLimiter { max_pages: u32 }
            impl ResourceLimiter for TestLimiter {
                fn memory_growing(&mut self, _current: usize, desired: usize, _maximum: Option<usize>) -> Result<bool, wasmtime::Error> {
                    Ok(desired <= self.max_pages as usize)
                }
                fn table_growing(&mut self, _current: usize, _desired: usize, _maximum: Option<usize>) -> Result<bool, wasmtime::Error> { Ok(true) }
            }
            let mut l = TestLimiter { max_pages: 2 };
            assert!(l.memory_growing(1, 2, None).unwrap());
            assert!(!l.memory_growing(2, 3, None).unwrap());
        }

        #[test]
        fn test_gas_meter_charges() {
            let mut meter = crate::vm::gas_meter::GasMeter::new(100);
            assert!(meter.charge(50).is_ok());
            assert!(meter.charge(60).is_err());
        }
    }
