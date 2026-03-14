INTERACTIVE CLI IMPLEMENTATION – CYTAH-SPEED

Objective

Refactor the current CLI command-based interface into an interactive numeric menu system.

The CLI must allow users to run Cytah-Speed features by pressing numbers + Enter.

No mock implementations are allowed.
All menu actions must call real runtime logic already implemented in the project.

---

Main Menu Structure

When running:

cargo run

The CLI must display:

=== CYTAH SPEED MAIN MENU ===

1. Node & Mining
2. Transactions
3. Smart Contracts
4. SDK Tools
5. Help
6. Exit

User selects by typing a number and pressing Enter.

---

Menu 1 – Node & Mining

Display:

=== NODE MENU ===

1. Start Node
2. Start Mining
3. Show Node Status
4. Back

Requirements

- "Start Node" must call the real node runtime start loop.
- "Start Mining" must call the real PoW mining loop.
- "Show Node Status" must display:
  - current block height
    - DAG tips count
      - peer count
        - mempool size

        ---

        Menu 2 – Transactions

        Display:

        === TRANSACTION MENU ===

        1. Transfer CTS
        2. Check Balance
        3. Show Wallet Address
        4. Back

        Requirements

        - Transfer must construct and broadcast real transaction.
        - Balance must query real state database.
        - Address must load wallet keypair.

        ---

        Menu 3 – Smart Contracts

        Display:

        === SMART CONTRACT MENU ===

        1. Deploy Contract
        2. Call Contract
        3. Query Contract State
        4. List Contracts
        5. Back

        Must use real VM / execution engine.

        ---

        Menu 4 – SDK Tools

        Display:

        === SDK MENU ===

        1. Generate Wallet
        2. Sign Transaction
        3. Verify Signature
        4. Export Private Key
        5. Back

        Must use real cryptographic primitives (Secp256k1 or existing).

        ---

        Menu 5 – Help

        Display:

        - network ports
        - data directory
        - mining rules
        - supply rules (600M CTS fixed)

        ---

        Menu 6 – Exit

        Gracefully shutdown node runtime if running.

        ---

        Technical Requirements

        - Implement using blocking stdin loop.
        - No async menu frameworks.
        - Must run in pure terminal.
        - Use modular functions per menu.
        - Avoid code duplication.
        - All logic must call existing services:
          - NodeRuntime
            - Miner
              - WalletManager
                - TxExecutor
                  - ContractEngine

                  ---

                  UX Requirements

                  - Clear separators
                  - Always allow returning to previous menu
                  - Handle invalid input safely
                  - Never panic on bad input

                  ---

                  Performance

                  - Node runtime must run in background thread.
                  - Menu must remain responsive.

                  ---

                  Completion Criteria

                  - User can fully operate Cytah-Speed node using only numeric input.
                  - Mining can be started without restarting CLI.
                  - Transactions can be sent while node is running.
                  - Contracts can be deployed and executed.
                  - SDK tools are accessible.

                  No placeholder code.
                  All features must be real and testable.