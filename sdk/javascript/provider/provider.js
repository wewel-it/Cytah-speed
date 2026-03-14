/**
 * Cytah-Speed Web3 Provider
 * Browser-compatible provider for dApp development
 */

class CytahProvider {
    constructor(endpoint = 'http://localhost:8080') {
        this.endpoint = endpoint;
        this.wsEndpoint = endpoint.replace('http', 'ws');
        this.eventListeners = new Map();
        this.subscriptions = new Map();
        this.nextId = 1;
    }

    /**
     * Connect to a Cytah-Speed node
     */
    async connect() {
        try {
            // Test connection with a simple RPC call
            const response = await this.rpcCall('get_node_info', []);
            this.connected = true;
            return true;
        } catch (error) {
            this.connected = false;
            throw new Error(`Failed to connect to Cytah-Speed node: ${error.message}`);
        }
    }

    /**
     * Make an RPC call to the node
     */
    async rpcCall(method, params = []) {
        const payload = {
            jsonrpc: '2.0',
            id: this.nextId++,
            method,
            params
        };

        const response = await fetch(`${this.endpoint}/api/v1/rpc`, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
            },
            body: JSON.stringify(payload)
        });

        if (!response.ok) {
            throw new Error(`RPC call failed: ${response.statusText}`);
        }

        const result = await response.json();

        if (result.error) {
            throw new Error(`RPC error: ${result.error.message}`);
        }

        return result.result;
    }

    /**
     * Get account balance
     */
    async getBalance(address) {
        return await this.rpcCall('get_balance', [address]);
    }

    /**
     * Get transaction details
     */
    async getTransaction(txHash) {
        return await this.rpcCall('get_transaction', [txHash]);
    }

    /**
     * Get block information
     */
    async getBlock(height) {
        return await this.rpcCall('get_block', [height]);
    }

    /**
     * Send a signed transaction
     */
    async sendTransaction(signedTx) {
        return await this.rpcCall('send_transaction', [signedTx]);
    }

    /**
     * Deploy a smart contract
     */
    async deployContract(wasmCode, initArgs = null) {
        const params = [wasmCode];
        if (initArgs) params.push(initArgs);
        return await this.rpcCall('deploy_contract', params);
    }

    /**
     * Call a smart contract method
     */
    async callContract(contractAddress, method, args = null) {
        const params = [contractAddress, method];
        if (args) params.push(args);
        return await this.rpcCall('call_contract', params);
    }

    /**
     * Subscribe to blockchain events via WebSocket
     */
    subscribe(eventType, callback) {
        if (!this.wsConnection) {
            this.connectWebSocket();
        }

        const subscriptionId = `sub_${this.nextId++}`;
        this.eventListeners.set(subscriptionId, { eventType, callback });

        // Send subscription message
        if (this.wsConnection && this.wsConnection.readyState === WebSocket.OPEN) {
            this.wsConnection.send(JSON.stringify({
                type: 'Subscribe',
                data: { event_types: [eventType] }
            }));
        }

        this.subscriptions.set(subscriptionId, eventType);
        return subscriptionId;
    }

    /**
     * Unsubscribe from events
     */
    unsubscribe(subscriptionId) {
        if (this.subscriptions.has(subscriptionId)) {
            const eventType = this.subscriptions.get(subscriptionId);

            if (this.wsConnection && this.wsConnection.readyState === WebSocket.OPEN) {
                this.wsConnection.send(JSON.stringify({
                    type: 'Unsubscribe',
                    data: { event_types: [eventType] }
                }));
            }

            this.eventListeners.delete(subscriptionId);
            this.subscriptions.delete(subscriptionId);
        }
    }

    /**
     * Subscribe to new blocks
     */
    onNewBlock(callback) {
        return this.subscribe('new_block', callback);
    }

    /**
     * Subscribe to new transactions
     */
    onTransaction(callback) {
        return this.subscribe('new_transaction', callback);
    }

    /**
     * Subscribe to contract events
     */
    onContractEvent(callback) {
        return this.subscribe('contract_event', callback);
    }

    /**
     * Connect to WebSocket for real-time events
     */
    connectWebSocket() {
        try {
            this.wsConnection = new WebSocket(`${this.wsEndpoint}/events`);

            this.wsConnection.onopen = () => {
                console.log('Connected to Cytah-Speed WebSocket');
            };

            this.wsConnection.onmessage = (event) => {
                try {
                    const message = JSON.parse(event.data);

                    if (message.type === 'Event') {
                        const blockchainEvent = message.data;
                        this.handleEvent(blockchainEvent);
                    }
                } catch (error) {
                    console.error('Failed to parse WebSocket message:', error);
                }
            };

            this.wsConnection.onclose = () => {
                console.log('WebSocket connection closed');
                // Attempt to reconnect after a delay
                setTimeout(() => this.connectWebSocket(), 5000);
            };

            this.wsConnection.onerror = (error) => {
                console.error('WebSocket error:', error);
            };

        } catch (error) {
            console.error('Failed to connect to WebSocket:', error);
        }
    }

    /**
     * Handle incoming blockchain events
     */
    handleEvent(event) {
        // Notify all listeners for this event type
        for (const [id, listener] of this.eventListeners) {
            if (listener.eventType === event.event_type ||
                listener.eventType === 'all') {
                try {
                    listener.callback(event);
                } catch (error) {
                    console.error('Event listener error:', error);
                }
            }
        }
    }

    /**
     * Check if provider is connected
     */
    isConnected() {
        return this.connected || false;
    }

    /**
     * Get provider information
     */
    getInfo() {
        return {
            name: 'Cytah-Speed Provider',
            version: '1.0.0',
            endpoint: this.endpoint,
            connected: this.isConnected()
        };
    }
}

// Export for different environments
if (typeof module !== 'undefined' && module.exports) {
    // Node.js
    module.exports = CytahProvider;
} else if (typeof define === 'function' && define.amd) {
    // AMD
    define([], function() { return CytahProvider; });
} else if (typeof window !== 'undefined') {
    // Browser global
    window.CytahProvider = CytahProvider;
}