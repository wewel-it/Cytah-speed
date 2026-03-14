#!/usr/bin/env bash

# Cytah-Speed Devnet Tools
# Local development network management

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
DEVNET_DIR="$PROJECT_ROOT/devnet"
CONFIG_FILE="$DEVNET_DIR/config.toml"
LOG_FILE="$DEVNET_DIR/devnet.log"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Logging functions
log_info() {
    echo -e "${BLUE}[INFO]${NC} $1" | tee -a "$LOG_FILE"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1" | tee -a "$LOG_FILE"
}

log_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1" | tee -a "$LOG_FILE"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1" | tee -a "$LOG_FILE"
}

# Check if command exists
command_exists() {
    command -v "$1" >/dev/null 2>&1
}

# Setup devnet directory
setup_devnet_dir() {
    if [ ! -d "$DEVNET_DIR" ]; then
        mkdir -p "$DEVNET_DIR"
        log_info "Created devnet directory: $DEVNET_DIR"
    fi

    if [ ! -f "$LOG_FILE" ]; then
        touch "$LOG_FILE"
    fi
}

# Generate devnet configuration
generate_config() {
    cat > "$CONFIG_FILE" << EOF
# Cytah-Speed Devnet Configuration
[network]
listen_addr = "/ip4/127.0.0.1/tcp/0"
bootstrap_peers = []

[rpc]
enabled = true
address = "127.0.0.1:8080"

[consensus]
mining_enabled = true
devnet_mode = true

[devnet]
data_dir = "$DEVNET_DIR/data"
test_accounts = [
    { address = "0x1234567890123456789012345678901234567890", balance = "1000000" },
    { address = "0x0987654321098765432109876543210987654321", balance = "500000" }
]
EOF
    log_success "Generated devnet configuration: $CONFIG_FILE"
}

# Start devnet
start_devnet() {
    log_info "Starting Cytah-Speed devnet..."

    # Check if already running
    if pgrep -f "cytah.*devnet" > /dev/null; then
        log_warning "Devnet appears to already be running"
        return 1
    fi

    # Build the project if needed
    if [ ! -f "$PROJECT_ROOT/target/release/cytah" ]; then
        log_info "Building Cytah-Speed..."
        cd "$PROJECT_ROOT"
        cargo build --release
    fi

    # Start the node
    cd "$PROJECT_ROOT"
    nohup ./target/release/cytah --config "$CONFIG_FILE" > "$DEVNET_DIR/node.log" 2>&1 &
    echo $! > "$DEVNET_DIR/node.pid"

    log_success "Devnet started with PID $(cat "$DEVNET_DIR/node.pid")"
    log_info "RPC endpoint: http://127.0.0.1:8080"
    log_info "Logs: $DEVNET_DIR/node.log"
}

# Stop devnet
stop_devnet() {
    if [ -f "$DEVNET_DIR/node.pid" ]; then
        PID=$(cat "$DEVNET_DIR/node.pid")
        if kill -0 "$PID" 2>/dev/null; then
            log_info "Stopping devnet (PID: $PID)..."
            kill "$PID"
            sleep 2
            if kill -0 "$PID" 2>/dev/null; then
                log_warning "Force killing devnet..."
                kill -9 "$PID"
            fi
            log_success "Devnet stopped"
        else
            log_warning "Devnet process not found"
        fi
        rm -f "$DEVNET_DIR/node.pid"
    else
        log_warning "No devnet PID file found"
    fi
}

# Reset devnet
reset_devnet() {
    log_info "Resetting devnet..."

    stop_devnet

    # Remove data directory
    if [ -d "$DEVNET_DIR/data" ]; then
        rm -rf "$DEVNET_DIR/data"
        log_info "Removed data directory"
    fi

    # Clear logs
    > "$LOG_FILE"
    > "$DEVNET_DIR/node.log" 2>/dev/null || true

    log_success "Devnet reset complete"
}

# Check devnet status
status_devnet() {
    if [ -f "$DEVNET_DIR/node.pid" ]; then
        PID=$(cat "$DEVNET_DIR/node.pid")
        if kill -0 "$PID" 2>/dev/null; then
            log_success "Devnet is running (PID: $PID)"
            return 0
        else
            log_warning "Devnet PID file exists but process is not running"
            rm -f "$DEVNET_DIR/node.pid"
            return 1
        fi
    else
        log_info "Devnet is not running"
        return 1
    fi
}

# Mint test tokens
mint_test_tokens() {
    log_info "Minting test tokens..."

    # This would typically call an RPC endpoint or use a special devnet command
    # For now, we'll just log the action
    log_warning "Test token minting not yet implemented"
    log_info "Test accounts configured in $CONFIG_FILE"
}

# Spawn test nodes
spawn_test_nodes() {
    local num_nodes=${1:-2}

    log_info "Spawning $num_nodes test nodes..."

    for i in $(seq 1 "$num_nodes"); do
        local port=$((8080 + i))
        local config_file="$DEVNET_DIR/node$i.toml"

        # Generate node-specific config
        cat > "$config_file" << EOF
[network]
listen_addr = "/ip4/127.0.0.1/tcp/0"
bootstrap_peers = []

[rpc]
enabled = true
address = "127.0.0.1:$port"

[consensus]
mining_enabled = true
devnet_mode = true

[devnet]
data_dir = "$DEVNET_DIR/data$i"
node_id = $i
EOF

        # Start the node
        cd "$PROJECT_ROOT"
        nohup ./target/release/cytah --config "$config_file" > "$DEVNET_DIR/node$i.log" 2>&1 &
        echo $! > "$DEVNET_DIR/node$i.pid"

        log_success "Started test node $i (PID: $(cat "$DEVNET_DIR/node$i.pid"), RPC: http://127.0.0.1:$port)"
    done
}

# Stop test nodes
stop_test_nodes() {
    log_info "Stopping test nodes..."

    for pid_file in "$DEVNET_DIR"/node[0-9]*.pid; do
        if [ -f "$pid_file" ]; then
            PID=$(cat "$pid_file")
            if kill -0 "$PID" 2>/dev/null; then
                kill "$PID"
                log_info "Stopped test node (PID: $PID)"
            fi
            rm -f "$pid_file"
        fi
    done

    log_success "All test nodes stopped"
}

# Show usage
usage() {
    cat << EOF
Cytah-Speed Devnet Tools

USAGE:
    $0 <command> [options]

COMMANDS:
    start           Start the devnet
    stop            Stop the devnet
    reset           Reset the devnet (stops and clears data)
    status          Check devnet status
    mint-tokens     Mint test tokens to configured accounts
    spawn-nodes     Spawn additional test nodes
    stop-nodes      Stop all test nodes
    logs            Show devnet logs
    help            Show this help message

EXAMPLES:
    $0 start                    # Start devnet
    $0 reset                    # Reset devnet
    $0 spawn-nodes 3           # Spawn 3 test nodes
    $0 logs                     # Show logs

DEVNET DIRECTORY: $DEVNET_DIR
CONFIG FILE: $CONFIG_FILE
LOG FILE: $LOG_FILE
EOF
}

# Show logs
show_logs() {
    if [ -f "$DEVNET_DIR/node.log" ]; then
        tail -f "$DEVNET_DIR/node.log"
    else
        log_error "No devnet logs found"
    fi
}

# Main script logic
main() {
    setup_devnet_dir

    case "${1:-help}" in
        start)
            generate_config
            start_devnet
            ;;
        stop)
            stop_devnet
            ;;
        reset)
            reset_devnet
            ;;
        status)
            status_devnet
            ;;
        mint-tokens)
            mint_test_tokens
            ;;
        spawn-nodes)
            spawn_test_nodes "$2"
            ;;
        stop-nodes)
            stop_test_nodes
            ;;
        logs)
            show_logs
            ;;
        help|--help|-h)
            usage
            ;;
        *)
            log_error "Unknown command: $1"
            usage
            exit 1
            ;;
    esac
}

# Run main function with all arguments
main "$@"