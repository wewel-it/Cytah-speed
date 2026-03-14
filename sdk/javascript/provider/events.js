/**
 * Cytah-Speed Event Management
 * Handles real-time event subscriptions and notifications
 */

class CytahEvents {
    constructor(provider) {
        this.provider = provider;
        this.listeners = new Map();
        this.subscriptions = new Set();
    }

    /**
     * Subscribe to new block events
     */
    onNewBlock(callback) {
        return this.subscribe('new_block', callback);
    }

    /**
     * Subscribe to new transaction events
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
     * Subscribe to peer events
     */
    onPeerEvent(callback) {
        return this.subscribe('peer_connected', callback);
        return this.subscribe('peer_disconnected', callback);
    }

    /**
     * Subscribe to all events
     */
    onAllEvents(callback) {
        return this.subscribe('all', callback);
    }

    /**
     * Internal subscription method
     */
    subscribe(eventType, callback) {
        const subscriptionId = this.provider.subscribe(eventType, callback);
        this.listeners.set(subscriptionId, { eventType, callback });
        this.subscriptions.add(subscriptionId);
        return subscriptionId;
    }

    /**
     * Unsubscribe from events
     */
    unsubscribe(subscriptionId) {
        if (this.subscriptions.has(subscriptionId)) {
            this.provider.unsubscribe(subscriptionId);
            this.listeners.delete(subscriptionId);
            this.subscriptions.delete(subscriptionId);
        }
    }

    /**
     * Unsubscribe from all events
     */
    unsubscribeAll() {
        for (const subscriptionId of this.subscriptions) {
            this.provider.unsubscribe(subscriptionId);
        }
        this.listeners.clear();
        this.subscriptions.clear();
    }

    /**
     * Get active subscriptions
     */
    getSubscriptions() {
        return Array.from(this.subscriptions);
    }

    /**
     * Get subscription count
     */
    getSubscriptionCount() {
        return this.subscriptions.size;
    }

    /**
     * Check if connected to WebSocket
     */
    isConnected() {
        return this.provider.wsConnection &&
               this.provider.wsConnection.readyState === WebSocket.OPEN;
    }
}

// Export for different environments
if (typeof module !== 'undefined' && module.exports) {
    // Node.js
    module.exports = CytahEvents;
} else if (typeof define === 'function' && define.amd) {
    // AMD
    define([], function() { return CytahEvents; });
} else if (typeof window !== 'undefined') {
    // Browser global
    window.CytahEvents = CytahEvents;
}