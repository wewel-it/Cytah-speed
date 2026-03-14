You are a senior distributed systems engineer and blockchain protocol researcher.

Your task is to upgrade this GhostDAG-based BlockDAG implementation into production-grade infrastructure.

Perform deep architectural improvements, code refactoring, performance optimization, and security hardening across the following modules:

1. P2P Networking and Discovery

- Improve libp2p peer discovery using hybrid strategies (DNS seeds, DHT, bootstrap nodes).
- Implement peer scoring, reputation system, anti-spam bandwidth throttling.
- Add block propagation optimization (header-first, compact block relay).
- Implement parallel gossip broadcasting and latency-aware peer selection.
- Add network simulation tests for high node counts (1000+ nodes).

2. Mining and Difficulty Adjustment

- Replace naive difficulty adjustment with a robust DAA based on block rate statistics and network hashrate estimation.
- Prevent timestamp manipulation and hash rate oscillation attacks.
- Implement adaptive block target interval tuning for high-rate DAG.
- Add miner incentive validation and orphan rate monitoring.

3. GhostDAG Finality Engine

- Refactor blue set selection algorithm for deterministic ordering.
- Implement optimized reachability index using interval tree or DAG ancestry cache.
- Add protection against k-cluster grinding and anticone manipulation.
- Improve virtual selected parent stability under network delay.
- Implement probabilistic and score-based finality threshold logic.
- Add large DAG reorg simulation tests.

4. DAG Mempool Management

- Convert mempool into DAG-aware priority structure.
- Implement fee market with congestion pricing.
- Add double-spend fast detection using UTXO conflict graph.
- Add mempool eviction policy based on fee density and age.
- Implement rate limiting per peer.

5. Storage and Pruning

- Optimize RocksDB configuration for high-throughput writes.
- Implement rolling pruning window with state snapshot checkpoints.
- Add fast node bootstrap via snapshot sync.
- Add DAG reachability index persistence.
- Implement crash-safe state rollback mechanism.

6. Security Hardening

- Add DoS protection, signature batch verification, replay protection.
- Implement transaction validation pipeline with stateless checks first.
- Add fuzz testing and adversarial DAG attack simulations.
- Implement hidden chain and spam burst attack detection.

7. SDK and RPC Layer

- Expand Rust and Java SDK with full transaction builder, wallet tools, and streaming APIs.
- Add JSON-RPC and gRPC interfaces.
- Improve developer ergonomics and documentation generation.
- Provide example dApps and integration tests.

8. Performance and Parallel Execution

- Refactor validation engine for lock-free parallelism.
- Implement UTXO sharding or execution batching.
- Add metrics instrumentation (TPS, orphan rate, latency, memory usage).
- Provide benchmark suite and stress testing tools.

General Requirements:

- Maintain deterministic consensus behavior.
- Write production-grade Rust code with async concurrency.
- Ensure memory safety and minimal locking.
- Add extensive unit tests, integration tests, and simulation tools.
- Document all architectural changes clearly.

Goal:
Transform this project into a scalable high-throughput BlockDAG mainnet-ready protocol similar to modern GhostDAG implementations.